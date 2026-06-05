use dpi::PhysicalSize;
use gleam::gl::{self, Gl};
use servo::{DeviceIntRect, RenderingContext, RgbaImage};
use std::cell::Cell;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;

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
    // surfman connection Servo needs for WebGL/external-image surface sharing.
    connection: Option<surfman::Connection>,
    fbo: Cell<gl::GLuint>,
    color_tex: Cell<gl::GLuint>,
    depth_rbo: Cell<gl::GLuint>,
    size: Cell<PhysicalSize<u32>>,
}

impl SdlRenderingContext {
    pub fn new(gl: Rc<dyn Gl>, glow: Arc<glow::Context>, size: PhysicalSize<u32>) -> Rc<Self> {
        let fbo = gl.gen_framebuffers(1)[0];
        let color_tex = gl.gen_textures(1)[0];
        let depth_rbo = gl.gen_renderbuffers(1)[0];
        let connection = match surfman::Connection::new() {
            Ok(c) => Some(c),
            Err(e) => {
                log::warn!("surfman Connection::new failed: {e:?} (WebGL may not work)");
                None
            }
        };
        let ctx = Rc::new(Self {
            gl,
            glow,
            connection,
            fbo: Cell::new(fbo),
            color_tex: Cell::new(color_tex),
            depth_rbo: Cell::new(depth_rbo),
            size: Cell::new(size),
        });
        ctx.allocate(size);
        ctx
    }

    /// (Re)allocate the color texture and depth renderbuffer at `size`. The GL
    /// object names are kept stable, so any egui texture registration of the
    /// color texture stays valid across resizes.
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
        gl.tex_parameter_i(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as gl::GLint);
        gl.tex_parameter_i(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as gl::GLint);
        gl.tex_parameter_i(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as gl::GLint);
        gl.tex_parameter_i(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as gl::GLint);
        gl.framebuffer_texture_2d(
            gl::FRAMEBUFFER,
            gl::COLOR_ATTACHMENT0,
            gl::TEXTURE_2D,
            self.color_tex.get(),
            0,
        );

        gl.bind_renderbuffer(gl::RENDERBUFFER, self.depth_rbo.get());
        gl.renderbuffer_storage(gl::RENDERBUFFER, gl::DEPTH_COMPONENT24, w, h);
        gl.framebuffer_renderbuffer(
            gl::FRAMEBUFFER,
            gl::DEPTH_ATTACHMENT,
            gl::RENDERBUFFER,
            self.depth_rbo.get(),
        );

        gl.bind_texture(gl::TEXTURE_2D, 0);
        gl.bind_renderbuffer(gl::RENDERBUFFER, 0);
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

        // Flip vertically: GL returns rows bottom-up.
        let stride = w as usize * 4;
        let orig = pixels.clone();
        for row in 0..h as usize {
            let dst = row * stride;
            let src = (h as usize - row - 1) * stride;
            pixels[dst..dst + stride].copy_from_slice(&orig[src..src + stride]);
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
