//! The WebGL composite path: turning a Servo front buffer into a texture SDL's
//! context can sample.
//!
//! The WebGL thread draws into surfman surfaces, which on EGL are GL textures
//! wrapped in an `EGLImageKHR`. Importing one is a plain
//! `glEGLImageTargetTexture2DOES` into whatever context is current, but surfman
//! only offers it through `create_surface_texture`, which makes *its own* context
//! current first — so the texture would land where WebRender cannot sample it.
//! Wrapping SDL's context in a surfman `Context` makes that make-current a no-op.

use euclid::default::Size2D;
use std::cell::RefCell;
use std::ffi::c_void;
use std::mem;
use surfman::{Connection, Context, Device, Surface, SurfaceTexture};

/// `eglGetCurrentSurface` selectors. retsurf links no EGL headers.
const EGL_DRAW: i32 = 0x3059;
const EGL_READ: i32 = 0x305a;

type EglGetCurrent = unsafe extern "C" fn() -> *const c_void;
type EglGetCurrentSurface = unsafe extern "C" fn(i32) -> *const c_void;

/// SDL's EGL handles. An `EGLImageKHR` belongs to a display rather than to a
/// context, so surfman has to run on the display SDL already opened.
#[derive(Clone, Copy)]
struct EglState {
    display: *const c_void,
    context: *const c_void,
    draw_surface: *const c_void,
    read_surface: *const c_void,
}

/// Servo's WebGL front buffers, sampled into SDL's GL context.
pub struct FrontBuffers {
    /// Handed to Servo, which builds the WebGL thread's own device off it.
    connection: Connection,
    device: Device,
    /// SDL's EGL context wrapped as a surfman one, so surfman's make-current
    /// asks EGL for the state it is already in.
    context: RefCell<Context>,
}

impl FrontBuffers {
    /// Must run with SDL's context current: every handle below is read off it,
    /// and surfman loads its GL entry points from it. `None` costs WebGL only.
    pub fn new(get_proc: impl Fn(&str) -> *const c_void) -> Option<Self> {
        let egl = EglState::current(get_proc)?;
        let connection = connection(&egl)?;
        let adapter = connection.create_adapter().ok()?;
        let device = match connection.create_device(&adapter) {
            Ok(device) => device,
            Err(err) => {
                log::warn!("surfman device unavailable ({err:?}); WebGL disabled");
                return None;
            }
        };
        let context = match unsafe { device.create_context_from_native_context(egl.native()) } {
            Ok(context) => context,
            Err(err) => {
                log::warn!("SDL's GL context is not wrappable ({err:?}); WebGL disabled");
                return None;
            }
        };
        log::info!("gl: WebGL front buffers share SDL's EGL display");
        Some(Self {
            connection,
            device,
            context: RefCell::new(context),
        })
    }

    pub fn connection(&self) -> Connection {
        self.connection.clone()
    }

    /// The front buffer as a texture in SDL's context, or the surface handed
    /// back: it arrives by value, and dropping one panics inside surfman.
    pub fn create_texture(
        &self,
        surface: Surface,
    ) -> Result<(SurfaceTexture, u32, Size2D<i32>), Surface> {
        let size = self.device.surface_info(&surface).size;
        let surface_texture = match self
            .device
            .create_surface_texture(&mut self.context.borrow_mut(), surface)
        {
            Ok(surface_texture) => surface_texture,
            Err((err, surface)) => {
                log::warn!("WebGL front buffer not sampleable ({err:?})");
                return Err(surface);
            }
        };
        let texture = self
            .device
            .surface_texture_object(&surface_texture)
            .map_or(0, |texture| texture.0.get());
        Ok((surface_texture, texture, size))
    }

    pub fn destroy_texture(&self, surface_texture: SurfaceTexture) -> Option<Surface> {
        self.device
            .destroy_surface_texture(&mut self.context.borrow_mut(), surface_texture)
            .map_err(|(err, _)| log::warn!("WebGL front buffer texture leaked ({err:?})"))
            .ok()
    }
}

impl Drop for FrontBuffers {
    fn drop(&mut self) {
        // surfman panics on a context it was never asked to destroy. SDL's own
        // context survives it: this one was wrapped, not created.
        if let Err(err) = self.device.destroy_context(&mut self.context.borrow_mut()) {
            log::warn!("failed to release the wrapped GL context: {err:?}");
        }
    }
}

impl EglState {
    /// `None` when SDL is not on EGL (desktop GLX). Goes through SDL's loader
    /// because it falls back to `dlsym`: EGL 1.4 promises `eglGetProcAddress`
    /// for extensions only.
    fn current(get_proc: impl Fn(&str) -> *const c_void) -> Option<Self> {
        let display: EglGetCurrent =
            unsafe { mem::transmute(non_null(get_proc("eglGetCurrentDisplay"))?) };
        let context: EglGetCurrent =
            unsafe { mem::transmute(non_null(get_proc("eglGetCurrentContext"))?) };
        let surface: EglGetCurrentSurface =
            unsafe { mem::transmute(non_null(get_proc("eglGetCurrentSurface"))?) };

        let state = unsafe {
            Self {
                display: display(),
                context: context(),
                draw_surface: surface(EGL_DRAW),
                read_surface: surface(EGL_READ),
            }
        };
        if state.display.is_null() || state.context.is_null() {
            log::info!("gl: SDL is not on EGL; WebGL disabled");
            return None;
        }
        Some(state)
    }

    /// The surfaces are read once: SDL creates its window surface with the
    /// window and never replaces it on the platforms this path compiles for.
    fn native(&self) -> surfman::NativeContext {
        use surfman::multi::context::NativeContext as MultiNativeContext;
        use surfman::wayland::context::NativeContext;

        MultiNativeContext::Default(MultiNativeContext::Default(NativeContext {
            egl_context: self.context,
            egl_draw_surface: self.draw_surface,
            egl_read_surface: self.read_surface,
        }))
    }
}

/// A surfman connection on SDL's own EGL display: `Connection::new()` opens a
/// second one, and an `EGLImageKHR` is not importable across displays. Only the
/// wayland backend takes a bare `EGLDisplay`, and makes no wayland call for it.
fn connection(egl: &EglState) -> Option<Connection> {
    use surfman::multi::connection::Connection as MultiConnection;
    use surfman::wayland::connection::{Connection as EglConnection, NativeConnection};

    match unsafe { EglConnection::from_native_connection(NativeConnection(egl.display)) } {
        Ok(connection) => Some(MultiConnection::Default(MultiConnection::Default(
            connection,
        ))),
        Err(err) => {
            log::warn!("surfman connection over SDL's EGL display failed ({err:?}); WebGL off");
            None
        }
    }
}

fn non_null(ptr: *const c_void) -> Option<*const c_void> {
    (!ptr.is_null()).then_some(ptr)
}
