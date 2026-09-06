use super::{build_window, set_window_icon};
use crate::config::DisplayConfig;
use crate::platform::render::SdlRenderingContext;
use egui_sdl2::{egui, EguiGlow};
use gleam::gl::Gl;
use sdl2::sys;
use sdl2::video::GLContext;
use sdl2::VideoSubsystem;
use std::rc::Rc;
use std::sync::Arc;

/// One GL attribute, refusal as `Err`: sdl2's `gl_attr` setters panic instead,
/// which under `panic = "abort"` ends the run before the software fallback.
fn set_gl_attr(attr: sys::SDL_GLattr, value: i32) -> Result<(), String> {
    if unsafe { sys::SDL_GL_SetAttribute(attr, value) } != 0 {
        return Err(format!(
            "driver refused GL attribute {}: {}",
            attr as i32,
            sdl2::get_error()
        ));
    }
    Ok(())
}

/// Everything through SDL2's single GL/GLES context: WebRender renders into an
/// FBO, egui draws its colour texture into the window. SDL2 owns the context
/// because on bare kmsdrm it cannot hand surfman a usable window handle.
pub(super) struct GlBackend {
    pub(super) window: sdl2::video::Window,
    // Kept alive for the lifetime of the window; dropping it destroys the context.
    gl_context: GLContext,
    glow_ctx: Arc<glow::Context>,
    pub(super) egui: EguiGlow,
    pub(super) rendering_ctx: Rc<SdlRenderingContext>,
    /// egui's handle to the FBO colour texture; the GL name is stable across
    /// resizes, so this stays valid for the program's lifetime.
    pub(super) browser_tex: egui::TextureId,
    /// Whether the swap actually blocks: fbdev + Mali on muOS refuses the
    /// interval, and the loop leans on it for pacing.
    pub(super) vsync: bool,
    /// See [`DisplayConfig::dark_last_row`].
    dark_last_row: bool,
}

impl GlBackend {
    pub(super) fn new(
        video_subsystem: &VideoSubsystem,
        config: &DisplayConfig,
    ) -> Result<Self, String> {
        // Mali blobs on RK3326/RK3566 expose GLES 3.2; WebRender needs >= 3.0.
        let (profile, minor) = if config.use_gles {
            (sys::SDL_GLprofile::SDL_GL_CONTEXT_PROFILE_ES, 0)
        } else {
            (sys::SDL_GLprofile::SDL_GL_CONTEXT_PROFILE_CORE, 2)
        };
        set_gl_attr(sys::SDL_GLattr::SDL_GL_CONTEXT_PROFILE_MASK, profile as i32)?;
        set_gl_attr(sys::SDL_GLattr::SDL_GL_CONTEXT_MAJOR_VERSION, 3)?;
        set_gl_attr(sys::SDL_GLattr::SDL_GL_CONTEXT_MINOR_VERSION, minor)?;
        set_gl_attr(sys::SDL_GLattr::SDL_GL_DOUBLEBUFFER, 1)?;

        let mut window = build_window(video_subsystem, config, true)?;
        set_window_icon(&mut window);

        let gl_context = window
            .gl_create_context()
            .map_err(|e| format!("failed to create GL context: {e}"))?;
        window
            .gl_make_current(&gl_context)
            .map_err(|e| format!("failed to make GL context current: {e}"))?;

        // Caps the loop; on a panning fbdev it also lands the flip in the
        // blanking interval (muOS tears a band off the top frame without it).
        let vsync = video_subsystem
            .gl_set_swap_interval(sdl2::video::SwapInterval::VSync)
            .is_ok();
        log::info!(
            "gl: vsync {}",
            if vsync {
                "on"
            } else {
                "REFUSED, pacing by hand"
            }
        );

        // One loader closure feeds glow (egui) and gleam (Servo/WebRender).
        let get_proc =
            |name: &str| video_subsystem.gl_get_proc_address(name) as *const std::os::raw::c_void;

        let glow_ctx = Arc::new(unsafe { glow::Context::from_loader_function(get_proc) });

        // Load the gleam API matching the context profile just created.
        let gl: Rc<dyn Gl> = unsafe {
            if config.use_gles {
                gleam::gl::GlesFns::load_with(get_proc)
            } else {
                gleam::gl::GlFns::load_with(get_proc)
            }
        };

        let (w, h) = window.drawable_size();
        log::info!("window: GL context current ({w}x{h}); creating rendering context");
        let rendering_ctx =
            SdlRenderingContext::new(gl, glow_ctx.clone(), dpi::PhysicalSize::new(w, h));
        log::info!("window: rendering context created");

        let mut egui = EguiGlow::new(&window, glow_ctx.clone(), None, false);
        let browser_tex = egui
            .painter
            .register_native_texture(rendering_ctx.color_texture());

        Ok(Self {
            window,
            gl_context,
            glow_ctx,
            egui,
            rendering_ctx,
            browser_tex,
            vsync,
            dark_last_row: config.dark_last_row,
        })
    }

    pub(super) fn paint(&mut self) {
        // Servo left its context current and our FBO bound; restore both before
        // egui issues any GL call.
        if let Err(err) = self.window.gl_make_current(&self.gl_context) {
            log::error!("failed to make GL context current: {err}");
        }
        use glow::HasContext;
        unsafe { self.glow_ctx.bind_framebuffer(glow::FRAMEBUFFER, None) };
        self.egui.paint();
        if self.dark_last_row {
            // After egui, before the swap: nothing may paint over it.
            // GL's origin is bottom-left, so y=0 is the row scanned out last.
            let (w, _) = self.window.drawable_size();
            unsafe {
                self.glow_ctx.enable(glow::SCISSOR_TEST);
                self.glow_ctx.scissor(0, 0, w as i32, 1);
                self.glow_ctx.clear_color(0.0, 0.0, 0.0, 1.0);
                self.glow_ctx.clear(glow::COLOR_BUFFER_BIT);
                self.glow_ctx.disable(glow::SCISSOR_TEST);
            }
        }
        self.window.gl_swap_window();
    }
}
