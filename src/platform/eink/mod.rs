//! E-ink panel output through FBInk (the `eink` feature). The composed BGRA
//! frame is grayscaled one changed rect at a time and handed to FBInk, which
//! owns the panel's refresh ioctls; the SDL window it stands in for is never shown.
//!
//! Every frame is a partial refresh, since flashing mid-scroll blinks the panel
//! black several times a second. Their ghosting is cleared by one flash once the
//! panel settles after about a panel's worth of change, from a thread of its own
//! because no frame follows. FBInk keeps its state in globals, so every call into
//! it holds [`Shared`]'s lock.
//!
//! Touch comes from the same device's evdev node ([`touch`]).
//!
//! The C side is `fbink_shim.c`, linked from `target/fbink` (`tools/kindle/fbink.sh`).

mod touch;

use crate::platform::render::BYTES_PER_PIXEL;
use sdl2::rect::Rect;
use std::ffi::c_int;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Change since the last flash, in panels' worth, that earns the next one.
const FLASH_AFTER_PANELS: f32 = 1.0;
/// How long the panel stays unchanged before that flash.
const FLASH_SETTLE: Duration = Duration::from_millis(600);

/// BT.601 luma weights in 1/256ths; they sum to 256.
const LUMA_R: u32 = 77;
const LUMA_G: u32 = 150;
const LUMA_B: u32 = 29;

extern "C" {
    fn retsurf_eink_open(width: *mut u32, height: *mut u32, dpi: *mut u32) -> c_int;
    fn retsurf_eink_draw(gray: *const u8, w: c_int, h: c_int, x: c_int, y: c_int) -> c_int;
    fn retsurf_eink_flash() -> c_int;
    fn retsurf_eink_touch_quirks(swap_axes: *mut bool, mirror_x: *mut bool, mirror_y: *mut bool);
    fn retsurf_eink_close();
}

/// The open e-ink framebuffer. One per process: FBInk keeps its state in globals.
pub struct Panel {
    pub width: u32,
    pub height: u32,
    /// One rect's worth of gray pixels, reused across frames.
    gray: Vec<u8>,
    shared: Arc<Shared>,
    flasher: Option<JoinHandle<()>>,
}

/// What the frame path and the flash thread share; the lock also serializes FBInk.
struct Shared {
    state: Mutex<FlashState>,
    changed: Condvar,
}

struct FlashState {
    /// Change since the last flash, in panels' worth.
    pending: f32,
    last_change: Instant,
    closing: bool,
}

impl Panel {
    /// Opens the panel and starts its touch reader; `None` where there is no e-ink
    /// framebuffer. SDL's events subsystem must already be up.
    pub fn open() -> Option<Self> {
        let (mut width, mut height, mut dpi) = (0, 0, 0);
        // SAFETY: three valid out-pointers; the shim writes them only on success.
        if unsafe { retsurf_eink_open(&mut width, &mut height, &mut dpi) } < 0 {
            log::info!("eink: no panel");
            return None;
        }
        log::info!("eink: panel {width}x{height} at {dpi} dpi");
        let mut quirks = touch::Quirks {
            swap_axes: false,
            mirror_x: false,
            mirror_y: false,
        };
        // SAFETY: three valid out-pointers, read after a successful open.
        unsafe {
            retsurf_eink_touch_quirks(
                &mut quirks.swap_axes,
                &mut quirks.mirror_x,
                &mut quirks.mirror_y,
            )
        };
        touch::spawn(quirks);

        let shared = Arc::new(Shared {
            state: Mutex::new(FlashState {
                pending: 0.0,
                last_change: Instant::now(),
                closing: false,
            }),
            changed: Condvar::new(),
        });
        let flasher = std::thread::Builder::new()
            .name("eink-flash".into())
            .spawn({
                let shared = shared.clone();
                move || flash_loop(&shared)
            })
            .map_err(|err| log::error!("eink: flash thread not started ({err}); ghosting stays"))
            .ok();
        Some(Self {
            width,
            height,
            gray: Vec::new(),
            shared,
            flasher,
        })
    }

    /// Draws `region` of a panel-sized BGRA frame, `pitch` bytes a row, as a
    /// partial refresh.
    pub fn present(&mut self, bgra: &[u8], pitch: usize, region: Rect) {
        let Some(region) = region.intersection(Rect::new(0, 0, self.width, self.height)) else {
            return;
        };
        let (x, y) = (region.x() as usize, region.y() as usize);
        let (w, h) = (region.width() as usize, region.height() as usize);
        self.gray.clear();
        self.gray.reserve(w * h);
        for row in y..y + h {
            let start = row * pitch + x * BYTES_PER_PIXEL;
            let pixels = &bgra[start..start + w * BYTES_PER_PIXEL];
            self.gray
                .extend(pixels.as_chunks::<BYTES_PER_PIXEL>().0.iter().map(luma));
        }

        let mut state = self.shared.lock();
        // SAFETY: `gray` holds exactly w * h bytes, row-major with no padding.
        let rc = unsafe {
            retsurf_eink_draw(
                self.gray.as_ptr(),
                w as c_int,
                h as c_int,
                x as c_int,
                y as c_int,
            )
        };
        if rc < 0 {
            log::error!("eink: draw failed ({rc})");
        }
        state.pending += (w * h) as f32 / (self.width * self.height) as f32;
        state.last_change = Instant::now();
        self.shared.changed.notify_one();
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, FlashState> {
        // A panic mid-draw leaves only a refresh count behind; keep going.
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Flashes the panel once it has gone [`FLASH_SETTLE`] without change after
/// [`FLASH_AFTER_PANELS`] of it; returns when `closing` is set.
fn flash_loop(shared: &Shared) {
    let mut state = shared.lock();
    while !state.closing {
        if state.pending < FLASH_AFTER_PANELS {
            state = shared
                .changed
                .wait(state)
                .unwrap_or_else(|p| p.into_inner());
            continue;
        }
        let due = state.last_change + FLASH_SETTLE;
        let now = Instant::now();
        if now < due {
            let (guard, _) = (shared.changed)
                .wait_timeout(state, due - now)
                .unwrap_or_else(|p| p.into_inner());
            state = guard;
            continue;
        }
        // SAFETY: the panel is open until `closing`, and the lock is held.
        let rc = unsafe { retsurf_eink_flash() };
        if rc < 0 {
            log::warn!("eink: flash failed ({rc})");
        }
        state.pending = 0.0;
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        self.shared.lock().closing = true;
        self.shared.changed.notify_one();
        if let Some(flasher) = self.flasher.take() {
            let _ = flasher.join();
        }
        // SAFETY: closing is idempotent in the shim, and the flash thread is gone.
        unsafe { retsurf_eink_close() };
    }
}

/// Converts one BGRA pixel; returns its 8-bit gray.
fn luma(px: &[u8; BYTES_PER_PIXEL]) -> u8 {
    let (b, g, r) = (px[0] as u32, px[1] as u32, px[2] as u32);
    ((r * LUMA_R + g * LUMA_G + b * LUMA_B) >> 8) as u8
}
