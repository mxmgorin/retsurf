use crate::config::InterfaceConfig;
use egui_sdl2::egui_glow;
use sdl2::video::{GLContext, GLProfile};
use sdl2::Sdl;
use std::sync::Arc;

/// The window plus the GL/GLES context that SDL2 itself owns.
///
/// On bare-kmsdrm targets (muOS/Knulli/ROCKNIX without a compositor) the `sdl2`
/// crate cannot hand surfman a usable raw-window-handle, so we let SDL2 create
/// the context (it does so via EGL/GBM, exactly like other SDL2 ports) and
/// share it with egui through `glow`. Servo renders separately into a
/// `SoftwareRenderingContext` and is composited as a texture (Path A).
pub struct AppWindow {
    _video_subsystem: sdl2::VideoSubsystem,
    window: sdl2::video::Window,
    // Kept alive for the lifetime of the window; dropping it destroys the context.
    gl_context: GLContext,
    glow_ctx: Arc<egui_glow::glow::Context>,
}

impl AppWindow {
    pub fn new(sdl: &Sdl, config: &InterfaceConfig) -> Result<Self, String> {
        let video_subsystem = sdl.video()?;

        {
            let gl_attr = video_subsystem.gl_attr();
            if config.use_gles {
                // Mali blobs on RK3326/RK3566 expose GLES 3.2; WebRender needs >= 3.0.
                gl_attr.set_context_profile(GLProfile::GLES);
                gl_attr.set_context_version(3, 0);
            } else {
                gl_attr.set_context_profile(GLProfile::Core);
                gl_attr.set_context_version(3, 2);
            }
            gl_attr.set_double_buffer(true);
        }

        let window = video_subsystem
            .window("retsurf", config.width, config.height)
            .opengl()
            .resizable()
            .build()
            .map_err(|e| format!("failed to build window: {e}"))?;

        let gl_context = window
            .gl_create_context()
            .map_err(|e| format!("failed to create GL context: {e}"))?;
        window
            .gl_make_current(&gl_context)
            .map_err(|e| format!("failed to make GL context current: {e}"))?;

        let glow_ctx = unsafe {
            egui_glow::glow::Context::from_loader_function(|name| {
                video_subsystem.gl_get_proc_address(name) as *const _
            })
        };

        Ok(Self {
            _video_subsystem: video_subsystem,
            window,
            gl_context,
            glow_ctx: Arc::new(glow_ctx),
        })
    }

    pub fn get_sdl2_window(&self) -> &sdl2::video::Window {
        &self.window
    }

    pub fn get_glow_ctx(&self) -> Arc<egui_glow::glow::Context> {
        self.glow_ctx.clone()
    }

    /// Make SDL2's GL context current on this thread. Must be called before egui
    /// paints, because Servo's software context makes *its* context current while
    /// rendering.
    pub fn make_current(&self) {
        // Servo's surfman context changed the thread's current EGL context/surface
        // directly, behind SDL's back. SDL caches which context it last made current
        // and short-circuits `gl_make_current` when it matches, so it would skip the
        // real `eglMakeCurrent` and leave us on surfman's surfaceless context (whose
        // default framebuffer is UNDEFINED). Clear SDL's cache with a NULL bind first
        // to force an actual rebind of our window surface.
        unsafe {
            sdl2::sys::SDL_GL_MakeCurrent(self.window.raw(), std::ptr::null_mut());
        }
        if let Err(err) = self.window.gl_make_current(&self.gl_context) {
            log::error!("Failed to make GL context current: {err}");
        }
    }

    /// Bind the window's default framebuffer (0) as the draw target. egui_glow
    /// renders into whatever framebuffer is currently bound and does not bind one
    /// itself, so we must point it at the window before painting.
    pub fn bind_default_framebuffer(&self) {
        use egui_glow::glow::HasContext;
        unsafe {
            self.glow_ctx
                .bind_framebuffer(egui_glow::glow::FRAMEBUFFER, None);
        }
    }

    #[inline]
    pub fn present(&self) {
        self.window.gl_swap_window();
    }

    /// Physical size of the window in pixels.
    pub fn size(&self) -> (u32, u32) {
        self.window.drawable_size()
    }
}
