use crate::config::DisplayConfig;
use crate::platform::render::SdlRenderingContext;
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
    pub fn new(sdl: &Sdl, config: &DisplayConfig) -> Result<Self, String> {
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

        // Cap the main loop to the display refresh; without this the loop would
        // busy-spin while the gamepad drives continuous cursor/scroll updates.
        let _ = video_subsystem.gl_set_swap_interval(sdl2::video::SwapInterval::VSync);

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
        log::info!("window: GL context current ({w}x{h}); creating rendering context");
        let rendering_ctx =
            SdlRenderingContext::new(gl, glow_ctx.clone(), dpi::PhysicalSize::new(w, h));
        log::info!("window: rendering context created");

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

    /// Logical window size (matches SDL mouse-event coordinates).
    pub fn size(&self) -> (u32, u32) {
        self.window.size()
    }

    /// Physical (drawable) window size in pixels — what the GL framebuffer and the
    /// browser's rendering context are sized in.
    pub fn drawable_size(&self) -> (u32, u32) {
        self.window.drawable_size()
    }
}

/// Match SDL's text-input state to `active`, idempotently. On Android starting
/// text input raises the system soft keyboard and begins delivering
/// `SDL_TEXTINPUT` events (which egui-sdl2 routes to the focused field); stopping
/// it hides the keyboard. The sdl2 crate doesn't wrap these, so we call the raw
/// FFI. Desktop keeps SDL's default (text input always on) and never calls this.
#[allow(dead_code)] // only called on Android; still type-checked on desktop
pub fn set_text_input(active: bool) {
    let cur = unsafe { sdl2::sys::SDL_IsTextInputActive() } == sdl2::sys::SDL_bool::SDL_TRUE;
    if active && !cur {
        unsafe { sdl2::sys::SDL_StartTextInput() };
    } else if !active && cur {
        unsafe { sdl2::sys::SDL_StopTextInput() };
    }
}
