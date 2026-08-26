use dpi::PhysicalSize;
use gleam::gl::{self, Gl};
use servo::{DeviceIntRect, RenderingContext, RgbaImage};
use std::cell::Cell;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;

/// Every buffer here is 32-bit colour: RGBA or BGRA, four bytes either way.
#[cfg(feature = "software")]
pub const BYTES_PER_PIXEL: usize = 4;

/// Create a surfman connection, or `None` if surfman can't initialize here.
///
/// Servo uses it only for WebGL/WebGPU external images. surfman 0.12 requires
/// `eglGetPlatformDisplay` (EGL 1.5) and *panics* instead of returning `Err` when
/// it's missing, as on EGL 1.4 Mali blobs (Knulli/muOS/ROCKNIX), so the call is
/// guarded with `catch_unwind` — `None` there costs only WebGL, not rendering. Must
/// run before other threads start: it briefly swaps the global panic hook (logging,
/// not silencing, so an unexpected panic is still recorded).
///
/// The whole probe is behind the `webgl` feature, off in the handheld build.
fn create_surfman_connection() -> Option<surfman::Connection> {
    #[cfg(not(feature = "webgl"))]
    {
        log::info!("WebGL disabled at build time (`webgl` feature off); skipping surfman");
        None
    }

    #[cfg(feature = "webgl")]
    {
        log::debug!("probing surfman connection (WebGL)");
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|info| {
            // Log rather than swallow: the expected EGL 1.4 panic is just noise at
            // debug level, but if anything else panics in this window we still see it.
            log::debug!("surfman connection probe panicked: {info}");
        }));
        let result = std::panic::catch_unwind(surfman::Connection::new);
        std::panic::set_hook(prev_hook);
        match result {
            Ok(Ok(connection)) => Some(connection),
            Ok(Err(e)) => {
                log::warn!("surfman connection unavailable ({e:?}); WebGL disabled");
                None
            }
            Err(_) => {
                log::warn!("surfman connection unsupported on this device; WebGL disabled");
                None
            }
        }
    }
}

/// The page as swgl last rasterized it: BGRA8 bytes, bottom-up (WebRender draws
/// with the GL origin at the bottom), `stride` bytes per row.
#[cfg(feature = "software")]
pub struct Frame<'a> {
    pub pixels: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub stride: usize,
}

#[cfg(feature = "software")]
impl Frame<'_> {
    /// The rows top-down — the order every presentation path wants them in.
    pub fn rows_top_down(&self) -> impl Iterator<Item = &[u8]> {
        let row = self.width as usize * BYTES_PER_PIXEL;
        (0..self.height as usize)
            .rev()
            .map(move |y| &self.pixels[y * self.stride..y * self.stride + row])
    }
}

/// A [`servo::RenderingContext`] backed by swgl, WebRender's own software
/// rasterizer — the whole page path with no GL driver under it, for devices with
/// no GPU at all.
///
/// WebRender picks its software paths off the renderer name (`Software
/// WebRender`) rather than a build flag, so there is nothing else to configure:
/// handing Servo this context is the entire switch. The framebuffer is swgl's
/// own allocation; [`SwglRenderingContext::frame`] lends it out for compositing.
#[cfg(feature = "software")]
pub struct SwglRenderingContext {
    swgl: swgl::Context,
    // The same context as `swgl`, as the trait object WebRender draws through.
    gl: Rc<dyn Gl>,
    size: Cell<PhysicalSize<u32>>,
}

#[cfg(feature = "software")]
impl SwglRenderingContext {
    pub fn new(size: PhysicalSize<u32>) -> Rc<Self> {
        let swgl = swgl::Context::create();
        // swgl keeps one current context per process in a global, so this must
        // happen before any call through `gl` — including WebRender's setup.
        swgl.make_current();
        let ctx = Rc::new(Self {
            swgl,
            gl: Rc::new(swgl),
            size: Cell::new(size),
        });
        ctx.allocate(size);
        ctx
    }

    /// Size the default framebuffer, letting swgl own the storage: a null buffer
    /// means it allocates, and a zero stride means it derives one from the width.
    fn allocate(&self, size: PhysicalSize<u32>) {
        let w = size.width.max(1) as i32;
        let h = size.height.max(1) as i32;
        self.swgl
            .init_default_framebuffer(0, 0, w, h, 0, std::ptr::null_mut());
    }

    /// The last rendered frame, or `None` before anything has been drawn.
    pub fn frame(&self) -> Option<Frame<'_>> {
        let (data, width, height, stride) = self.swgl.get_color_buffer(0, true);
        if data.is_null() || width <= 0 || height <= 0 {
            return None;
        }
        let len = stride as usize * height as usize;
        Some(Frame {
            // Valid until the next `allocate`, which takes `&self` — so the
            // borrow this ties the slice to is the one that rules that out.
            pixels: unsafe { std::slice::from_raw_parts(data as *const u8, len) },
            width: width as u32,
            height: height as u32,
            stride: stride as usize,
        })
    }
}

// No `Drop`: the `Rc<dyn Gl>` handed to WebRender points at the same context, and
// nothing orders the two. There is one context per process and the app exits
// without unwinding (see `App::run`), so the OS reclaims it.

#[cfg(feature = "software")]
impl RenderingContext for SwglRenderingContext {
    fn prepare_for_rendering(&self) {
        self.gl.bind_framebuffer(gl::FRAMEBUFFER, 0);
    }

    fn read_to_image(&self, source_rectangle: DeviceIntRect) -> Option<RgbaImage> {
        let w = source_rectangle.width();
        let h = source_rectangle.height();
        if w <= 0 || h <= 0 {
            return None;
        }
        let frame = self.frame()?;
        let (x, y) = (source_rectangle.min.x.max(0), source_rectangle.min.y.max(0));
        let (w, h) = (
            w.min(frame.width as i32 - x).max(0) as usize,
            h.min(frame.height as i32 - y).max(0) as usize,
        );
        if w == 0 || h == 0 {
            return None;
        }

        // Bottom-up BGRA in, top-down RGBA out.
        let start = x as usize * BYTES_PER_PIXEL;
        let mut pixels = Vec::with_capacity(w * h * BYTES_PER_PIXEL);
        for row in (0..h).rev() {
            let at = (y as usize + row) * frame.stride + start;
            pixels.extend_from_slice(&frame.pixels[at..at + w * BYTES_PER_PIXEL]);
        }
        // swgl stores its `GL_RGBA8` framebuffer as BGRA bytes.
        for pixel in pixels.chunks_exact_mut(BYTES_PER_PIXEL) {
            pixel.swap(0, 2);
        }

        RgbaImage::from_raw(w as u32, h as u32, pixels)
    }

    fn size(&self) -> PhysicalSize<u32> {
        self.size.get()
    }

    fn resize(&self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 || size == self.size.get() {
            return;
        }
        self.size.set(size);
        self.allocate(size);
    }

    fn present(&self) {
        // No swap: the frame is a CPU buffer the window composites from.
    }

    fn make_current(&self) -> Result<(), surfman::Error> {
        self.swgl.make_current();
        Ok(())
    }

    fn gleam_gl_api(&self) -> Rc<dyn Gl> {
        self.gl.clone()
    }

    fn glow_gl_api(&self) -> Arc<glow::Context> {
        unreachable!("glow is only reached through Servo's offscreen wrapper context, which needs a GL driver and is never built on this path")
    }
}

/// A [`servo::RenderingContext`] backed by SDL2's single GL/GLES context plus a
/// self-managed framebuffer object.
///
/// Servo (WebRender) renders into the FBO; egui then draws the FBO's color
/// texture into the window. Because there is only one GL context (SDL2's) and no
/// CPU readback, this runs on the device's native Mali GLES driver — no surfman
/// software adapter / llvmpipe, and none of the dual-context pitfalls of Path A.
///
/// WebRender renders into whichever framebuffer is bound after
/// `prepare_for_rendering`, so we simply bind our FBO there.
pub struct SdlRenderingContext {
    gl: Rc<dyn Gl>,
    glow: Arc<glow::Context>,
    // surfman connection Servo uses for WebGL/WebGPU external images. `None` on
    // devices where surfman can't initialize (e.g. EGL 1.4 Mali blobs); WebGL is
    // then disabled but normal rendering is unaffected.
    connection: Option<surfman::Connection>,
    fbo: Cell<gl::GLuint>,
    color_tex: Cell<gl::GLuint>,
    size: Cell<PhysicalSize<u32>>,
}

impl SdlRenderingContext {
    pub fn new(gl: Rc<dyn Gl>, glow: Arc<glow::Context>, size: PhysicalSize<u32>) -> Rc<Self> {
        let fbo = gl.gen_framebuffers(1)[0];
        let color_tex = gl.gen_textures(1)[0];
        let connection = create_surfman_connection();
        let ctx = Rc::new(Self {
            gl,
            glow,
            connection,
            fbo: Cell::new(fbo),
            color_tex: Cell::new(color_tex),
            size: Cell::new(size),
        });
        ctx.setup_texture_params();
        ctx.allocate(size);
        ctx
    }

    /// Set the color texture's sampling/wrap params once, at creation. They
    /// persist across the `tex_image_2d` reallocations in [`Self::allocate`], so
    /// there's no need to re-set them on every resize. NEAREST is correct because
    /// the composite is 1:1 (no scaling, see `ui/mod.rs`), so LINEAR would only
    /// burn texture bandwidth without changing the output.
    fn setup_texture_params(&self) {
        let gl = &self.gl;
        gl.bind_texture(gl::TEXTURE_2D, self.color_tex.get());
        let set = |pname: gl::GLenum, val: gl::GLenum| {
            gl.tex_parameter_i(gl::TEXTURE_2D, pname, val as gl::GLint);
        };
        set(gl::TEXTURE_MAG_FILTER, gl::NEAREST);
        set(gl::TEXTURE_MIN_FILTER, gl::NEAREST);
        set(gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE);
        set(gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE);
        gl.bind_texture(gl::TEXTURE_2D, 0);
    }

    /// (Re)allocate the color texture at `size` and attach it to the FBO. The GL
    /// object names are kept stable, so any egui texture registration of the
    /// color texture stays valid across resizes. No depth/stencil attachment:
    /// WebRender draws flat 2D tiles with the depth test never enabled, so
    /// `COLOR_ATTACHMENT0` alone makes the FBO complete.
    fn allocate(&self, size: PhysicalSize<u32>) {
        let w = size.width.max(1) as gl::GLsizei;
        let h = size.height.max(1) as gl::GLsizei;
        let gl = &self.gl;

        gl.bind_framebuffer(gl::FRAMEBUFFER, self.fbo.get());

        gl.bind_texture(gl::TEXTURE_2D, self.color_tex.get());
        gl.tex_image_2d(
            gl::TEXTURE_2D,
            0,
            gl::RGBA as gl::GLint,
            w,
            h,
            0,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
            None,
        );
        gl.framebuffer_texture_2d(
            gl::FRAMEBUFFER,
            gl::COLOR_ATTACHMENT0,
            gl::TEXTURE_2D,
            self.color_tex.get(),
            0,
        );

        gl.bind_texture(gl::TEXTURE_2D, 0);
        gl.bind_framebuffer(gl::FRAMEBUFFER, 0);
    }

    /// The FBO color texture, as an egui-/glow-facing handle. The browser frame
    /// is rendered bottom-up (GL convention), so draw it with a vertically
    /// flipped UV rect.
    pub fn color_texture(&self) -> glow::NativeTexture {
        glow::NativeTexture(NonZeroU32::new(self.color_tex.get()).expect("color texture id is 0"))
    }
}

impl RenderingContext for SdlRenderingContext {
    fn prepare_for_rendering(&self) {
        self.gl.bind_framebuffer(gl::FRAMEBUFFER, self.fbo.get());
    }

    fn read_to_image(&self, source_rectangle: DeviceIntRect) -> Option<RgbaImage> {
        let w = source_rectangle.width();
        let h = source_rectangle.height();
        if w <= 0 || h <= 0 {
            return None;
        }

        let gl = &self.gl;
        gl.bind_framebuffer(gl::FRAMEBUFFER, self.fbo.get());
        let mut pixels = gl.read_pixels(
            source_rectangle.min.x,
            source_rectangle.min.y,
            w,
            h,
            gl::RGBA,
            gl::UNSIGNED_BYTE,
        );

        // Flip vertically: GL returns rows bottom-up. Swap row pairs through a
        // single stride-sized scratch buffer instead of cloning the whole frame
        // (an odd middle row is already in place, so the half loop skips it).
        let stride = w as usize * 4;
        let h = h as usize;
        let mut tmp = vec![0u8; stride];
        for row in 0..h / 2 {
            let top = row * stride;
            let bot = (h - row - 1) * stride;
            tmp.copy_from_slice(&pixels[top..top + stride]);
            pixels.copy_within(bot..bot + stride, top);
            pixels[bot..bot + stride].copy_from_slice(&tmp);
        }

        RgbaImage::from_raw(w as u32, h as u32, pixels)
    }

    fn size(&self) -> PhysicalSize<u32> {
        self.size.get()
    }

    fn resize(&self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 || size == self.size.get() {
            return;
        }
        self.size.set(size);
        self.allocate(size);
    }

    fn present(&self) {
        // No swap: egui composites the color texture into the window framebuffer.
    }

    fn make_current(&self) -> Result<(), surfman::Error> {
        // Single shared SDL2 GL context; already current on this thread.
        Ok(())
    }

    fn gleam_gl_api(&self) -> Rc<dyn Gl> {
        self.gl.clone()
    }

    fn glow_gl_api(&self) -> Arc<glow::Context> {
        self.glow.clone()
    }

    fn connection(&self) -> Option<surfman::Connection> {
        self.connection.clone()
    }
}
