use super::{apply_feathering, build_window, set_window_icon, CompositeTiming, WindowBackend};
use crate::config::DisplayConfig;
use crate::platform::render::SdlRenderingContext;
use egui_sdl2::{egui, EguiGlow, EventResponse};
use gleam::gl::Gl;
use sdl2::sys;
use sdl2::video::GLContext;
use sdl2::VideoSubsystem;
use servo::RenderingContext;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

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

/// A window with its GL context current, or the SDL error that stopped it.
fn open_gl_window(
    video_subsystem: &VideoSubsystem,
    config: &DisplayConfig,
) -> Result<(sdl2::video::Window, GLContext), String> {
    let mut window = build_window(video_subsystem, config, true)?;
    set_window_icon(&mut window);

    let gl_context = window
        .gl_create_context()
        .map_err(|e| format!("failed to create GL context: {e}"))?;
    window
        .gl_make_current(&gl_context)
        .map_err(|e| format!("failed to make GL context current: {e}"))?;
    Ok((window, gl_context))
}

/// The panel's own frame period, or [`super::ASSUMED_PANEL_INTERVAL`] where the
/// driver reports no rate (Xvfb, and some fbdev firmwares).
fn panel_interval(video_subsystem: &VideoSubsystem, window: &sdl2::video::Window) -> Duration {
    window
        .display_index()
        .and_then(|index| video_subsystem.current_display_mode(index))
        .ok()
        .filter(|mode| mode.refresh_rate > 0)
        .map_or(super::ASSUMED_PANEL_INTERVAL, |mode| {
            Duration::from_secs_f64(1.0 / f64::from(mode.refresh_rate))
        })
}

/// SDL's own name for "use EGL on X11, not GLX".
const FORCE_EGL_HINT: &str = "SDL_VIDEO_X11_FORCE_EGL";

/// Ask X11 for EGL rather than GLX, which SDL would otherwise pick for an ES
/// context wherever Mesa can serve one — leaving no EGL display for WebGL's
/// front buffers. Never over a choice already made through the environment.
fn prefer_egl_on_x11(video_subsystem: &VideoSubsystem, config: &DisplayConfig) -> bool {
    if !cfg!(feature = "webgl")
        || !config.use_gles
        || video_subsystem.current_video_driver() != "x11"
        || std::env::var_os(FORCE_EGL_HINT).is_some()
    {
        return false;
    }
    sdl2::hint::set(FORCE_EGL_HINT, "1")
}

/// Everything through SDL2's single GL/GLES context: WebRender renders into an
/// FBO, egui draws its colour texture into the window. SDL2 owns the context
/// because the sdl2 crate hands surfman no window handle for a vendor backend.
pub(super) struct GlBackend {
    window: sdl2::video::Window,
    // Kept alive for the lifetime of the window; dropping it destroys the context.
    gl_context: GLContext,
    glow_ctx: Arc<glow::Context>,
    egui: EguiGlow,
    rendering_ctx: Rc<SdlRenderingContext>,
    /// egui's handle to the FBO colour texture; the GL name is stable across
    /// resizes, so this stays valid for the program's lifetime.
    browser_tex: egui::TextureId,
    /// One panel refresh. A swap that blocks holds the loop on its own, so this
    /// only binds where the driver ignores the interval it accepted.
    frame_interval: Duration,
    /// See [`DisplayConfig::dark_last_row`].
    dark_last_row: bool,
}

impl GlBackend {
    pub(super) fn new(
        video_subsystem: &VideoSubsystem,
        config: &DisplayConfig,
        ctx_init: fn(&egui::Context),
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

        let forced_egl = prefer_egl_on_x11(video_subsystem, config);
        let (window, gl_context) = match open_gl_window(video_subsystem, config) {
            Ok(pair) => pair,
            Err(e) if forced_egl => {
                log::warn!("EGL refused ({e}); retrying on the GL backend SDL picks itself");
                sdl2::hint::set(FORCE_EGL_HINT, "0");
                open_gl_window(video_subsystem, config)?
            }
            Err(e) => return Err(e),
        };

        // Caps the loop; on a panning fbdev it also lands the flip in the
        // blanking interval (muOS tears a band off the top frame without it).
        let vsync = video_subsystem
            .gl_set_swap_interval(sdl2::video::SwapInterval::VSync)
            .is_ok();
        let frame_interval = panel_interval(video_subsystem, &window);
        log::info!(
            "gl: vsync {}, panel {:.1?} a frame",
            if vsync {
                "on"
            } else {
                "REFUSED, pacing by hand"
            },
            frame_interval,
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
            SdlRenderingContext::new(gl, glow_ctx.clone(), dpi::PhysicalSize::new(w, h), get_proc);
        log::info!("window: rendering context created");

        let mut egui = EguiGlow::new(&window, glow_ctx.clone(), None, false);
        let browser_tex = egui
            .painter
            .register_native_texture(rendering_ctx.color_texture());
        ctx_init(&egui.ctx);
        apply_feathering(&egui.ctx, false);

        Ok(Self {
            window,
            gl_context,
            glow_ctx,
            egui,
            rendering_ctx,
            browser_tex,
            frame_interval,
            dark_last_row: config.dark_last_row,
        })
    }
}

impl WindowBackend for GlBackend {
    fn window(&self) -> &sdl2::video::Window {
        &self.window
    }

    fn rendering_ctx(&self) -> Rc<dyn RenderingContext> {
        self.rendering_ctx.clone()
    }

    fn egui_ctx(&self) -> &egui::Context {
        &self.egui.ctx
    }

    fn run_ui(&mut self, run_ui: &mut dyn FnMut(&egui::Context)) {
        self.egui.run(run_ui);
    }

    fn repaint_delay(&self) -> Duration {
        self.egui.repaint_delay()
    }

    fn on_event(&mut self, event: &sdl2::event::Event) -> EventResponse {
        self.egui.state.on_event(&self.window, event)
    }

    #[cfg(target_os = "android")]
    fn sync_egui_window_size(&mut self) {
        self.egui.state.sync_window_size(&self.window);
    }

    fn pointer_pos_in_points(&self) -> Option<egui::Pos2> {
        self.egui.state.get_pointer_pos_in_points()
    }

    /// The page is already in the FBO texture egui draws; the blit coordinates
    /// are the software backend's concern.
    fn paint(&mut self, _page_at: (i32, i32), _page_painted: bool) -> Option<CompositeTiming> {
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
        None
    }

    fn frame_interval(&self) -> Option<Duration> {
        Some(self.frame_interval)
    }

    fn browser_texture(&self) -> Option<egui::TextureId> {
        Some(self.browser_tex)
    }

    fn destroy(&mut self) {
        self.egui.destroy();
    }
}
