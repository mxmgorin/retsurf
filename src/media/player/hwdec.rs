//! Hardware video decoding through libpmhwdec (<https://github.com/Knifethrower/pmhwdec>):
//! the Allwinner video engine, Rockchip MPP and V4L2 decoders of handheld SoCs.
//!
//! The library is LGPL, so it is never linked: it is opened at run time and its
//! version 1 API looked up by name. Missing library, missing symbols, no
//! hardware for the codec or a refused stream all mean software decoding
//! (see [`super::decode`]). Where it is looked for:
//!
//! - `RETSURF_PMHWDEC=<path>`, or `RETSURF_PMHWDEC=0` to never use it;
//! - `libpmhwdec.so.1` next to the executable (how a port ships it);
//! - `libpmhwdec.so.1` on the system's library path.
//!
//! The library's own switches still apply (`PMHWDEC_LOG=1`, `PMHWDEC_BACKEND=off`).

use std::ffi::{c_char, c_int, c_void, CStr};
use std::ptr::NonNull;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

/// `PmhwdecCodec`.
pub(crate) const CODEC_H264: c_int = 0;

/// `pmhwdec_send` results.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const OK: c_int = 0;
pub(crate) const AGAIN: c_int = 1;
pub(crate) const ERROR: c_int = -1;
pub(crate) const UNSUPPORTED: c_int = -2;

/// `PMHWDEC_FMT_NV12`, the only output format of version 1.
const FMT_NV12: c_int = 1;

/// How long `send` waits on `AGAIN` with no picture coming out before it gives
/// up on the access unit.
const AGAIN_PATIENCE: Duration = Duration::from_secs(2);

/// `PmhwdecCaps`.
#[repr(C)]
pub(crate) struct Caps {
    pub(crate) max_width: c_int,
    pub(crate) max_height: c_int,
    pub(crate) max_bit_depth: c_int,
    pub(crate) backend: *const c_char,
}

/// `PmhwdecPicture`: visible size and offset, NV12 planes (CPU-readable, maybe
/// uncached), the pts given to `send`, and a dmabuf this player does not use
/// (Servo takes CPU frames on this path).
#[repr(C)]
pub(crate) struct Picture {
    pub(crate) width: c_int,
    pub(crate) height: c_int,
    pub(crate) left: c_int,
    pub(crate) top: c_int,
    pub(crate) format: c_int,
    pub(crate) plane: [*const u8; 2],
    pub(crate) stride: [c_int; 2],
    pub(crate) pts: i64,
    pub(crate) corrupt: c_int,
    pub(crate) dmabuf_fd: c_int,
    pub(crate) drm_fourcc: u32,
    pub(crate) drm_modifier: u64,
    pub(crate) offset: [u32; 2],
    pub(crate) private: *mut c_void,
}

impl Picture {
    fn empty() -> Self {
        Self {
            width: 0,
            height: 0,
            left: 0,
            top: 0,
            format: 0,
            plane: [std::ptr::null(); 2],
            stride: [0; 2],
            pts: 0,
            corrupt: 0,
            dmabuf_fd: -1,
            drm_fourcc: 0,
            drm_modifier: 0,
            offset: [0; 2],
            private: std::ptr::null_mut(),
        }
    }
}

/// A picture borrowed from the decoder: tight copies of its visible area.
pub(crate) struct Nv12<'a> {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) pts: i64,
    pub(crate) y: &'a [u8],
    pub(crate) y_stride: usize,
    /// Interleaved U, V; starts at the visible area's first chroma pair.
    pub(crate) uv: &'a [u8],
    pub(crate) uv_stride: usize,
}

/// The version 1 API, by name.
pub(crate) struct Lib {
    pub(crate) query: unsafe extern "C" fn(c_int, *mut Caps) -> c_int,
    pub(crate) open: unsafe extern "C" fn(c_int, c_int, c_int) -> *mut c_void,
    pub(crate) send: unsafe extern "C" fn(*mut c_void, *const u8, c_int, i64) -> c_int,
    pub(crate) get: unsafe extern "C" fn(*mut c_void, *mut Picture) -> c_int,
    pub(crate) put: unsafe extern "C" fn(*mut c_void, *mut Picture),
    pub(crate) flush: unsafe extern "C" fn(*mut c_void),
    pub(crate) close: unsafe extern "C" fn(*mut c_void),
    pub(crate) backend_name: unsafe extern "C" fn(*const c_void) -> *const c_char,
}

impl Lib {
    /// The process-wide library, loaded on first use; `None` = software only.
    pub(crate) fn get() -> Option<&'static Lib> {
        static LIB: OnceLock<Option<Lib>> = OnceLock::new();
        LIB.get_or_init(load).as_ref()
    }

    /// Whether the hardware decodes `codec` at `width` x `height` (0 = unknown).
    pub(crate) fn supports(&self, codec: c_int, width: u32, height: u32) -> Option<String> {
        let mut caps = Caps {
            max_width: 0,
            max_height: 0,
            max_bit_depth: 0,
            backend: std::ptr::null(),
        };
        // SAFETY: `caps` is a valid out-parameter of the declared layout.
        if unsafe { (self.query)(codec, &mut caps) } != 0 {
            return None;
        }
        let too_big = |size: u32, max: c_int| max > 0 && size > max as u32;
        if too_big(width, caps.max_width) || too_big(height, caps.max_height) {
            return None;
        }
        Some(c_text(caps.backend))
    }
}

fn c_text(text: *const c_char) -> String {
    if text.is_null() {
        return "?".into();
    }
    // SAFETY: the library returns NUL-terminated static strings.
    unsafe { CStr::from_ptr(text) }.to_string_lossy().into_owned()
}

#[cfg(target_os = "linux")]
fn load() -> Option<Lib> {
    let chosen = std::env::var_os("RETSURF_PMHWDEC").filter(|v| !v.is_empty());
    if chosen.as_deref() == Some(std::ffi::OsStr::new("0")) {
        return None;
    }
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    match chosen {
        Some(path) => paths.push(path.into()),
        None => {
            let exe = std::env::current_exe().ok();
            if let Some(dir) = exe.as_deref().and_then(std::path::Path::parent) {
                paths.push(dir.join("libpmhwdec.so.1"));
            }
            paths.push("libpmhwdec.so.1".into());
        }
    }
    for path in &paths {
        // SAFETY: loading runs the library's initializers; libpmhwdec has none
        // with side effects beyond its own state.
        let library = match unsafe { libloading::Library::new(path) } {
            Ok(library) => library,
            Err(e) => {
                log::debug!("pmhwdec: {}: {e}", path.display());
                continue;
            }
        };
        return match load_from(library) {
            Some(lib) => {
                log::info!("pmhwdec: {} loaded", path.display());
                Some(lib)
            }
            None => {
                log::warn!("pmhwdec: {} lacks the version 1 API: not used", path.display());
                None
            }
        };
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn load() -> Option<Lib> {
    None
}

/// The API out of an opened library; the library stays loaded for good.
#[cfg(target_os = "linux")]
pub(crate) fn load_from(library: libloading::Library) -> Option<Lib> {
    // SAFETY: each symbol is the C function of that name in pmhwdec.h (version
    // 1), with the signature given here.
    let lib = unsafe {
        Lib {
            query: *library.get(b"pmhwdec_query\0").ok()?,
            open: *library.get(b"pmhwdec_open\0").ok()?,
            send: *library.get(b"pmhwdec_send\0").ok()?,
            get: *library.get(b"pmhwdec_get\0").ok()?,
            put: *library.get(b"pmhwdec_put\0").ok()?,
            flush: *library.get(b"pmhwdec_flush\0").ok()?,
            close: *library.get(b"pmhwdec_close\0").ok()?,
            backend_name: *library.get(b"pmhwdec_backend_name\0").ok()?,
        }
    };
    // The function pointers above live as long as the library: never unload it.
    std::mem::forget(library);
    Some(lib)
}

/// One open hardware decoder; closed on drop.
pub(crate) struct HwDecoder {
    lib: &'static Lib,
    dec: NonNull<c_void>,
}

impl HwDecoder {
    /// `None`: no hardware for this stream (software decodes it).
    pub(crate) fn open(lib: &'static Lib, codec: c_int, width: u32, height: u32) -> Option<Self> {
        let clamp = |v: u32| v.min(c_int::MAX as u32) as c_int;
        // SAFETY: plain values in, an owned handle or NULL out.
        let dec = unsafe { (lib.open)(codec, clamp(width), clamp(height)) };
        NonNull::new(dec).map(|dec| Self { lib, dec })
    }

    pub(crate) fn backend(&self) -> String {
        // SAFETY: a live handle.
        c_text(unsafe { (self.lib.backend_name)(self.dec.as_ptr()) })
    }

    /// Sends one access unit (empty = end of stream), handing pictures to
    /// `each` while the decoder has no free buffer. [`OK`], [`ERROR`] (the unit
    /// was dropped) or [`UNSUPPORTED`] (software must take the stream).
    pub(crate) fn send(&mut self, data: &[u8], pts: i64, each: &mut dyn FnMut(Nv12)) -> c_int {
        let (ptr, size) = if data.is_empty() {
            (std::ptr::null(), 0)
        } else {
            (data.as_ptr(), data.len().min(c_int::MAX as usize) as c_int)
        };
        let mut idle = Duration::ZERO;
        loop {
            // SAFETY: a live handle; `ptr` is valid for `size` bytes or NULL/0.
            let r = unsafe { (self.lib.send)(self.dec.as_ptr(), ptr, size, pts) };
            if r != AGAIN {
                return r;
            }
            if self.drain(each) == 0 {
                if idle >= AGAIN_PATIENCE {
                    log::warn!("pmhwdec: no picture buffer frees up; unit dropped");
                    return ERROR;
                }
                thread::sleep(Duration::from_millis(1));
                idle += Duration::from_millis(1);
            }
        }
    }

    /// Every picture ready, in display order, each returned to the decoder
    /// right after `each` saw it. The count.
    pub(crate) fn drain(&mut self, each: &mut dyn FnMut(Nv12)) -> usize {
        let mut count = 0;
        loop {
            let mut pic = Picture::empty();
            // SAFETY: a live handle and an out-parameter of the declared layout.
            if unsafe { (self.lib.get)(self.dec.as_ptr(), &mut pic) } != 1 {
                return count;
            }
            count += 1;
            if let Some(view) = view(&pic) {
                each(view);
            } else {
                log::debug!("pmhwdec: picture format {} skipped", pic.format);
            }
            // SAFETY: the picture `get` just returned, given back once.
            unsafe { (self.lib.put)(self.dec.as_ptr(), &mut pic) };
        }
    }

    /// Drops everything queued (a seek); send from a key frame afterwards.
    pub(crate) fn flush(&mut self) {
        // SAFETY: a live handle.
        unsafe { (self.lib.flush)(self.dec.as_ptr()) };
    }
}

impl Drop for HwDecoder {
    fn drop(&mut self) {
        // SAFETY: a live handle, closed once.
        unsafe { (self.lib.close)(self.dec.as_ptr()) };
    }
}

/// The visible area of an NV12 picture as slices; `None` for anything else.
fn view(pic: &Picture) -> Option<Nv12<'_>> {
    if pic.format != FMT_NV12 || pic.width <= 0 || pic.height <= 0 {
        return None;
    }
    let (width, height) = (pic.width as usize, pic.height as usize);
    let (left, top) = (pic.left.max(0) as usize, pic.top.max(0) as usize);
    let (y_stride, uv_stride) = (pic.stride[0] as usize, pic.stride[1] as usize);
    let uv_row = width.div_ceil(2) * 2;
    if pic.plane.iter().any(|p| p.is_null())
        || y_stride < left + width
        || uv_stride < (left & !1) + uv_row
    {
        return None;
    }
    let y_start = top * y_stride + left;
    let uv_start = (top / 2) * uv_stride + (left & !1);
    let y_len = y_start + (height - 1) * y_stride + width;
    let uv_len = uv_start + (height.div_ceil(2) - 1) * uv_stride + uv_row;
    // SAFETY: the library guarantees each plane holds `stride` bytes for every
    // row of the picture (visible area plus its offset).
    let (y, uv) = unsafe {
        (
            std::slice::from_raw_parts(pic.plane[0], y_len),
            std::slice::from_raw_parts(pic.plane[1], uv_len),
        )
    };
    Some(Nv12 {
        width,
        height,
        pts: pic.pts,
        y: &y[y_start..],
        y_stride,
        uv: &uv[uv_start..],
        uv_stride,
    })
}
