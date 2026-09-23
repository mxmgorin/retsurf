use crate::config::DisplayConfig;
use egui_sdl2::{egui, EventResponse};
use sdl2::video::WindowBuilder;
use sdl2::{Sdl, VideoSubsystem};
use servo::RenderingContext;
use std::rc::Rc;
use std::time::Duration;

mod gl;
#[cfg(feature = "software")]
mod software;

use self::gl::GlBackend;
#[cfg(feature = "software")]
use self::software::SoftwareBackend;

/// Per-step cost of a software frame (`[debug] frame_timing`).
#[derive(Default, Clone, Copy)]
pub struct CompositeTiming {
    /// Page blit and clearing around it.
    pub page: Duration,
    /// egui shapes to triangles.
    pub tessellate: Duration,
    /// egui texture deltas (mostly the font atlas) to SDL textures.
    pub textures: Duration,
    /// Rasterizing the chrome's triangles.
    pub chrome: Duration,
    /// Composed surface to the presentation texture.
    pub upload: Duration,
    /// Presentation texture to the panel.
    pub present: Duration,
}

impl CompositeTiming {
    pub fn add(&mut self, other: Self) {
        self.page += other.page;
        self.tessellate += other.tessellate;
        self.textures += other.textures;
        self.chrome += other.chrome;
        self.upload += other.upload;
        self.present += other.present;
    }
}

/// Stands in where the driver reports no refresh rate: every panel here is 60 Hz.
const ASSUMED_PANEL_INTERVAL: Duration = Duration::from_micros(16_667);

/// What every renderer bundle offers the window — dispatch lives here, so the
/// rest of the app never spells a backend or a `cfg` again.
trait WindowBackend {
    fn window(&self) -> &sdl2::video::Window;
    fn rendering_ctx(&self) -> Rc<dyn RenderingContext>;
    fn egui_ctx(&self) -> &egui::Context;
    fn run_ui(&mut self, run_ui: &mut dyn FnMut(&egui::Context));
    fn repaint_delay(&self) -> Duration;
    fn on_event(&mut self, event: &sdl2::event::Event) -> EventResponse;
    #[cfg(target_os = "android")]
    fn sync_egui_window_size(&mut self);
    #[cfg(target_os = "android")]
    fn window_mut(&mut self) -> &mut sdl2::video::Window;
    fn pointer_pos_in_points(&self) -> Option<egui::Pos2>;
    fn paint(&mut self, page_at: (i32, i32), page_painted: bool) -> Option<CompositeTiming>;
    fn frame_interval(&self) -> Option<Duration>;
    fn destroy(&mut self);
    /// egui's handle to the page texture; `None` where the backend composites
    /// the page itself.
    fn browser_texture(&self) -> Option<egui::TextureId> {
        None
    }
    /// Adopt an edited frame cap; a backend the swap interval paces ignores it.
    fn set_max_fps(&mut self, _max_fps: u32) {}
    /// Full-frame buffers kept in RAM; zero where they live in the driver.
    fn compose_bytes(&self) -> usize {
        0
    }
}

/// The window, its renderer, and the egui drawing the chrome — one bundle,
/// because which renderer came up decides all three.
pub struct AppWindow {
    video_subsystem: VideoSubsystem,
    backend: Box<dyn WindowBackend>,
    /// The size the window opened at — what [`Self::remembered_size`] measures against.
    initial_size: (u32, u32),
}

impl AppWindow {
    /// Build the window with the renderer `config` asks for. `ctx_init` styles
    /// each [`egui::Context`] this window creates.
    pub fn new(
        sdl: &Sdl,
        config: &DisplayConfig,
        ctx_init: fn(&egui::Context),
    ) -> Result<Self, String> {
        let video_subsystem = sdl.video()?;
        let backend = build_backend(&video_subsystem, config, ctx_init)?;
        let mut window = Self {
            video_subsystem,
            backend,
            initial_size: (0, 0),
        };
        // Not the configured size: a driver that owns the screen opens at the panel.
        window.initial_size = window.size();
        Ok(window)
    }

    pub fn sdl2_window(&self) -> &sdl2::video::Window {
        self.backend.window()
    }

    /// The context Servo renders into: an FBO in our GL context, or swgl's CPU
    /// framebuffer.
    pub fn rendering_ctx(&self) -> Rc<dyn RenderingContext> {
        self.backend.rendering_ctx()
    }

    /// egui's handle to the page texture; `None` on the software backend, which
    /// composites the page itself.
    pub fn browser_texture(&self) -> Option<egui::TextureId> {
        self.backend.browser_texture()
    }

    pub fn egui_ctx(&self) -> &egui::Context {
        self.backend.egui_ctx()
    }

    /// Run the UI; [`Self::paint`] puts the result on screen.
    pub fn run_ui(&mut self, mut run_ui: impl FnMut(&egui::Context)) {
        self.backend.run_ui(&mut run_ui);
    }

    /// How long until egui wants another frame, from the last [`Self::run_ui`].
    pub fn repaint_delay(&self) -> Duration {
        self.backend.repaint_delay()
    }

    /// Feed an SDL event to egui.
    pub fn on_event(&mut self, event: &sdl2::event::Event) -> EventResponse {
        self.backend.on_event(event)
    }

    /// Refresh egui's cached window size from the live window, for the platforms
    /// where SDL doesn't deliver a size-changed event (see [`crate::ui::AppUi`]).
    #[cfg(target_os = "android")]
    pub fn sync_egui_window_size(&mut self) {
        self.backend.sync_egui_window_size();
    }

    /// Match the system bars to `on`, idempotently: SDL gives a fullscreen
    /// window immersive-sticky decor, hiding the status and navigation bars.
    /// The change blocks this thread until the surface resizes (up to 500 ms).
    #[cfg(target_os = "android")]
    pub fn set_system_fullscreen(&mut self, on: bool) {
        use sdl2::video::FullscreenType;
        let want = match on {
            true => FullscreenType::Desktop,
            false => FullscreenType::Off,
        };
        let window = self.backend.window_mut();
        if window.fullscreen_state() == want {
            return;
        }
        if let Err(e) = window.set_fullscreen(want) {
            log::warn!("system fullscreen {on}: {e}");
        }
    }

    pub fn pointer_pos_in_points(&self) -> Option<egui::Pos2> {
        self.backend.pointer_pos_in_points()
    }

    /// Paint the last [`Self::run_ui`] and present it. `page_at` is the web
    /// view's top-left in physical pixels; the software backend blits the page
    /// there when `page_painted`. Only the software backend returns timing.
    pub fn paint(&mut self, page_at: (i32, i32), page_painted: bool) -> Option<CompositeTiming> {
        self.backend.paint(page_at, page_painted)
    }

    /// Shortest time between two presents: the panel's period on GL, the frame
    /// cap on software (`None` there means uncapped, which is what `max_fps = 0`
    /// asks for).
    pub fn frame_interval(&self) -> Option<Duration> {
        self.backend.frame_interval()
    }

    /// Adopt an edited frame cap (settings overlay). No-op on GL — the swap
    /// interval paces it.
    pub fn set_max_fps(&mut self, max_fps: u32) {
        self.backend.set_max_fps(max_fps);
    }

    pub fn destroy(&mut self) {
        self.backend.destroy();
    }

    /// Logical window size (matches SDL mouse-event coordinates).
    pub fn size(&self) -> (u32, u32) {
        self.sdl2_window().size()
    }

    /// Physical (drawable) size in pixels — what the framebuffer and the
    /// browser's rendering context are sized in.
    pub fn drawable_size(&self) -> (u32, u32) {
        self.sdl2_window().drawable_size()
    }

    /// The panel this window is on, in device pixels, and the window's own rect
    /// within it — what the page reads as `screen` and `outerWidth`. SDL reports
    /// display bounds in logical units, so they take the window's own ratio.
    pub fn screen_geometry(&self) -> ((u32, u32), (i32, i32, u32, u32)) {
        let window = self.sdl2_window();
        let (logical_w, logical_h) = window.size();
        let (drawable_w, drawable_h) = window.drawable_size();
        let ratio = |logical: u32, drawable: u32, value: i32| match logical {
            0 => value,
            _ => (value as f64 * drawable as f64 / logical as f64).round() as i32,
        };
        let screen = window
            .display_index()
            .and_then(|index| self.video_subsystem.display_bounds(index))
            .map(|bounds| {
                (
                    ratio(logical_w, drawable_w, bounds.width() as i32).max(1) as u32,
                    ratio(logical_h, drawable_h, bounds.height() as i32).max(1) as u32,
                )
            })
            .unwrap_or((drawable_w, drawable_h));
        let (x, y) = window.position();
        (
            screen,
            (
                ratio(logical_w, drawable_w, x),
                ratio(logical_h, drawable_h, y),
                drawable_w,
                drawable_h,
            ),
        )
    }

    /// The size to reopen at: `None` unless the user resized the window — a
    /// panel-sized or maximized one is not a size to open at.
    pub fn remembered_size(&self) -> Option<(u32, u32)> {
        let window = self.sdl2_window();
        let held =
            window.is_maximized() || window.fullscreen_state() != sdl2::video::FullscreenType::Off;
        let size = window.size();
        (!held && size != self.initial_size).then_some(size)
    }

    /// Full-frame buffers kept in RAM; zero on GL, where they live in the driver.
    pub fn compose_bytes(&self) -> usize {
        self.backend.compose_bytes()
    }
}

/// Feathering tessellates extra edge triangles — cheap on a GPU, costly in
/// software, so the default follows the renderer. Glyphs antialias regardless.
/// `RETSURF_FEATHERING=0|1` overrides.
fn apply_feathering(ctx: &egui::Context, software: bool) {
    let on = crate::config::env_flag("RETSURF_FEATHERING").unwrap_or(!software);
    ctx.tessellation_options_mut(|o| o.feathering = on);
    log::info!("egui feathering: {on}");
}

/// GL unless the config asks for software; a GL failure falls back, so a device
/// with no driver at all (the Miyoo Mini) lands there without config.
#[cfg(feature = "software")]
fn build_backend(
    video: &VideoSubsystem,
    config: &DisplayConfig,
    ctx_init: fn(&egui::Context),
) -> Result<Box<dyn WindowBackend>, String> {
    if config.software_render {
        return Ok(Box::new(SoftwareBackend::new(video, config, ctx_init)?));
    }
    match GlBackend::new(video, config, ctx_init) {
        Ok(backend) => Ok(Box::new(backend)),
        Err(e) => {
            log::warn!("GL unavailable ({e}); falling back to software rendering");
            Ok(Box::new(SoftwareBackend::new(video, config, ctx_init)?))
        }
    }
}

#[cfg(not(feature = "software"))]
fn build_backend(
    video: &VideoSubsystem,
    config: &DisplayConfig,
    ctx_init: fn(&egui::Context),
) -> Result<Box<dyn WindowBackend>, String> {
    Ok(Box::new(GlBackend::new(video, config, ctx_init)?))
}

fn build_window(
    video_subsystem: &VideoSubsystem,
    config: &DisplayConfig,
    gl: bool,
) -> Result<sdl2::video::Window, String> {
    let (w, h) = panel_size(video_subsystem).unwrap_or((config.width, config.height));
    let mut builder: WindowBuilder = video_subsystem.window("retsurf", w, h);
    if gl {
        builder.opengl();
    }
    builder
        .resizable()
        .build()
        .map_err(|e| format!("failed to build window: {e}"))
}

/// The panel's own size on Miyoo drivers, where a smaller window is drawn
/// centred with a black border (the Flip is 752x560, not the configured
/// 640x480). `None` elsewhere, where the configured size is a real choice.
fn panel_size(video_subsystem: &VideoSubsystem) -> Option<(u32, u32)> {
    if !matches!(video_subsystem.current_video_driver(), "Mini" | "mmiyoo") {
        return None;
    }
    // Older builds of this driver leave the mode zeroed; treat an implausibly
    // small one as no answer.
    let mode = video_subsystem.desktop_display_mode(0).ok()?;
    let (w, h) = (u32::try_from(mode.w).ok()?, u32::try_from(mode.h).ok()?);
    (w >= 320 && h >= 240).then(|| {
        log::info!("panel: {w}x{h} from the video driver");
        (w, h)
    })
}

/// Window icon from the bundled brand PNG (RGBA8). Best-effort: decode failures
/// log and keep SDL's default.
fn set_window_icon(window: &mut sdl2::video::Window) {
    use sdl2::pixels::PixelFormatEnum;
    static ICON_PNG: &[u8] = include_bytes!("../../../resources/icon.png");

    // png 0.18's Decoder needs BufRead + Seek; a Cursor over the slice provides both.
    let mut reader = match png::Decoder::new(std::io::Cursor::new(ICON_PNG)).read_info() {
        Ok(r) => r,
        Err(e) => return log::warn!("window icon: PNG header decode failed: {e}"),
    };
    let buf_size = match reader.output_buffer_size() {
        Some(n) => n,
        None => return log::warn!("window icon: PNG output buffer size overflow"),
    };
    let mut buf = vec![0u8; buf_size];
    let info = match reader.next_frame(&mut buf) {
        Ok(i) => i,
        Err(e) => return log::warn!("window icon: PNG decode failed: {e}"),
    };
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return log::warn!(
            "window icon: unexpected PNG format {:?}/{:?}",
            info.color_type,
            info.bit_depth
        );
    }
    // png emits R,G,B,A byte order, which is ABGR8888 on our (little-endian) targets.
    let frame = &mut buf[..info.line_size * info.height as usize];
    let surface = sdl2::surface::Surface::from_data(
        frame,
        info.width,
        info.height,
        info.line_size as u32,
        PixelFormatEnum::ABGR8888,
    );
    match surface {
        // SDL_SetWindowIcon copies the pixels, so the temporary surface can drop.
        Ok(surface) => window.set_icon(surface),
        Err(e) => log::warn!("window icon: surface build failed: {e}"),
    }
}

/// Match SDL's text-input state to `active`, idempotently. On Android this
/// raises/hides the soft keyboard and gates `SDL_TEXTINPUT` delivery; the sdl2
/// crate doesn't wrap the calls. Desktop keeps SDL's default and never calls it.
#[allow(dead_code)] // only called on Android; still type-checked on desktop
pub fn set_text_input(active: bool) {
    let cur = unsafe { sdl2::sys::SDL_IsTextInputActive() } == sdl2::sys::SDL_bool::SDL_TRUE;
    if active && !cur {
        unsafe { sdl2::sys::SDL_StartTextInput() };
    } else if !active && cur {
        unsafe { sdl2::sys::SDL_StopTextInput() };
    }
}
