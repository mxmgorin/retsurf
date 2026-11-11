//! Hardware pictures to the screen on the GPU: the NV12 planes go up as two
//! small textures (3 MB a 1080p frame instead of 8 MB of BGRA) and a shader on a
//! GL context shared with Servo's turns them into RGBA, which WebRender samples
//! as an external image (Servo's GL video path, `media_glvideo_enabled`).
//!
//! On an H700 (RG 34XXSP, RG Cube XX) the CPU route spent ~28 ms of four cores
//! converting each 1080p frame, and WebRender's 8 MB texture upload was the
//! step that capped the frame rate; both go away here.
//!
//! The window publishes its EGL display and context with [`publish_current`]
//! before Servo is built; a player uses the route once Servo reports its GL
//! player thread running ([`set_glplayer_up`]). Any failure falls back to the
//! CPU conversion.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use servo_media::player::video::{Buffer, VideoFrame, VideoFrameData};

use super::video::{Frame, Nv12Planes};

/// The window's EGL display and context, once published.
static SHARE: OnceLock<(usize, usize)> = OnceLock::new();

/// Servo's GL player thread runs: texture frames are accepted.
static GLPLAYER_UP: AtomicBool = AtomicBool::new(false);

/// Off unless `RETSURF_GLVIDEO=1`: it needs a Servo whose media frame renderer
/// updates the image for each new texture frame. Without that, WebRender's
/// picture cache keeps showing the first frame.
fn enabled() -> bool {
    std::env::var("RETSURF_GLVIDEO").is_ok_and(|v| v == "1")
}

/// Called with the window's GL context current: hands its EGL display and
/// context to Servo (and keeps them for the paint threads). False when the
/// context is not EGL, or the route is switched off.
pub(crate) fn publish_current() -> bool {
    if !enabled() {
        return false;
    }
    let Some((display, context)) = imp::current() else {
        return false;
    };
    if SHARE.set((display, context)).is_err() {
        return true;
    }
    servo::Servo::initialize_gl_accelerated_media(
        servo::MediaNativeDisplay::Egl(display),
        servo::MediaGlApi::Gles2,
        servo::MediaGlContext::Egl(context),
    );
    log::info!("video: GL route offered to Servo (EGL display {display:#x})");
    true
}

/// Whether the window published a context (the pref follows this).
pub(crate) fn published() -> bool {
    SHARE.get().is_some()
}

/// From `create_player`: Servo hands a player its GL context only while the GL
/// player thread runs.
pub(crate) fn set_glplayer_up(up: bool) {
    GLPLAYER_UP.store(up, Ordering::Relaxed);
}

/// One RGBA render target handed to WebRender; free again once Servo drops
/// every frame naming it.
struct TexMark(u32);

impl Buffer for TexMark {
    fn to_vec(&self) -> Option<VideoFrameData> {
        Some(VideoFrameData::Texture(self.0))
    }
}

/// Render targets: the renderer holds a few while it composites.
const TARGETS: usize = 4;

/// Per paint thread; made on that thread, which its context stays current on.
pub(super) struct GlPainter {
    inner: imp::Gpu,
    targets: Vec<(u32, u32, Arc<TexMark>)>,
    size: (usize, usize),
}

pub(super) enum Painted {
    Frame(VideoFrame),
    /// Every target is still with the renderer: skip this frame.
    Busy,
}

impl GlPainter {
    /// `None` when the route is off or anything about the context fails.
    pub(super) fn new() -> Option<Self> {
        if !GLPLAYER_UP.load(Ordering::Relaxed) {
            return None;
        }
        let &(display, context) = SHARE.get()?;
        match imp::Gpu::new(display, context) {
            Ok(inner) => {
                log::info!("video: painting on the GPU ({})", inner.renderer());
                Some(Self {
                    inner,
                    targets: Vec::new(),
                    size: (0, 0),
                })
            }
            Err(e) => {
                log::warn!("video: GPU painting unavailable ({e}); converting on the CPU");
                None
            }
        }
    }

    /// Uploads and converts `frame` (NV12 only); `Err` sends this frame, and
    /// the stream, back to the CPU route.
    pub(super) fn paint(&mut self, frame: &Frame) -> Result<Painted, String> {
        let planes: Nv12Planes<'_> = frame.nv12().ok_or("not NV12")?;
        let (w, h) = (frame.width, frame.height);
        if self.size != (w, h) {
            self.inner.release(self.targets.drain(..).filter(|t| Arc::strong_count(&t.2) == 1));
            self.inner.resize(w, h)?;
            for _ in 0..TARGETS {
                let (tex, fbo) = self.inner.target(w, h)?;
                self.targets.push((tex, fbo, Arc::new(TexMark(tex))));
            }
            self.size = (w, h);
        }
        let Some(&(tex, fbo, ref mark)) = self.targets.iter().find(|t| Arc::strong_count(&t.2) == 1)
        else {
            return Ok(Painted::Busy);
        };
        let mark = mark.clone();
        self.inner.draw(&planes, w, h, fbo)?;
        let buffer: Arc<dyn Buffer> = mark;
        VideoFrame::new(w as i32, h as i32, buffer)
            .map(Painted::Frame)
            .ok_or_else(|| format!("texture {tex}: no frame"))
    }
}

impl Drop for GlPainter {
    fn drop(&mut self) {
        // Targets Servo still holds stay alive in the shared context group.
        self.inner.release(self.targets.drain(..).filter(|t| Arc::strong_count(&t.2) == 1));
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::ffi::{c_void, CStr, CString};
    use std::rc::Rc;

    use gleam::gl::{self, Gl};

    use super::super::video::Nv12Planes;

    type Display = *mut c_void;
    type Context = *mut c_void;
    type Config = *mut c_void;
    type Surface = *mut c_void;

    const EGL_NONE: i32 = 0x3038;
    const EGL_CONFIG_ID: i32 = 0x3028;
    const EGL_CONTEXT_CLIENT_VERSION: i32 = 0x3098;
    const EGL_OPENGL_ES_API: u32 = 0x30A0;
    const EGL_WIDTH: i32 = 0x3057;
    const EGL_HEIGHT: i32 = 0x3056;
    const EGL_EXTENSIONS: i32 = 0x3055;

    struct Egl {
        _lib: libloading::Library,
        get_proc: unsafe extern "C" fn(*const std::ffi::c_char) -> *const c_void,
        query_context: unsafe extern "C" fn(Display, Context, i32, *mut i32) -> u32,
        choose_config: unsafe extern "C" fn(Display, *const i32, *mut Config, i32, *mut i32) -> u32,
        create_context: unsafe extern "C" fn(Display, Config, Context, *const i32) -> Context,
        destroy_context: unsafe extern "C" fn(Display, Context) -> u32,
        make_current: unsafe extern "C" fn(Display, Surface, Surface, Context) -> u32,
        create_pbuffer: unsafe extern "C" fn(Display, Config, *const i32) -> Surface,
        destroy_surface: unsafe extern "C" fn(Display, Surface) -> u32,
        bind_api: unsafe extern "C" fn(u32) -> u32,
        get_error: unsafe extern "C" fn() -> i32,
        query_string: unsafe extern "C" fn(Display, i32) -> *const std::ffi::c_char,
        current_display: unsafe extern "C" fn() -> Display,
        current_context: unsafe extern "C" fn() -> Context,
    }

    impl Egl {
        fn load() -> Result<Self, String> {
            let lib = ["libEGL.so.1", "libEGL.so"]
                .iter()
                .find_map(|name| unsafe { libloading::Library::new(name) }.ok())
                .ok_or("no libEGL")?;
            fn sym<T: Copy>(lib: &libloading::Library, name: &str) -> Result<T, String> {
                let c = format!("{name}\0");
                unsafe { lib.get::<T>(c.as_bytes()) }
                    .map(|s| *s)
                    .map_err(|e| format!("{name}: {e}"))
            }
            macro_rules! sym {
                ($name:literal) => {
                    sym(&lib, $name)?
                };
            }
            Ok(Self {
                get_proc: sym!("eglGetProcAddress"),
                query_context: sym!("eglQueryContext"),
                choose_config: sym!("eglChooseConfig"),
                create_context: sym!("eglCreateContext"),
                destroy_context: sym!("eglDestroyContext"),
                make_current: sym!("eglMakeCurrent"),
                create_pbuffer: sym!("eglCreatePbufferSurface"),
                destroy_surface: sym!("eglDestroySurface"),
                bind_api: sym!("eglBindAPI"),
                get_error: sym!("eglGetError"),
                query_string: sym!("eglQueryString"),
                current_display: sym!("eglGetCurrentDisplay"),
                current_context: sym!("eglGetCurrentContext"),
                _lib: lib,
            })
        }

        fn error(&self, what: &str) -> String {
            format!("{what} failed (EGL error {:#x})", unsafe { (self.get_error)() })
        }
    }

    /// The calling thread's current EGL display and context, if any.
    pub(super) fn current() -> Option<(usize, usize)> {
        let egl = Egl::load().ok()?;
        let (display, context) =
            unsafe { ((egl.current_display)(), (egl.current_context)()) };
        (!display.is_null() && !context.is_null()).then_some((display as usize, context as usize))
    }

    const VERTEX: &str = "#version 300 es
out vec2 uv;
void main() {
    // A full-target strip. WebRender samples an external texture the GL way,
    // row 0 at the bottom (as a decoder writing through a framebuffer leaves
    // it), so the picture goes in bottom row first.
    vec2 p = vec2(float(gl_VertexID & 1), float(gl_VertexID >> 1));
    uv = vec2(p.x, 1.0 - p.y);
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
";

    const FRAGMENT: &str = "#version 300 es
precision highp float;
uniform sampler2D luma;
uniform sampler2D chroma;
uniform mat3 matrix;
in vec2 uv;
out vec4 color;
void main() {
    vec3 yuv = vec3(texture(luma, uv).r, texture(chroma, uv).rg) - vec3(16.0, 128.0, 128.0) / 255.0;
    color = vec4(clamp(matrix * yuv, 0.0, 1.0), 1.0);
}
";

    /// Limited-range YUV to RGB, column-major (Y, U, V columns).
    const BT709: [f32; 9] = [
        1.164_383, 1.164_383, 1.164_383, 0.0, -0.213_249, 2.112_402, 1.792_741, -0.532_909, 0.0,
    ];
    const BT601: [f32; 9] = [
        1.164_383, 1.164_383, 1.164_383, 0.0, -0.391_762, 2.017_232, 1.596_027, -0.812_968, 0.0,
    ];

    pub(super) struct Gpu {
        egl: Egl,
        display: Display,
        context: Context,
        surface: Surface,
        gl: Rc<dyn Gl>,
        program: u32,
        matrix_at: i32,
        planes: [u32; 2],
    }

    impl Gpu {
        pub(super) fn new(display: usize, share: usize) -> Result<Self, String> {
            let egl = Egl::load()?;
            let (display, share) = (display as Display, share as Context);
            unsafe {
                let mut id = 0;
                if (egl.query_context)(display, share, EGL_CONFIG_ID, &mut id) == 0 {
                    return Err(egl.error("eglQueryContext"));
                }
                let attrs = [EGL_CONFIG_ID, id, EGL_NONE];
                let (mut config, mut n) = (std::ptr::null_mut(), 0);
                if (egl.choose_config)(display, attrs.as_ptr(), &mut config, 1, &mut n) == 0 || n < 1 {
                    return Err(egl.error("eglChooseConfig"));
                }
                (egl.bind_api)(EGL_OPENGL_ES_API);
                let attrs = [EGL_CONTEXT_CLIENT_VERSION, 3, EGL_NONE];
                let context = (egl.create_context)(display, config, share, attrs.as_ptr());
                if context.is_null() {
                    return Err(egl.error("eglCreateContext"));
                }
                let exts = (egl.query_string)(display, EGL_EXTENSIONS);
                let surfaceless = !exts.is_null()
                    && CStr::from_ptr(exts)
                        .to_string_lossy()
                        .split(' ')
                        .any(|e| e == "EGL_KHR_surfaceless_context");
                let mut surface = std::ptr::null_mut();
                if !surfaceless || (egl.make_current)(display, surface, surface, context) == 0 {
                    let attrs = [EGL_WIDTH, 1, EGL_HEIGHT, 1, EGL_NONE];
                    surface = (egl.create_pbuffer)(display, config, attrs.as_ptr());
                    if surface.is_null() || (egl.make_current)(display, surface, surface, context) == 0 {
                        let e = egl.error("eglMakeCurrent");
                        (egl.destroy_context)(display, context);
                        return Err(e);
                    }
                }
                let get_proc = egl.get_proc;
                let gl = gl::GlesFns::load_with(|name| {
                    let name = CString::new(name).unwrap_or_default();
                    get_proc(name.as_ptr())
                });
                let mut gpu = Self {
                    egl,
                    display,
                    context,
                    surface,
                    gl,
                    program: 0,
                    matrix_at: -1,
                    planes: [0; 2],
                };
                gpu.program()?;
                Ok(gpu)
            }
        }

        pub(super) fn renderer(&self) -> String {
            self.gl.get_string(gl::RENDERER)
        }

        fn shader(&self, kind: u32, source: &str) -> Result<u32, String> {
            let gl = &self.gl;
            let shader = gl.create_shader(kind);
            gl.shader_source(shader, &[source.as_bytes()]);
            gl.compile_shader(shader);
            let mut ok = [0];
            unsafe { gl.get_shader_iv(shader, gl::COMPILE_STATUS, &mut ok) };
            if ok[0] == 0 {
                return Err(format!("shader: {}", gl.get_shader_info_log(shader)));
            }
            Ok(shader)
        }

        fn program(&mut self) -> Result<(), String> {
            let vs = self.shader(gl::VERTEX_SHADER, VERTEX)?;
            let fs = self.shader(gl::FRAGMENT_SHADER, FRAGMENT)?;
            let gl = &self.gl;
            let program = gl.create_program();
            gl.attach_shader(program, vs);
            gl.attach_shader(program, fs);
            gl.link_program(program);
            let mut ok = [0];
            unsafe { gl.get_program_iv(program, gl::LINK_STATUS, &mut ok) };
            if ok[0] == 0 {
                return Err(format!("program: {}", gl.get_program_info_log(program)));
            }
            gl.delete_shader(vs);
            gl.delete_shader(fs);
            gl.use_program(program);
            gl.uniform_1i(gl.get_uniform_location(program, "luma"), 0);
            gl.uniform_1i(gl.get_uniform_location(program, "chroma"), 1);
            self.matrix_at = gl.get_uniform_location(program, "matrix");
            self.program = program;
            // GLES 3 draws from the default vertex array; the strip needs no buffers.
            let vao = gl.gen_vertex_arrays(1)[0];
            gl.bind_vertex_array(vao);
            Ok(())
        }

        fn texture(&self, format: u32, w: usize, h: usize) -> u32 {
            let gl = &self.gl;
            let tex = gl.gen_textures(1)[0];
            gl.bind_texture(gl::TEXTURE_2D, tex);
            gl.tex_storage_2d(gl::TEXTURE_2D, 1, format, w as i32, h as i32);
            for (k, v) in [
                (gl::TEXTURE_MIN_FILTER, gl::LINEAR),
                (gl::TEXTURE_MAG_FILTER, gl::LINEAR),
                (gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE),
                (gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE),
            ] {
                gl.tex_parameter_i(gl::TEXTURE_2D, k, v as i32);
            }
            tex
        }

        /// The two plane textures for a new picture size.
        pub(super) fn resize(&mut self, w: usize, h: usize) -> Result<(), String> {
            if self.planes[0] != 0 {
                self.gl.delete_textures(&self.planes);
            }
            let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
            self.planes = [self.texture(gl::R8, w, h), self.texture(gl::RG8, cw, ch)];
            match self.gl.get_error() {
                gl::NO_ERROR => Ok(()),
                e => Err(format!("plane textures: GL error {e:#x}")),
            }
        }

        /// An RGBA target and its framebuffer.
        pub(super) fn target(&mut self, w: usize, h: usize) -> Result<(u32, u32), String> {
            let tex = self.texture(gl::RGBA8, w, h);
            let gl = &self.gl;
            let fbo = gl.gen_framebuffers(1)[0];
            gl.bind_framebuffer(gl::FRAMEBUFFER, fbo);
            gl.framebuffer_texture_2d(gl::FRAMEBUFFER, gl::COLOR_ATTACHMENT0, gl::TEXTURE_2D, tex, 0);
            match gl.check_frame_buffer_status(gl::FRAMEBUFFER) {
                gl::FRAMEBUFFER_COMPLETE => Ok((tex, fbo)),
                s => Err(format!("render target incomplete ({s:#x})")),
            }
        }

        pub(super) fn release(&mut self, targets: impl Iterator<Item = (u32, u32, std::sync::Arc<super::TexMark>)>) {
            for (tex, fbo, _) in targets {
                self.gl.delete_framebuffers(&[fbo]);
                self.gl.delete_textures(&[tex]);
            }
        }

        pub(super) fn draw(&mut self, planes: &Nv12Planes<'_>, w: usize, h: usize, fbo: u32) -> Result<(), String> {
            let gl = &self.gl;
            gl.pixel_store_i(gl::UNPACK_ALIGNMENT, 1);
            gl.active_texture(gl::TEXTURE0);
            gl.bind_texture(gl::TEXTURE_2D, self.planes[0]);
            gl.tex_sub_image_2d(gl::TEXTURE_2D, 0, 0, 0, w as i32, h as i32, gl::RED, gl::UNSIGNED_BYTE, planes.y);
            gl.active_texture(gl::TEXTURE1);
            gl.bind_texture(gl::TEXTURE_2D, self.planes[1]);
            let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
            gl.tex_sub_image_2d(gl::TEXTURE_2D, 0, 0, 0, cw as i32, ch as i32, gl::RG, gl::UNSIGNED_BYTE, planes.uv);
            gl.bind_framebuffer(gl::FRAMEBUFFER, fbo);
            gl.viewport(0, 0, w as i32, h as i32);
            gl.use_program(self.program);
            let matrix = if h >= 720 { &BT709 } else { &BT601 };
            gl.uniform_matrix_3fv(self.matrix_at, false, matrix);
            gl.draw_arrays(gl::TRIANGLE_STRIP, 0, 4);
            // WebRender samples the target from its own context and does not
            // wait on a fence: the picture must be complete before it is handed over.
            gl.finish();
            match gl.get_error() {
                gl::NO_ERROR => Ok(()),
                e => Err(format!("draw: GL error {e:#x}")),
            }
        }
    }

    impl Drop for Gpu {
        fn drop(&mut self) {
            unsafe {
                if self.planes[0] != 0 {
                    self.gl.delete_textures(&self.planes);
                }
                (self.egl.make_current)(self.display, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut());
                if !self.surface.is_null() {
                    (self.egl.destroy_surface)(self.display, self.surface);
                }
                (self.egl.destroy_context)(self.display, self.context);
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::super::video::Nv12Planes;

    pub(super) fn current() -> Option<(usize, usize)> {
        None
    }

    pub(super) struct Gpu;

    impl Gpu {
        pub(super) fn new(_: usize, _: usize) -> Result<Self, String> {
            Err("no GL route on this platform".into())
        }
        pub(super) fn renderer(&self) -> String {
            String::new()
        }
        pub(super) fn resize(&mut self, _: usize, _: usize) -> Result<(), String> {
            Err(String::new())
        }
        pub(super) fn target(&mut self, _: usize, _: usize) -> Result<(u32, u32), String> {
            Err(String::new())
        }
        pub(super) fn release(&mut self, _: impl Iterator<Item = (u32, u32, std::sync::Arc<super::TexMark>)>) {}
        pub(super) fn draw(&mut self, _: &Nv12Planes<'_>, _: usize, _: usize, _: u32) -> Result<(), String> {
            Err(String::new())
        }
    }
}
