use crate::config::InterfaceConfig;
use crate::render::SdlRenderingContext;
use gleam::gl::Gl;
use sdl2::video::{GLContext, GLProfile};
use sdl2::Sdl;
use servo::RenderingContext;
use std::rc::Rc;
use std::sync::Arc;

/// The window plus the single GL/GLES context that SDL2 owns.
///
/// On bare-kmsdrm targets (muOS/Knulli/ROCKNIX without a compositor) the `sdl2`
/// crate cannot hand surfman a usable raw-window-handle, so SDL2 creates the
/// context itself (via EGL/GBM, like other SDL2 ports). Both egui (`glow`) and
/// Servo (`gleam`, via [`SdlRenderingContext`]) share this one context.
pub struct AppWindow {
    _video_subsystem: sdl2::VideoSubsystem,
    window: sdl2::video::Window,
    // Kept alive for the lifetime of the window; dropping it destroys the context.
    gl_context: GLContext,
    glow_ctx: Arc<glow::Context>,
    rendering_ctx: Rc<SdlRenderingContext>,
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

        let glow_ctx = Arc::new(unsafe {
            glow::Context::from_loader_function(|name| {
                video_subsystem.gl_get_proc_address(name) as *const _
            })
        });

        // Servo/WebRender talks GL through `gleam`. Load the matching API for the
        // context profile we just created.
        let gl: Rc<dyn Gl> = unsafe {
            if config.use_gles {
                gleam::gl::GlesFns::load_with(|name| {
                    video_subsystem.gl_get_proc_address(name) as *const _
                })
            } else {
                gleam::gl::GlFns::load_with(|name| {
                    video_subsystem.gl_get_proc_address(name) as *const _
                })
            }
        };

        let (w, h) = window.drawable_size();
        let rendering_ctx =
            SdlRenderingContext::new(gl, glow_ctx.clone(), dpi::PhysicalSize::new(w, h));

        Ok(Self {
            _video_subsystem: video_subsystem,
            window,
            gl_context,
            glow_ctx,
            rendering_ctx,
        })
    }

    pub fn get_sdl2_window(&self) -> &sdl2::video::Window {
        &self.window
    }

    pub fn get_glow_ctx(&self) -> Arc<glow::Context> {
        self.glow_ctx.clone()
    }

    /// The rendering context Servo renders into (an FBO in our GL context).
    pub fn get_rendering_ctx(&self) -> Rc<dyn RenderingContext> {
        self.rendering_ctx.clone()
    }

    /// The FBO color texture, for egui to composite into the window.
    pub fn rendering_color_texture(&self) -> glow::NativeTexture {
        self.rendering_ctx.color_texture()
    }

    pub fn make_current(&self) {
        if let Err(err) = self.window.gl_make_current(&self.gl_context) {
            log::error!("Failed to make GL context current: {err}");
        }
    }

    /// Bind the window's default framebuffer (0) as the draw target. Servo's
    /// render binds our FBO, so we must point egui back at the window before it
    /// paints.
    pub fn bind_default_framebuffer(&self) {
        use glow::HasContext;
        unsafe {
            self.glow_ctx.bind_framebuffer(glow::FRAMEBUFFER, None);
        }
    }

    #[inline]
    pub fn present(&self) {
        self.window.gl_swap_window();
    }
}
