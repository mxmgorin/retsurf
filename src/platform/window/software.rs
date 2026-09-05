use super::{apply_feathering, build_window, set_window_icon, CompositeTiming};
use crate::config::DisplayConfig;
use crate::platform::render::{SwglRenderingContext, BYTES_PER_PIXEL};
use egui_sdl2::{egui, EguiCanvas};
use sdl2::pixels::{Color, PixelFormatEnum};
use sdl2::rect::Rect;
use sdl2::render::{BlendMode, Canvas, Texture, WindowCanvas};
use sdl2::surface::{Surface, SurfaceContext};
use sdl2::VideoSubsystem;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// swgl outputs BGRA bytes — SDL's `ARGB8888` — so composing in the same format
/// keeps the page blit a straight row copy.
const COMPOSE_FORMAT: PixelFormatEnum = PixelFormatEnum::ARGB8888;

/// Background where neither the page nor the chrome covers.
const CLEAR_COLOR: Color = Color::BLACK;

/// Frame cap from `[display] max_fps` (`0` uncapped). Nothing on this path
/// blocks like a GL swap, so an uncapped scrolling frame pins the CPU.
fn frame_interval(max_fps: u32) -> Option<Duration> {
    let interval = (max_fps > 0).then(|| Duration::from_secs_f64(1.0 / max_fps as f64));
    match interval {
        Some(interval) => log::info!("frame cap: {max_fps} fps ({:.1?} a frame)", interval),
        None => log::info!("frame cap: none"),
    }
    interval
}

/// No GL anywhere: swgl rasterizes the page, SDL's own renderer paints the
/// chrome over it, one texture copy to the panel — the only presentation path
/// some drivers (the Miyoo Mini's `mmiyoo`) support.
pub(super) struct SoftwareBackend {
    pub(super) canvas: WindowCanvas,
    /// Composition target: the page first, then egui over it.
    offscreen: Canvas<Surface<'static>>,
    /// The composed frame, uploaded and copied once per frame.
    present: Texture,
    /// Window size the two above were built for.
    size: (u32, u32),
    pub(super) egui: EguiCanvas<SurfaceContext<'static>>,
    pub(super) rendering_ctx: Rc<SwglRenderingContext>,
    /// Whether the chrome keeps rounded corners (see [`corner_rounding`]).
    rounding: bool,
    /// Shapes the chrome standing in the surface was drawn from; what
    /// [`chrome_damage`] diffs against.
    chrome_shapes: Vec<egui::epaint::ClippedShape>,
    /// Areas that chrome covers, one per primitive — one enclosing rect would
    /// count the page between toolbar and cursor as chrome.
    chrome_rects: Vec<Rect>,
    /// Where the last page frame was blitted, for frames that clear around it
    /// without blitting one.
    page_rect: Option<Rect>,
    /// Panel area the previous frame changed (see the present step).
    last_changed: Option<Rect>,
    /// Whether to send the panel less than a whole frame (see [`partial_present`]).
    partial: bool,
    /// The frame cap, as a minimum frame time (see [`frame_interval`]).
    pub(super) frame_interval: Option<Duration>,
}

impl SoftwareBackend {
    pub(super) fn new(
        video_subsystem: &VideoSubsystem,
        config: &DisplayConfig,
    ) -> Result<Self, String> {
        let mut window = build_window(video_subsystem, config, false)?;
        set_window_icon(&mut window);

        // No vsync request: some drivers on this path don't advertise it, and
        // asking excludes them.
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
            frame_interval: frame_interval(config.max_fps),
        })
    }

    pub(super) fn set_max_fps(&mut self, max_fps: u32) {
        self.frame_interval = frame_interval(max_fps);
    }

    /// Composition surface, presentation texture, swgl framebuffer.
    pub(super) fn compose_bytes(&self) -> usize {
        const FRAMES: usize = 3;
        let (w, h) = self.size;
        FRAMES * w as usize * h as usize * BYTES_PER_PIXEL
    }

    pub(super) fn paint(
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

        // Chrome already in the surface stands unless egui draws it differently
        // or this frame's page blit writes over it.
        let changed = chrome_damage(egui, chrome_shapes, *size);
        let erased = page_painted
            .then(|| covered_chrome(chrome_rects, *page_rect))
            .flatten();
        let redraw = union(changed, erased);
        // Blit on a new browser frame, and under redrawn chrome: departing
        // chrome may cover page pixels only the page can restore.
        let restore = (!page_painted).then_some(redraw).flatten();
        if page_painted || restore.is_some() {
            if let Some(blitted) = blit_page(offscreen, rendering_ctx, page_at, restore) {
                *page_rect = Some(blitted);
            }
        }
        if let Some(redraw) = redraw {
            // Only outside the page rect: clearing the whole surface before
            // overwriting most of it is wasted memory traffic.
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
            // Chrome unchanged: drop egui's output. Its deltas are empty, or
            // the frame would have been redrawn.
            None => drop(egui.run_output.take()),
        }

        // `None`: frame identical to the panel's, skip the copy. What is sent
        // is the whole frame — see `partial_present`.
        let changed = union(redraw, page_painted.then_some(*page_rect).flatten());
        let region = changed.map(|changed| {
            if *partial {
                union(Some(changed), *last_changed).unwrap_or(changed)
            } else {
                Rect::new(0, 0, size.0, size.1)
            }
        });
        *last_changed = changed;

        let at = Instant::now();
        let surface = offscreen.surface();
        let pitch = surface.pitch() as usize;
        // Whole frame even for a partial present: a streaming texture does not
        // promise to keep what was uploaded before.
        match (region, surface.without_lock()) {
            (Some(_), Some(pixels)) => {
                if let Err(e) = present.update(None, pixels, pitch) {
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

    /// Keep the composition targets window-sized. Rebuilding the painter drops
    /// egui's textures, uploaded once per [`egui::Context`], so the context is
    /// rebuilt and restyled with it.
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
        // A new surface holds neither the chrome nor the page.
        self.chrome_shapes.clear();
        self.chrome_rects.clear();
        self.page_rect = None;
        self.last_changed = None;
        // `unsafe_textures` lets textures outlive their creator; destroy the
        // old one by hand.
        let stale = std::mem::replace(&mut self.present, present);
        unsafe { stale.destroy() };
        self.size = size;
    }
}

/// [`EguiCanvas::paint`] taken apart so each step is timed on its own: one
/// number does not say whether the cost is the triangles or the pixels.
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

    // Stored unsquared, so the next frame's diff is like for like.
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

    // All primitive areas, not just the damaged: shapes outside the damage
    // still stand in the surface.
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

/// Where egui's new frame differs from the surface: `None` when it draws the
/// same, the whole window when the two cannot be compared shape by shape.
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
        // Both sides: drawn where it now is, erased where it was.
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
    // One point of margin: visual bounds may understate antialiasing reach.
    to_pixels(damage?.expand(1.0), egui.run_output.pixels_per_point).intersection(whole)
}

/// The smallest whole-pixel rect covering `rect`.
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
fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.union(b)),
        (some, None) | (None, some) => some,
    }
}

/// The pixels each primitive draws over. Clip rects would be cheaper and
/// useless: a foreground layer is clipped to the whole window.
fn primitives_areas(
    primitives: &[egui::ClippedPrimitive],
    pixels_per_point: f32,
) -> impl Iterator<Item = Rect> + '_ {
    primitives.iter().filter_map(move |primitive| {
        // Paint callbacks need a renderer this backend lacks; the painter
        // logs and skips them.
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

/// Chrome the page blit is about to overwrite: the rect enclosing the parts the
/// page reaches, `None` if it reaches none.
fn covered_chrome(chrome: &[Rect], page: Option<Rect>) -> Option<Rect> {
    let page = page?;
    chrome
        .iter()
        .filter_map(|area| area.intersection(page))
        .reduce(|a, b| a.union(b))
}

/// Square a shape's corners, recursing into groups: rounded rects tessellate to
/// triangles, which SDL rasterizes at ~15x the per-pixel cost of a rect fill.
fn square_corners(shape: &mut egui::Shape) {
    match shape {
        egui::Shape::Rect(rect) => rect.corner_radius = egui::epaint::CornerRadius::ZERO,
        egui::Shape::Vec(shapes) => shapes.iter_mut().for_each(square_corners),
        _ => {}
    }
}

/// Whether the panel copy may be clipped to what changed. Off: the Miyoo driver
/// draws a clipped copy in the wrong place. `RETSURF_PARTIAL_PRESENT=1` opts in.
fn partial_present() -> bool {
    let on = std::env::var("RETSURF_PARTIAL_PRESENT").is_ok_and(|v| v != "0");
    log::info!("partial present: {on}");
    on
}

/// Whether the chrome keeps rounded corners. Off: over half the chrome's raster
/// cost (see [`square_corners`]). `RETSURF_ROUNDING=1` puts them back.
fn corner_rounding() -> bool {
    let on = std::env::var("RETSURF_ROUNDING").is_ok_and(|v| v != "0");
    log::info!("egui corner rounding: {on}");
    on
}

/// The surface the frame is composed in and the texture it is presented through.
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
    // A whole frame replaces, never blends; SDL defaults alpha formats to `BLEND`.
    present.set_blend_mode(BlendMode::None);
    Ok((offscreen, present))
}

/// Fill `damage` outside `covered` (where the page was written). No page rect
/// means the whole damage needs it.
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
    // Zero-sized strips and strips the damage does not reach drop out here.
    let strips: Vec<Rect> = strips
        .iter()
        .filter_map(|strip| strip.intersection(damage))
        .collect();
    if let Err(e) = offscreen.fill_rects(&strips) {
        log::error!("could not clear around the page: {e}");
    }
}

/// Blit the page into the surface at `at`, rows reversed (swgl is bottom-up);
/// both sides BGRA, so a row is a memcpy. `within` narrows the copy. Returns
/// where the page sits, whole, for [`clear_around`].
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
