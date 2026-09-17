use super::webgl::FrontBuffers;
use super::BYTES_PER_PIXEL;
use dpi::PhysicalSize;
use euclid::default::Size2D;
use gleam::gl::{self, Gl};
use servo::{DeviceIntRect, RenderingContext, RgbaImage};
use std::cell::Cell;
use std::ffi::c_void;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::Arc;
use surfman::{Surface, SurfaceTexture};

/// A [`servo::RenderingContext`] over SDL2's single GL/GLES context plus an FBO:
/// WebRender renders into the FBO, egui draws its colour texture into the window.
pub struct SdlRenderingContext {
    gl: Rc<dyn Gl>,
    glow: Arc<glow::Context>,
    /// The WebGL composite path; `None` costs WebGL, not rendering.
    webgl: Option<FrontBuffers>,
    fbo: Cell<gl::GLuint>,
    color_tex: Cell<gl::GLuint>,
    size: Cell<PhysicalSize<u32>>,
}

impl SdlRenderingContext {
    pub fn new(
        gl: Rc<dyn Gl>,
        glow: Arc<glow::Context>,
        size: PhysicalSize<u32>,
        get_proc: impl Fn(&str) -> *const c_void,
    ) -> Rc<Self> {
        let fbo = gl.gen_framebuffers(1)[0];
        let color_tex = gl.gen_textures(1)[0];
        let ctx = Rc::new(Self {
            gl,
            glow,
            webgl: FrontBuffers::new(get_proc),
            fbo: Cell::new(fbo),
            color_tex: Cell::new(color_tex),
            size: Cell::new(size),
        });
        ctx.setup_texture_params();
        ctx.allocate(size);
        ctx
    }

    /// Set once: params survive the `tex_image_2d` reallocations in
    /// [`Self::allocate`]. NEAREST because the composite is 1:1 (see `ui/mod.rs`).
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

    /// (Re)allocate the colour texture and attach it; the GL names stay stable so
    /// egui's registration survives resizes. No depth/stencil — WebRender never
    /// enables the depth test.
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

    /// The FBO colour texture for egui/glow. Content is bottom-up (GL
    /// convention); draw with flipped UVs.
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

        // GL rows are bottom-up; swap row pairs through one scratch row.
        let stride = w as usize * BYTES_PER_PIXEL;
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
        self.webgl.as_ref().map(FrontBuffers::connection)
    }

    fn create_texture(
        &self,
        surface: Surface,
    ) -> Result<(SurfaceTexture, u32, Size2D<i32>), Surface> {
        match self.webgl.as_ref() {
            Some(webgl) => webgl.create_texture(surface),
            None => Err(surface),
        }
    }

    fn destroy_texture(&self, surface_texture: SurfaceTexture) -> Option<Surface> {
        self.webgl
            .as_ref()
            .and_then(|webgl| webgl.destroy_texture(surface_texture))
    }
}
