//! The panel's multitouch device read straight from evdev, since SDL's dummy
//! video driver opens no input. The first slot (type B protocol) becomes SDL
//! finger events in window-normalized coordinates. The device is grabbed so the
//! firmware UI underneath does not act on the same touches.

use std::ffi::c_int;
use std::fs::File;
use std::io::Read;
use std::mem::size_of;
use std::os::fd::AsRawFd;

const INPUT_DIR: &str = "/dev/input";

const EV_SYN: u16 = 0x00;
const EV_ABS: u16 = 0x03;
const SYN_REPORT: u16 = 0x00;
const ABS_MT_SLOT: u16 = 0x2f;
const ABS_MT_POSITION_X: u16 = 0x35;
const ABS_MT_POSITION_Y: u16 = 0x36;
const ABS_MT_TRACKING_ID: u16 = 0x39;

/// `_IOC` fields for evdev's `'E'` ioctls.
const IOC_READ: u32 = 2;
const IOC_WRITE: u32 = 1;
const EVDEV_IOC_TYPE: u32 = b'E' as u32;
const EVIOCGABS_BASE: u32 = 0x40;
const EVIOCGRAB_NR: u32 = 0x90;

/// SDL's id for the one touch device this reader stands in for.
const TOUCH_ID: i64 = 1;

/// How the touch axes map onto the view.
#[derive(Clone, Copy)]
pub struct Quirks {
    pub swap_axes: bool,
    pub mirror_x: bool,
    pub mirror_y: bool,
}

/// Starts a thread turning the first multitouch device's touches into SDL finger
/// events; a missing device is only logged.
pub fn spawn(quirks: Quirks) {
    let Some((file, x_axis, y_axis)) = find_device() else {
        log::warn!("eink: no multitouch device under {INPUT_DIR}");
        return;
    };
    let grab = ioc(IOC_WRITE, EVIOCGRAB_NR, size_of::<c_int>());
    // SAFETY: a valid evdev fd; the int argument means "grab".
    if unsafe { libc::ioctl(file.as_raw_fd(), grab, 1) } < 0 {
        log::warn!("eink: touch device not grabbed; the firmware sees touches too");
    }
    let spawned = std::thread::Builder::new()
        .name("eink-touch".into())
        .spawn(move || read_loop(file, x_axis, y_axis, quirks));
    if let Err(err) = spawned {
        log::error!("eink: touch thread not started ({err})");
    }
}

/// A touch axis's raw range.
#[derive(Clone, Copy)]
struct Axis {
    min: i32,
    max: i32,
}

impl Axis {
    fn normalize(self, value: i32) -> f32 {
        let span = (self.max - self.min).max(1) as f32;
        ((value - self.min) as f32 / span).clamp(0.0, 1.0)
    }
}

fn find_device() -> Option<(File, Axis, Axis)> {
    let mut paths: Vec<_> = std::fs::read_dir(INPUT_DIR)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("event"))
        })
        .collect();
    paths.sort();
    paths.into_iter().find_map(|path| {
        let file = File::open(&path).ok()?;
        let x = abs_axis(&file, ABS_MT_POSITION_X)?;
        let y = abs_axis(&file, ABS_MT_POSITION_Y)?;
        log::info!(
            "eink: touch {} x {}..{} y {}..{}",
            path.display(),
            x.min,
            x.max,
            y.min,
            y.max
        );
        Some((file, x, y))
    })
}

/// The axis's range, or `None` where the device has no such axis.
fn abs_axis(file: &File, code: u16) -> Option<Axis> {
    // SAFETY: plain-old-data the kernel fills in.
    let mut info: libc::input_absinfo = unsafe { std::mem::zeroed() };
    let request = ioc(
        IOC_READ,
        EVIOCGABS_BASE + code as u32,
        size_of::<libc::input_absinfo>(),
    );
    // SAFETY: `info` is the size the request encodes.
    let rc = unsafe { libc::ioctl(file.as_raw_fd(), request, &mut info) };
    (rc >= 0 && info.maximum > info.minimum).then_some(Axis {
        min: info.minimum,
        max: info.maximum,
    })
}

/// Linux's `_IOC(dir, 'E', nr, size)`.
fn ioc(dir: u32, nr: u32, size: usize) -> libc::Ioctl {
    ((dir << 30) | ((size as u32) << 16) | (EVDEV_IOC_TYPE << 8) | nr) as libc::Ioctl
}

/// The first slot's state between two `SYN_REPORT`s.
#[derive(Default)]
struct Slot {
    down: bool,
    was_down: bool,
    x: i32,
    y: i32,
    moved: bool,
}

fn read_loop(mut file: File, x_axis: Axis, y_axis: Axis, quirks: Quirks) {
    const EVENT_SIZE: usize = size_of::<libc::input_event>();
    let mut buf = [0u8; EVENT_SIZE * 64];
    let mut slot = 0;
    let mut first = Slot::default();
    loop {
        let n = match file.read(&mut buf) {
            Ok(n) => n,
            Err(err) => return log::error!("eink: touch read failed ({err})"),
        };
        for chunk in buf[..n].as_chunks::<EVENT_SIZE>().0 {
            // SAFETY: the kernel writes whole `input_event`s; read unaligned from bytes.
            let event: libc::input_event =
                unsafe { std::ptr::read_unaligned(chunk.as_ptr().cast()) };
            match (event.type_, event.code) {
                (EV_ABS, ABS_MT_SLOT) => slot = event.value,
                (EV_ABS, ABS_MT_TRACKING_ID) if slot == 0 => first.down = event.value >= 0,
                (EV_ABS, ABS_MT_POSITION_X) if slot == 0 => {
                    first.x = event.value;
                    first.moved = true;
                }
                (EV_ABS, ABS_MT_POSITION_Y) if slot == 0 => {
                    first.y = event.value;
                    first.moved = true;
                }
                (EV_SYN, SYN_REPORT) => report(&mut first, x_axis, y_axis, quirks),
                _ => {}
            }
        }
    }
}

fn report(slot: &mut Slot, x_axis: Axis, y_axis: Axis, quirks: Quirks) {
    let (mut x, mut y) = (x_axis.normalize(slot.x), y_axis.normalize(slot.y));
    if quirks.swap_axes {
        (x, y) = (y, x);
    }
    if quirks.mirror_x {
        x = 1.0 - x;
    }
    if quirks.mirror_y {
        y = 1.0 - y;
    }
    use sdl2::sys::SDL_EventType::{SDL_FINGERDOWN, SDL_FINGERMOTION, SDL_FINGERUP};
    let kind = match (slot.was_down, slot.down) {
        (false, true) => Some(SDL_FINGERDOWN),
        (true, true) if slot.moved => Some(SDL_FINGERMOTION),
        (true, false) => Some(SDL_FINGERUP),
        _ => None,
    };
    if let Some(kind) = kind {
        push_finger(kind as u32, x, y);
    }
    slot.was_down = slot.down;
    slot.moved = false;
}

fn push_finger(kind: u32, x: f32, y: f32) {
    // SAFETY: all-zero is a valid `SDL_TouchFingerEvent`; the fields set below
    // are the ones SDL reads for a finger event.
    let mut finger: sdl2::sys::SDL_TouchFingerEvent = unsafe { std::mem::zeroed() };
    finger.type_ = kind;
    finger.touchId = TOUCH_ID;
    finger.x = x;
    finger.y = y;
    finger.pressure = 1.0;
    let mut event = sdl2::sys::SDL_Event { tfinger: finger };
    // SAFETY: SDL_PushEvent is thread-safe once the events subsystem is up,
    // which it is before any window, and so before this thread exists.
    if unsafe { sdl2::sys::SDL_PushEvent(&mut event) } < 0 {
        log::warn!("eink: touch event dropped");
    }
}
