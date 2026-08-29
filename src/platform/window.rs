use crate::config::DisplayConfig;
use crate::platform::render::SdlRenderingContext;
use egui_sdl2::{egui, EguiGlow, EventResponse};
use gleam::gl::Gl;
use sdl2::sys;
use sdl2::video::{GLContext, WindowBuilder};
use sdl2::{Sdl, VideoSubsystem};
use servo::RenderingContext;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

#[cfg(feature = "software")]
use crate::platform::render::{SwglRenderingContext, BYTES_PER_PIXEL};
#[cfg(feature = "software")]
use egui_sdl2::EguiCanvas;
#[cfg(feature = "software")]
use sdl2::pixels::{Color, PixelFormatEnum};
#[cfg(feature = "software")]
use sdl2::rect::Rect;
#[cfg(feature = "software")]
use sdl2::render::{BlendMode, Canvas, Texture, WindowCanvas};
#[cfg(feature = "software")]
use sdl2::surface::{Surface, SurfaceContext};
// Only the software backend times its own steps; GL leaves that to the driver.
#[cfg(feature = "software")]
use std::time::Instant;

/// swgl writes its framebuffer as BGRA bytes, which is what SDL calls
/// `ARGB8888`. Composing in that format keeps the page a straight row copy.
#[cfg(feature = "software")]
const COMPOSE_FORMAT: PixelFormatEnum = PixelFormatEnum::ARGB8888;

/// Behind the page and the chrome, on the frames neither covers.
#[cfg(feature = "software")]
const CLEAR_COLOR: Color = Color::BLACK;

/// Where a software frame's time went, step by step over the same 1.7 MB
/// surface. Which step dominates decides what is worth optimising next — so the
/// split is measured rather than guessed (`[debug] frame_timing`).
#[derive(Default, Clone, Copy)]
pub struct CompositeTiming {
    /// Copying the page under the chrome, and clearing what it does not cover.
    pub page: Duration,
    /// egui's shapes into triangles.
    pub tessellate: Duration,
    /// egui's texture deltas (the font atlas, mostly) into SDL textures.
    pub textures: Duration,
    /// Those triangles, rasterized by SDL's software renderer.
    pub chrome: Duration,
    /// The composed surface into the presentation texture.
    pub upload: Duration,
    /// That texture onto the panel.
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

/// 30 fps, the software path's ceiling. Nothing here blocks the way a GL swap
/// does, so without a cap a scrolling frame would run the CPU flat out.
#[cfg(feature = "software")]
const SOFTWARE_FRAME_INTERVAL: Duration = Duration::from_millis(33);

/// The window, the renderer that puts pixels in it, and the egui that draws the
/// chrome — one bundle, because which renderer came up decides all three.
///
/// On bare-kmsdrm targets (muOS/Knulli/ROCKNIX without a compositor) the `sdl2`
/// crate cannot hand surfman a usable raw-window-handle, so SDL2 creates the GL
/// context itself (via EGL/GBM, like other SDL2 ports). Both egui (`glow`) and
/// Servo (`gleam`, via [`SdlRenderingContext`]) then share that one context.
pub struct AppWindow {
    _video_subsystem: VideoSubsystem,
    backend: Backend,
    /// Applied to every fresh [`egui::Context`]. Only the software backend
    /// builds more than one, when a resize rebuilds its painter.
    #[cfg(feature = "software")]
    ctx_init: fn(&egui::Context),
}

/// Both backends are boxed: they are 700 to 900 bytes each, and this enum is
/// moved by value along every path that builds a window.
enum Backend {
    Gl(Box<GlBackend>),
    #[cfg(feature = "software")]
    Software(Box<SoftwareBackend>),
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
        let backend = build_backend(&video_subsystem, config)?;
        let software = !matches!(backend, Backend::Gl(_));
        let ctx = match &backend {
            Backend::Gl(b) => &b.egui.ctx,
            #[cfg(feature = "software")]
            Backend::Software(b) => &b.egui.ctx,
        };
        ctx_init(ctx);
        apply_feathering(ctx, software);
        Ok(Self {
            _video_subsystem: video_subsystem,
            backend,
            #[cfg(feature = "software")]
            ctx_init,
        })
    }

    pub fn sdl2_window(&self) -> &sdl2::video::Window {
        match &self.backend {
            Backend::Gl(b) => &b.window,
            #[cfg(feature = "software")]
            Backend::Software(b) => b.canvas.window(),
        }
    }

    /// The rendering context Servo renders into: an FBO in our GL context, or
    /// swgl's CPU framebuffer when there is no GL to be had.
    pub fn rendering_ctx(&self) -> Rc<dyn RenderingContext> {
        match &self.backend {
            Backend::Gl(b) => b.rendering_ctx.clone(),
            #[cfg(feature = "software")]
            Backend::Software(b) => b.rendering_ctx.clone(),
        }
    }

    /// egui's handle to the page, for the web view to draw — `None` when the
    /// page is a CPU buffer this window composites under the chrome itself.
    pub fn browser_texture(&self) -> Option<egui::TextureId> {
        match &self.backend {
            Backend::Gl(b) => Some(b.browser_tex),
            #[cfg(feature = "software")]
            Backend::Software(_) => None,
        }
    }

    pub fn egui_ctx(&self) -> &egui::Context {
        match &self.backend {
            Backend::Gl(b) => &b.egui.ctx,
            #[cfg(feature = "software")]
            Backend::Software(b) => &b.egui.ctx,
        }
    }

    /// Run the UI; [`Self::paint`] puts the result on screen.
    pub fn run_ui(&mut self, run_ui: impl FnMut(&egui::Context)) {
        match &mut self.backend {
            Backend::Gl(b) => b.egui.run(run_ui),
            #[cfg(feature = "software")]
            Backend::Software(b) => b.egui.run(run_ui),
        }
    }

    /// How long until egui wants another frame, from the last [`Self::run_ui`].
    pub fn repaint_delay(&self) -> Duration {
        match &self.backend {
            Backend::Gl(b) => b.egui.repaint_delay(),
            #[cfg(feature = "software")]
            Backend::Software(b) => b.egui.repaint_delay(),
        }
    }

    /// Feed an SDL event to egui.
    pub fn on_event(&mut self, event: &sdl2::event::Event) -> EventResponse {
        match &mut self.backend {
            Backend::Gl(b) => b.egui.state.on_event(&b.window, event),
            #[cfg(feature = "software")]
            Backend::Software(b) => b.egui.state.on_event(b.canvas.window(), event),
        }
    }

    /// Refresh egui's cached window size from the live window, for the platforms
    /// where SDL doesn't deliver a size-changed event (see [`crate::ui::AppUi`]).
    #[cfg(target_os = "android")]
    pub fn sync_egui_window_size(&mut self) {
        match &mut self.backend {
            Backend::Gl(b) => b.egui.state.sync_window_size(&b.window),
            #[cfg(feature = "software")]
            Backend::Software(b) => b.egui.state.sync_window_size(b.canvas.window()),
        }
    }

    pub fn pointer_pos_in_points(&self) -> Option<egui::Pos2> {
        match &self.backend {
            Backend::Gl(b) => b.egui.state.get_pointer_pos_in_points(),
            #[cfg(feature = "software")]
            Backend::Software(b) => b.egui.state.get_pointer_pos_in_points(),
        }
    }

    /// Paint the last [`Self::run_ui`] and present it. `page_at` is where the web
    /// view's top-left sits in physical pixels; the software backend composites
    /// the page frame there, under the chrome, and leaves the one already there
    /// alone unless `page_painted` says the browser drew a new one.
    /// Returns where the time went, on the backend that has anything to say
    /// about it; the GL one leaves it to the driver.
    pub fn paint(&mut self, page_at: (i32, i32), page_painted: bool) -> Option<CompositeTiming> {
        // The GL backend draws the page as a texture in the same rect, so only
        // the software one has any use for where that rect is.
        #[cfg(not(feature = "software"))]
        let _ = (page_at, page_painted);
        match &mut self.backend {
            Backend::Gl(b) => {
                b.paint();
                None
            }
            #[cfg(feature = "software")]
            Backend::Software(b) => Some(b.paint(page_at, page_painted, self.ctx_init)),
        }
    }

    /// The shortest a frame may take, or `None` when presenting already paces the
    /// loop itself — which GL does, through the swap interval.
    pub fn frame_interval(&self) -> Option<Duration> {
        match &self.backend {
            Backend::Gl(_) => None,
            #[cfg(feature = "software")]
            Backend::Software(_) => Some(SOFTWARE_FRAME_INTERVAL),
        }
    }

    pub fn destroy(&mut self) {
        match &mut self.backend {
            Backend::Gl(b) => b.egui.destroy(),
            #[cfg(feature = "software")]
            Backend::Software(b) => b.egui.destroy(),
        }
    }

    /// Logical window size (matches SDL mouse-event coordinates).
    pub fn size(&self) -> (u32, u32) {
        self.sdl2_window().size()
    }

    /// Physical (drawable) window size in pixels — what the framebuffer and the
    /// browser's rendering context are sized in.
    pub fn drawable_size(&self) -> (u32, u32) {
        self.sdl2_window().drawable_size()
    }
}

/// egui smooths a shape's edges by tessellating extra triangles for them. On a
/// GPU those are free; on a software rasterizer they are what the frame is made
/// of, so the default follows the renderer. Text is unaffected either way —
/// glyphs carry their own antialiasing. `RETSURF_FEATHERING=0|1` overrides it,
/// which is how the two are compared on one page.
fn apply_feathering(ctx: &egui::Context, software: bool) {
    let on = match std::env::var("RETSURF_FEATHERING") {
        Ok(v) => v != "0",
        Err(_) => !software,
    };
    ctx.tessellation_options_mut(|o| o.feathering = on);
    log::info!("egui feathering: {on}");
}

/// GL unless the config asks for software, and software when GL doesn't come up
/// — a device with no driver at all (the Miyoo Mini) lands there on its own,
/// without needing the config to say so.
#[cfg(feature = "software")]
fn build_backend(video: &VideoSubsystem, config: &DisplayConfig) -> Result<Backend, String> {
    if config.software_render {
        return SoftwareBackend::new(video, config).map(|b| Backend::Software(Box::new(b)));
    }
    match GlBackend::new(video, config) {
        Ok(backend) => Ok(Backend::Gl(Box::new(backend))),
        Err(e) => {
            log::warn!("GL unavailable ({e}); falling back to software rendering");
            SoftwareBackend::new(video, config).map(|b| Backend::Software(Box::new(b)))
        }
    }
}

#[cfg(not(feature = "software"))]
fn build_backend(video: &VideoSubsystem, config: &DisplayConfig) -> Result<Backend, String> {
    GlBackend::new(video, config).map(|b| Backend::Gl(Box::new(b)))
}

/// Ask SDL for one GL attribute, and let a refusal be an error. The `gl_attr`
/// setters panic instead, which on a driver with no GL at all — the Miyoo's —
/// ends the process at the first attribute rather than falling back to software,
/// and `panic = "abort"` gives nothing to fall back from.
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
/// FBO, egui draws that FBO's colour texture into the window.
struct GlBackend {
    window: sdl2::video::Window,
    // Kept alive for the lifetime of the window; dropping it destroys the context.
    gl_context: GLContext,
    glow_ctx: Arc<glow::Context>,
    egui: EguiGlow,
    rendering_ctx: Rc<SdlRenderingContext>,
    /// egui's handle to the FBO colour texture. Its GL name is stable across
    /// resizes, so this stays valid for the program's lifetime.
    browser_tex: egui::TextureId,
}

impl GlBackend {
    fn new(video_subsystem: &VideoSubsystem, config: &DisplayConfig) -> Result<Self, String> {
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

        // Cap the main loop to the display refresh; without this the loop would
        // busy-spin while the gamepad drives continuous cursor/scroll updates.
        let _ = video_subsystem.gl_set_swap_interval(sdl2::video::SwapInterval::VSync);

        // glow (egui) and gleam (Servo/WebRender) both resolve GL entry points
        // through SDL's loader; one closure feeds all three loads.
        let get_proc =
            |name: &str| video_subsystem.gl_get_proc_address(name) as *const std::os::raw::c_void;

        let glow_ctx = Arc::new(unsafe { glow::Context::from_loader_function(get_proc) });

        // Servo/WebRender talks GL through `gleam`. Load the matching API for the
        // context profile we just created.
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
        })
    }

    fn paint(&mut self) {
        // Servo's context made itself current while rendering; restore SDL2's
        // before egui issues any GL call, and point egui back at the window
        // framebuffer (Servo's render left our FBO bound).
        if let Err(err) = self.window.gl_make_current(&self.gl_context) {
            log::error!("failed to make GL context current: {err}");
        }
        use glow::HasContext;
        unsafe { self.glow_ctx.bind_framebuffer(glow::FRAMEBUFFER, None) };
        self.egui.paint();
        self.window.gl_swap_window();
    }
}

/// No GL anywhere: swgl rasterizes the page, SDL's own renderer paints the
/// chrome over it, and the composed frame reaches the panel as one texture copy
/// — the only presentation path some drivers (the Miyoo Mini's `mmiyoo`) show.
#[cfg(feature = "software")]
struct SoftwareBackend {
    canvas: WindowCanvas,
    /// Where the frame is composed: the page first, then egui over it.
    offscreen: Canvas<Surface<'static>>,
    /// The composed frame, uploaded and copied once per frame.
    present: Texture,
    /// Window size the two above were built for.
    size: (u32, u32),
    egui: EguiCanvas<SurfaceContext<'static>>,
    rendering_ctx: Rc<SwglRenderingContext>,
    /// Whether the chrome's rectangles keep their corners (see [`corner_rounding`]).
    rounding: bool,
    /// The shapes the chrome now standing in the composition surface was drawn
    /// from — what [`chrome_damage`] compares the new frame against — and the
    /// areas it covers, one per drawn primitive. One enclosing rect instead
    /// would reach from the toolbar to wherever the cursor is, and call the page
    /// between them chrome.
    chrome_shapes: Vec<egui::epaint::ClippedShape>,
    chrome_rects: Vec<Rect>,
    /// Where the last page frame was blitted, for the frames that do not blit
    /// one and still have to clear around it.
    page_rect: Option<Rect>,
    /// What the frame before this one changed on the panel (see the present
    /// step), and whether to send the panel less than a whole frame at all.
    last_changed: Option<Rect>,
    partial: bool,
}

#[cfg(feature = "software")]
impl SoftwareBackend {
    fn new(video_subsystem: &VideoSubsystem, config: &DisplayConfig) -> Result<Self, String> {
        let mut window = build_window(video_subsystem, config, false)?;
        set_window_icon(&mut window);

        // No vsync request: not every driver this path serves advertises it, and
        // asking excludes those that don't.
        let canvas = window.into_canvas().build().map_err(|e| e.to_string())?;
        let size = canvas.output_size()?;
        log::info!(
            "window: software renderer `{}` ({}x{})",
            canvas.info().name,
            size.0,
            size.1
        );

        let (offscreen, present) = build_compose_targets(&canvas, size)?;
        let egui = EguiCanvas::for_surface_with_format(canvas.window(), &offscreen, COMPOSE_FORMAT);
        let rendering_ctx = SwglRenderingContext::new(dpi::PhysicalSize::new(size.0, size.1));
        log::info!("window: swgl rendering context created");

        Ok(Self {
            canvas,
            offscreen,
            present,
            size,
            egui,
            rendering_ctx,
            rounding: corner_rounding(),
            chrome_shapes: Vec::new(),
            chrome_rects: Vec::new(),
            page_rect: None,
            last_changed: None,
            partial: partial_present(),
        })
    }

    fn paint(
        &mut self,
        page_at: (i32, i32),
        page_painted: bool,
        ctx_init: fn(&egui::Context),
    ) -> CompositeTiming {
        self.resize_compose_targets(ctx_init);

        let Self {
            canvas,
            offscreen,
            present,
            egui,
            rendering_ctx,
            rounding,
            chrome_shapes,
            chrome_rects,
            page_rect,
            last_changed,
            partial,
            size,
            ..
        } = self;
        let mut timing = CompositeTiming::default();
        let at = Instant::now();

        // The chrome already in the surface stands where egui draws it the same
        // and this frame's page blit does not write over it.
        let changed = chrome_damage(egui, chrome_shapes, *size);
        let erased = page_painted
            .then(|| covered_chrome(chrome_rects, *page_rect))
            .flatten();
        let redraw = union(changed, erased);
        // The page goes down when the browser drew a new frame, and again under
        // whatever chrome is being redrawn: the chrome leaving may have covered
        // part of the page, and only the page can take those pixels back.
        let restore = (!page_painted).then_some(redraw).flatten();
        if page_painted || restore.is_some() {
            if let Some(blitted) = blit_page(offscreen, rendering_ctx, page_at, restore) {
                *page_rect = Some(blitted);
            }
        }
        if let Some(redraw) = redraw {
            // Only what the page does not cover: on this device the surface is
            // 1.7 MB and clearing all of it before overwriting most of it again
            // is a fifth of the frame's memory traffic for nothing.
            clear_around(offscreen, *page_rect, redraw);
        }
        timing.page = at.elapsed();

        match redraw {
            Some(redraw) => paint_chrome(
                egui,
                offscreen,
                *rounding,
                redraw,
                (chrome_shapes, chrome_rects),
                &mut timing,
            ),
            // The chrome on the surface is this frame's chrome. Drop the output
            // it would have been drawn from — its deltas are empty, or the frame
            // would have been redrawn.
            None => drop(egui.run_output.take()),
        }

        // What of the panel this frame changes — and what the frame before it
        // changed, because a driver that flips between two buffers is about to
        // show the one written two frames ago. `None` is a frame identical to
        // the one already on the panel, which is worth not sending at all.
        let changed = union(redraw, page_painted.then_some(*page_rect).flatten());
        let region = if *partial {
            union(changed, *last_changed)
        } else {
            Some(Rect::new(0, 0, size.0, size.1))
        };
        *last_changed = changed;

        let at = Instant::now();
        let surface = offscreen.surface();
        let pitch = surface.pitch() as usize;
        match (region, surface.without_lock()) {
            (Some(region), Some(pixels)) => {
                // `update` reads `region.height()` rows of `region.width()`
                // pixels, a pitch apart, so it wants the surface from that
                // rect's own first pixel.
                let from = region.y() as usize * pitch + region.x() as usize * BYTES_PER_PIXEL;
                if let Err(e) = present.update(Some(region), &pixels[from..], pitch) {
                    log::error!("could not upload the composed frame: {e}");
                }
            }
            (Some(_), None) => log::error!("composition surface has no readable pixels"),
            (None, _) => {}
        }
        timing.upload = at.elapsed();

        let at = Instant::now();
        if let Some(region) = region {
            if let Err(e) = canvas.copy(present, Some(region), Some(region)) {
                log::error!("could not blit the composed frame: {e}");
            }
            canvas.present();
        }
        timing.present = at.elapsed();
        timing
    }

    /// Keep the composition targets the size of the window. Rebuilding the
    /// painter drops egui's textures, and egui only ever uploads them once per
    /// [`egui::Context`] — so the context is rebuilt with it and restyled.
    fn resize_compose_targets(&mut self, ctx_init: fn(&egui::Context)) {
        let Ok(size) = self.canvas.output_size() else {
            return;
        };
        if size == self.size {
            return;
        }
        let (offscreen, present) = match build_compose_targets(&self.canvas, size) {
            Ok(targets) => targets,
            Err(e) => return log::error!("could not resize the composition targets: {e}"),
        };
        self.egui.destroy();
        self.egui =
            EguiCanvas::for_surface_with_format(self.canvas.window(), &offscreen, COMPOSE_FORMAT);
        ctx_init(&self.egui.ctx);
        apply_feathering(&self.egui.ctx, true);
        self.offscreen = offscreen;
        // The new surface holds neither the chrome nor the page, whatever the
        // shapes say.
        self.chrome_shapes.clear();
        self.chrome_rects.clear();
        self.page_rect = None;
        self.last_changed = None;
        // `unsafe_textures` (the canvas backend's) makes textures outlive their
        // creator, so the old one has to go by hand.
        let stale = std::mem::replace(&mut self.present, present);
        unsafe { stale.destroy() };
        self.size = size;
    }
}

/// What [`EguiCanvas::paint`] does, taken apart so each step is timed on its
/// own: the chrome is most of a GPU-less frame, and one number for it does not
/// say whether the cost is the triangles or the pixels.
#[cfg(feature = "software")]
fn paint_chrome(
    egui: &mut EguiCanvas<SurfaceContext<'static>>,
    offscreen: &mut Canvas<Surface<'static>>,
    rounding: bool,
    damage: Rect,
    standing: (&mut Vec<egui::epaint::ClippedShape>, &mut Vec<Rect>),
    timing: &mut CompositeTiming,
) {
    let (painted, areas) = standing;
    let pixels_per_point = egui.run_output.pixels_per_point;
    let (mut textures_delta, mut shapes) = egui.run_output.take();

    // Kept as egui gave them, so the next frame's comparison is like for like;
    // squaring is a function of the shapes and cannot make two frames differ.
    painted.clear();
    painted.extend_from_slice(&shapes);

    if !rounding {
        for clipped in &mut shapes {
            square_corners(&mut clipped.shape);
        }
    }

    let at = Instant::now();
    let primitives = egui.ctx.tessellate(shapes, pixels_per_point);
    timing.tessellate = at.elapsed();

    let at = Instant::now();
    // Drained, not dropped: egui asserts that every delta was applied.
    for (id, deltas) in textures_delta.set.drain() {
        for delta in deltas {
            egui.painter.set_texture(id, &delta);
        }
    }
    timing.textures = at.elapsed();

    // Every primitive's area, not just the damaged ones: what stands in the
    // surface after this is the shapes outside the damage, which did not move,
    // plus the ones inside it.
    areas.clear();
    areas.extend(primitives_areas(&primitives, pixels_per_point));

    let at = Instant::now();
    egui.painter
        .paint_primitives_within(offscreen, pixels_per_point, primitives, Some(damage));
    timing.chrome = at.elapsed();

    for id in textures_delta.free.drain() {
        egui.painter.free_texture(&id);
    }
}

/// Where egui's new frame differs from the one already in the composition
/// surface: `None` when it draws exactly the same, and the whole window when the
/// two cannot be told apart shape by shape (one holds shapes the other does not,
/// or a texture changed under both). On a GPU-less device the pixels are the
/// cost, and a moving cursor or a typed character changes very few of them — so
/// what the rest of the chrome costs is finding that out.
#[cfg(feature = "software")]
fn chrome_damage(
    egui: &EguiCanvas<SurfaceContext<'static>>,
    painted: &[egui::epaint::ClippedShape],
    size: (u32, u32),
) -> Option<Rect> {
    let whole = Rect::new(0, 0, size.0, size.1);
    let fresh = &egui.run_output.shapes;
    if !egui.run_output.textures_delta.is_empty() || fresh.len() != painted.len() {
        return Some(whole);
    }
    let mut damage: Option<egui::Rect> = None;
    for (fresh, painted) in fresh.iter().zip(painted) {
        if fresh == painted {
            continue;
        }
        // Both sides: the shape has to be drawn where it now is, and erased
        // where it was.
        for clipped in [fresh, painted] {
            let bounds = clipped
                .shape
                .visual_bounding_rect()
                .intersect(clipped.clip_rect);
            if bounds.is_positive() {
                damage = Some(damage.map_or(bounds, |d| d.union(bounds)));
            }
        }
    }
    // A point of margin: a shape's visual bounds do not always account for how
    // far its own antialiasing reaches.
    to_pixels(damage?.expand(1.0), egui.run_output.pixels_per_point).intersection(whole)
}

/// The smallest whole pixels covering `rect`: a shape over part of one still
/// wrote it.
#[cfg(feature = "software")]
fn to_pixels(rect: egui::Rect, pixels_per_point: f32) -> Rect {
    let min = (rect.min * pixels_per_point).floor();
    let max = (rect.max * pixels_per_point).ceil();
    Rect::new(
        min.x as i32,
        min.y as i32,
        (max.x - min.x).max(0.0) as u32,
        (max.y - min.y).max(0.0) as u32,
    )
}

/// The rect covering both, or whichever of them there is.
#[cfg(feature = "software")]
fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.union(b)),
        (some, None) | (None, some) => some,
    }
}

/// The pixels each of `primitives` draws over. Their clip rects would be a
/// cheaper answer and a useless one: a foreground layer is clipped to the whole
/// window whatever it holds.
#[cfg(feature = "software")]
fn primitives_areas(
    primitives: &[egui::ClippedPrimitive],
    pixels_per_point: f32,
) -> impl Iterator<Item = Rect> + '_ {
    primitives.iter().filter_map(move |primitive| {
        // A paint callback draws through the renderer, which this backend has
        // none of; the painter logs it and skips it.
        let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive else {
            return None;
        };
        if mesh.is_empty() {
            return None;
        }
        let drawn = mesh.calc_bounds().intersect(primitive.clip_rect);
        drawn
            .is_positive()
            .then(|| to_pixels(drawn, pixels_per_point))
    })
}

/// How much of the chrome the page frame is about to overwrite: the rect
/// enclosing the parts of it the page reaches, `None` if it reaches none.
#[cfg(feature = "software")]
fn covered_chrome(chrome: &[Rect], page: Option<Rect>) -> Option<Rect> {
    let page = page?;
    chrome
        .iter()
        .filter_map(|area| area.intersection(page))
        .reduce(|a, b| a.union(b))
}

/// Square off a shape's corners, recursing into a group. egui triangulates a
/// rounded rectangle whole and SDL rasterizes a triangle at some fifteen times
/// the per-pixel cost of a rectangle fill, so the radius decides which path the
/// whole shape takes — there is no cheaper radius, only none.
#[cfg(feature = "software")]
fn square_corners(shape: &mut egui::Shape) {
    match shape {
        egui::Shape::Rect(rect) => rect.corner_radius = egui::epaint::CornerRadius::ZERO,
        egui::Shape::Vec(shapes) => shapes.iter_mut().for_each(square_corners),
        _ => {}
    }
}

/// Whether the panel may be sent less than a whole frame. On by default: the
/// copy is what a frame costs once the chrome stops being redrawn, and most
/// frames change a cursor's worth of it. `RETSURF_PARTIAL_PRESENT=0` sends whole
/// frames again — the answer if a driver's buffering shows stale pixels.
#[cfg(feature = "software")]
fn partial_present() -> bool {
    let on = std::env::var("RETSURF_PARTIAL_PRESENT")
        .ok()
        .is_none_or(|v| v != "0");
    log::info!("partial present: {on}");
    on
}

/// Whether the chrome keeps its rounded corners. Off on this backend, where they
/// are over half of what rasterizing the chrome costs (see [`square_corners`]);
/// `RETSURF_ROUNDING=1` puts them back, which is how the two are compared on one
/// page. Text and circles are unaffected either way.
#[cfg(feature = "software")]
fn corner_rounding() -> bool {
    let on = std::env::var("RETSURF_ROUNDING").is_ok_and(|v| v != "0");
    log::info!("egui corner rounding: {on}");
    on
}

/// The surface the frame is composed in and the texture it is presented through.
#[cfg(feature = "software")]
fn build_compose_targets(
    canvas: &WindowCanvas,
    size: (u32, u32),
) -> Result<(Canvas<Surface<'static>>, Texture), String> {
    let surface = Surface::new(size.0, size.1, COMPOSE_FORMAT)?;
    let offscreen = Canvas::from_surface(surface)?;
    let mut present = canvas
        .texture_creator()
        .create_texture_streaming(COMPOSE_FORMAT, size.0, size.1)
        .map_err(|e| e.to_string())?;
    // A whole frame replaces rather than blends: SDL gives a format with alpha
    // `BLEND` by default, and there is nothing under a full frame to mix with.
    present.set_blend_mode(BlendMode::None);
    Ok((offscreen, present))
}

/// Fill everything within `damage` that `covered` — where the page was written —
/// does not reach. No page rect means the whole damage needs it.
#[cfg(feature = "software")]
fn clear_around(offscreen: &mut Canvas<Surface<'static>>, covered: Option<Rect>, damage: Rect) {
    offscreen.set_draw_color(CLEAR_COLOR);
    let clear = |canvas: &mut Canvas<Surface<'static>>| {
        if let Err(e) = canvas.fill_rect(damage) {
            log::error!("could not clear the damaged area: {e}");
        }
    };
    let Some(page) = covered else {
        return clear(offscreen);
    };
    let Ok((w, h)) = offscreen.output_size() else {
        return clear(offscreen);
    };
    let (w, h) = (w as i32, h as i32);
    let strips = [
        Rect::new(0, 0, w as u32, page.y().max(0) as u32),
        Rect::new(
            0,
            page.bottom(),
            w as u32,
            (h - page.bottom()).max(0) as u32,
        ),
        Rect::new(0, page.y(), page.x().max(0) as u32, page.height()),
        Rect::new(
            page.right(),
            page.y(),
            (w - page.right()).max(0) as u32,
            page.height(),
        ),
    ];
    // Zero-sized strips are the common case (the page spans the width), and so
    // are strips the damage does not reach; both drop out here.
    let strips: Vec<Rect> = strips
        .iter()
        .filter_map(|strip| strip.intersection(damage))
        .collect();
    if let Err(e) = offscreen.fill_rects(&strips) {
        log::error!("could not clear around the page: {e}");
    }
}

/// Copy the page frame into the composition surface at `at`, rows reversed —
/// swgl leaves it bottom-up, like the GL framebuffer WebRender believes it is
/// drawing into. Both sides are BGRA, so a row is a memcpy. `within` narrows the
/// copy to a rect, for a frame that only needs the page back under a piece of
/// chrome. Returns where the page sits, whole, for [`clear_around`].
#[cfg(feature = "software")]
fn blit_page(
    offscreen: &mut Canvas<Surface<'static>>,
    page: &SwglRenderingContext,
    at: (i32, i32),
    within: Option<Rect>,
) -> Option<Rect> {
    let frame = page.frame()?;
    let surface = offscreen.surface_mut();
    let pitch = surface.pitch() as usize;
    let (dst_w, dst_h) = (surface.width() as usize, surface.height() as usize);
    let (x, y) = (at.0.max(0) as usize, at.1.max(0) as usize);
    let w = (frame.width as usize).min(dst_w.saturating_sub(x));
    let h = (frame.height as usize).min(dst_h.saturating_sub(y));
    if w == 0 || h == 0 {
        return None;
    }
    let whole = Rect::new(x as i32, y as i32, w as u32, h as u32);
    let Some(copy) = (match within {
        Some(within) => whole.intersection(within),
        None => Some(whole),
    }) else {
        return Some(whole);
    };
    let Some(dst) = surface.without_lock_mut() else {
        log::error!("composition surface has no writable pixels");
        return None;
    };
    let skip_rows = (copy.y() - whole.y()) as usize;
    let from = (copy.x() - whole.x()) as usize * BYTES_PER_PIXEL;
    let row = copy.width() as usize * BYTES_PER_PIXEL;
    for (dy, src) in frame
        .rows_top_down()
        .skip(skip_rows)
        .take(copy.height() as usize)
        .enumerate()
    {
        let at = (copy.y() as usize + dy) * pitch + copy.x() as usize * BYTES_PER_PIXEL;
        dst[at..at + row].copy_from_slice(&src[from..from + row]);
    }
    Some(whole)
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

/// The panel's own size on a Miyoo, where a window smaller than the framebuffer
/// is drawn centred with a black border round it — the Flip's panel is 752x560,
/// not the 640x480 the config defaults to. `None` on every other driver, where
/// the configured size is a real choice.
fn panel_size(video_subsystem: &VideoSubsystem) -> Option<(u32, u32)> {
    if !matches!(video_subsystem.current_video_driver(), "Mini" | "mmiyoo") {
        return None;
    }
    // Older builds of this driver leave the mode zeroed, so treat an
    // implausibly small one as no answer rather than opening a window nothing
    // can draw into.
    let mode = video_subsystem.desktop_display_mode(0).ok()?;
    let (w, h) = (u32::try_from(mode.w).ok()?, u32::try_from(mode.h).ok()?);
    (w >= 320 && h >= 240).then(|| {
        log::info!("panel: {w}x{h} from the video driver");
        (w, h)
    })
}

/// Set the window icon from the bundled brand PNG (RGBA8), baked into the binary.
/// Best-effort: any decode failure just logs and leaves SDL's default. Bare-kmsdrm
/// and Android have no window-icon concept, so SDL no-ops there harmlessly.
fn set_window_icon(window: &mut sdl2::video::Window) {
    use sdl2::pixels::PixelFormatEnum;
    static ICON_PNG: &[u8] = include_bytes!("../../resources/icon.png");

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
