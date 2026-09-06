use super::BYTES_PER_PIXEL;
use dpi::PhysicalSize;
use gleam::gl::{self, Gl};
use servo::{DeviceIntRect, RenderingContext, RgbaImage};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

/// The page as swgl last rasterized it: BGRA8, bottom-up, `stride` bytes per row.
pub struct Frame<'a> {
    pub pixels: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub stride: usize,
}

impl Frame<'_> {
    /// The rows top-down — the order presentation wants them in.
    pub fn rows_top_down(&self) -> impl Iterator<Item = &[u8]> {
        let row = self.width as usize * BYTES_PER_PIXEL;
        (0..self.height as usize)
            .rev()
            .map(move |y| &self.pixels[y * self.stride..y * self.stride + row])
    }
}

/// A [`servo::RenderingContext`] over swgl, WebRender's software rasterizer.
/// WebRender keys its software paths off the renderer name, so handing Servo
/// this context is the entire switch.
pub struct SwglRenderingContext {
    swgl: swgl::Context,
    // The same context as `swgl`, as the trait object WebRender draws through.
    gl: Rc<dyn Gl>,
    size: Cell<PhysicalSize<u32>>,
}

impl SwglRenderingContext {
    pub fn new(size: PhysicalSize<u32>) -> Rc<Self> {
        let swgl = swgl::Context::create();
        // swgl keeps one current context per process in a global; must precede
        // any call through `gl`, including WebRender's setup.
        swgl.make_current();
        let ctx = Rc::new(Self {
            swgl,
            gl: Rc::new(swgl),
            size: Cell::new(size),
        });
        ctx.allocate(size);
        ctx
    }

    /// Size the default framebuffer: a null buffer means swgl allocates, a zero
    /// stride means it derives one from the width.
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
            // Valid until the next `allocate`, which the `&self` borrow held
            // through the slice rules out.
            pixels: unsafe { std::slice::from_raw_parts(data as *const u8, len) },
            width: width as u32,
            height: height as u32,
            stride: stride as usize,
        })
    }
}

// No `Drop`: WebRender's `Rc<dyn Gl>` aliases the context and nothing orders the
// two; the app exits without unwinding (see `App::run`), so the OS reclaims it.

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
        for pixel in pixels.as_chunks_mut::<BYTES_PER_PIXEL>().0 {
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
        unreachable!("glow is only reached through Servo's offscreen GL wrapper, never built on the swgl path")
    }
}
