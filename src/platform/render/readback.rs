//! WebGL under the software renderer: Servo's front buffers copied into swgl
//! textures, since swgl cannot sample a GPU one.
//!
//! The WebGL thread renders on Mesa's surfaceless EGL platform, which needs no
//! window. A second context on that connection, used on the compositing thread,
//! reads each locked front buffer and lends the bytes to a swgl texture until
//! WebRender unlocks it. `SetTextureBuffer` attaches them by name without
//! binding, so WebRender's cached GL state stays valid.
//!
//! The compositing thread must hold no current context of another GL API: glvnd
//! refuses an EGL make-current next to a GLX one.

use super::BYTES_PER_PIXEL;
use euclid::default::Size2D;
use gleam::gl::Gl;
use glow::HasContext;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CStr;
use surfman::{Connection, Context, ContextAttributeFlags, ContextAttributes, Device, GLVersion};
use surfman::{Surface, SurfaceTexture};

/// Any version reads a texture through an FBO.
const READBACK_GL_VERSION: GLVersion = GLVersion { major: 2, minor: 1 };

/// A swgl texture lent the pixels of one locked front buffer.
struct Lent {
    texture: u32,
    /// Must outlive the texture: swgl reads it in place.
    pixels: Vec<u8>,
}

/// Front buffers copied from a surfaceless GL context into swgl textures.
pub struct Readback {
    connection: Connection,
    device: Device,
    context: RefCell<Context>,
    gl: glow::Context,
    framebuffer: glow::Framebuffer,
    /// Keyed by the source texture name.
    lent: RefCell<HashMap<u32, Lent>>,
}

impl Readback {
    /// `None` costs WebGL only: the page renders as before without it.
    pub fn new() -> Option<Self> {
        let connection = surfaceless_connection()?;
        let adapter = connection
            .create_adapter()
            .map_err(|err| log::warn!("readback: no surfman adapter ({err:?}); WebGL off"))
            .ok()?;
        let device = connection
            .create_device(&adapter)
            .map_err(|err| log::warn!("readback: no surfman device ({err:?}); WebGL off"))
            .ok()?;
        let mut context = match create_context(&device) {
            Ok(context) => context,
            Err(err) => {
                log::warn!("readback: no GL context ({err:?}); WebGL off");
                return None;
            }
        };
        if let Err(err) = device.make_context_current(&context) {
            log::warn!("readback: context not current ({err:?}); WebGL off");
            let _ = device.destroy_context(&mut context);
            return None;
        }
        let gl = unsafe {
            glow::Context::from_loader_function(|name| device.get_proc_address(&context, name))
        };
        let framebuffer = match unsafe { gl.create_framebuffer() } {
            Ok(framebuffer) => framebuffer,
            Err(err) => {
                log::warn!("readback: no framebuffer ({err}); WebGL off");
                let _ = device.destroy_context(&mut context);
                return None;
            }
        };
        log::info!(
            "gl: WebGL front buffers read back from {} for swgl",
            unsafe { gl.get_parameter_string(glow::RENDERER) }
        );
        Some(Self {
            connection,
            device,
            context: RefCell::new(context),
            gl,
            framebuffer,
            lent: RefCell::default(),
        })
    }

    pub fn connection(&self) -> Connection {
        self.connection.clone()
    }

    /// The front buffer's pixels as a swgl texture, or the surface handed back:
    /// it arrives by value, and dropping one panics inside surfman.
    pub fn create_texture(
        &self,
        swgl: &swgl::Context,
        surface: Surface,
    ) -> Result<(SurfaceTexture, u32, Size2D<i32>), Surface> {
        let mut context = self.context.borrow_mut();
        if let Err(err) = self.device.make_context_current(&context) {
            log::warn!("readback: context not current ({err:?})");
            return Err(surface);
        }
        let size = self.device.surface_info(&surface).size;
        let surface_texture = match self.device.create_surface_texture(&mut context, surface) {
            Ok(surface_texture) => surface_texture,
            Err((err, surface)) => {
                log::warn!("readback: front buffer not sampleable ({err:?})");
                return Err(surface);
            }
        };
        // Texture 0 samples as nothing; the unlock still returns the surface.
        let Some(source) = self.device.surface_texture_object(&surface_texture) else {
            log::warn!("readback: front buffer has no texture");
            return Ok((surface_texture, 0, size));
        };

        let mut pixels = self.read(source, size);
        // swgl stores RGBA8 as BGRA.
        for pixel in pixels.as_chunks_mut::<BYTES_PER_PIXEL>().0 {
            pixel.swap(0, 2);
        }
        let texture = swgl.gen_textures(1)[0];
        let mut lent = self.lent.borrow_mut();
        let lent = lent
            .entry(source.0.get())
            .insert_entry(Lent { texture, pixels })
            .into_mut();
        swgl.set_texture_buffer(
            texture,
            gleam::gl::RGBA8,
            size.width,
            size.height,
            size.width * BYTES_PER_PIXEL as i32,
            lent.pixels.as_mut_ptr().cast(),
            size.width,
            size.height,
        );
        Ok((surface_texture, texture, size))
    }

    pub fn destroy_texture(
        &self,
        swgl: &swgl::Context,
        surface_texture: SurfaceTexture,
    ) -> Option<Surface> {
        if let Some(source) = self.device.surface_texture_object(&surface_texture) {
            if let Some(lent) = self.lent.borrow_mut().remove(&source.0.get()) {
                swgl.delete_textures(&[lent.texture]);
            }
        }
        let mut context = self.context.borrow_mut();
        if let Err(err) = self.device.make_context_current(&context) {
            log::warn!("readback: context not current ({err:?})");
        }
        self.device
            .destroy_surface_texture(&mut context, surface_texture)
            .map_err(|(err, _)| log::warn!("readback: surface texture leaked ({err:?})"))
            .ok()
    }

    /// `source`'s pixels, RGBA and bottom-up as GL stores them.
    fn read(&self, source: glow::Texture, size: Size2D<i32>) -> Vec<u8> {
        let (width, height) = (size.width.max(0), size.height.max(0));
        let mut pixels = vec![0; width as usize * height as usize * BYTES_PER_PIXEL];
        let gl = &self.gl;
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(self.framebuffer));
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                self.device.surface_gl_texture_target(),
                Some(source),
                0,
            );
            gl.pixel_store_i32(glow::PACK_ALIGNMENT, 1);
            gl.read_pixels(
                0,
                0,
                width,
                height,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                None,
                0,
            );
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        }
        pixels
    }
}

impl Drop for Readback {
    fn drop(&mut self) {
        // surfman panics on a context it was never asked to destroy.
        let mut context = self.context.borrow_mut();
        if self.device.make_context_current(&context).is_ok() {
            unsafe { self.gl.delete_framebuffer(self.framebuffer) };
        }
        if let Err(err) = self.device.destroy_context(&mut context) {
            log::warn!("readback: GL context leaked ({err:?})");
        }
    }
}

/// The sonames surfman tries for libEGL, in its order.
const EGL_SONAMES: [&CStr; 2] = [c"libEGL.so.1", c"libEGL.so"];

/// Whether the libEGL surfman would open exports `eglGetPlatformDisplay`; returns
/// false when there is none. surfman panics on a missing entry point, and an EGL
/// 1.4 library lacks this one.
fn egl_has_platform_display() -> bool {
    let Some(handle) = EGL_SONAMES
        .iter()
        .map(|soname| unsafe { libc::dlopen(soname.as_ptr(), libc::RTLD_LAZY) })
        .find(|handle| !handle.is_null())
    else {
        return false;
    };
    let symbol = unsafe { libc::dlsym(handle, c"eglGetPlatformDisplay".as_ptr()) };
    unsafe { libc::dlclose(handle) };
    !symbol.is_null()
}

/// The alternate arm of surfman's unix chain, so it is Servo's `Connection` type.
fn surfaceless_connection() -> Option<Connection> {
    use surfman::mesa_surfaceless::connection::Connection as SurfacelessConnection;
    use surfman::multi::connection::Connection as MultiConnection;

    if !egl_has_platform_display() {
        log::warn!("readback: no EGL 1.5 library; WebGL off");
        return None;
    }
    SurfacelessConnection::new()
        .map(MultiConnection::Alternate)
        .map_err(|err| log::warn!("readback: no surfaceless EGL display ({err:?}); WebGL off"))
        .ok()
}

fn create_context(device: &Device) -> Result<Context, surfman::Error> {
    let attributes = ContextAttributes {
        version: READBACK_GL_VERSION,
        flags: ContextAttributeFlags::empty(),
    };
    let descriptor = device.create_context_descriptor(&attributes)?;
    device.create_context(&descriptor, None)
}
