//! The brand wordmark, painted: "ret" in ink, "surf" in the brand gradient,
//! over the brand wave. egui has no gradient text fill, so the `surf` glyphs
//! are tagged with a marker colour, tessellated here, and recoloured by height.
//! The start page and the splash draw it; the layout needs its size first.

use super::theme::{lerp_color, ACCENT, INK, SURF_WARM};
use egui_sdl2::egui;

/// Wordmark type size (logical px); the wave band and the reserved height
/// derive from it.
const WORDMARK_SIZE: f32 = 26.0;
/// Letter spacing as a share of the type size, matching the SVG wordmark.
const WORDMARK_TRACKING: f32 = 0.1;
/// The wave band below the text. Ratios track the SVG, nudged up so the thin
/// stroke stays legible on a handheld.
const WAVE_GAP: f32 = WORDMARK_SIZE * 0.2; // text bottom to wave centerline
const WAVE_AMP: f32 = WORDMARK_SIZE * 0.08; // crest height
const WAVE_STROKE: f32 = WORDMARK_SIZE * 0.045;
const WAVE_BAND: f32 = WAVE_GAP + WAVE_AMP + WAVE_STROKE;
/// The wave's periods under the text, and the segments drawing it.
const WAVE_PERIODS: f32 = 3.0;
const WAVE_SEGMENTS: usize = 120;
/// The wave's overhang past the text at either end, a share of the text's width.
const WAVE_OVERHANG: f32 = 0.02;
/// Where the `surf` gradient warms from teal, as a share of the x-height.
const WARM_FROM: f32 = 0.55;

/// The laid-out text, "ret" in ink and "surf" in the marker colour.
fn wordmark_job(size: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    // Keep the wordmark on Hack (monospace) for its logo feel, independent of the
    // body font.
    let fmt = |color: egui::Color32| egui::TextFormat {
        font_id: egui::FontId::monospace(size),
        color,
        extra_letter_spacing: size * WORDMARK_TRACKING,
        ..Default::default()
    };
    job.append("ret", 0.0, fmt(INK));
    // epaint skips extra_letter_spacing on a section's first glyph, so the
    // ret/surf joint needs it back. ACCENT is a marker recolored below.
    job.append("surf", size * WORDMARK_TRACKING, fmt(ACCENT));
    job
}

/// The mark's laid-out height at its own size, wave band included.
pub(super) fn wordmark_height(ui: &egui::Ui) -> f32 {
    wordmark_text_size(ui, WORDMARK_SIZE).y + WAVE_BAND
}

/// The wordmark at its own size, allocated in `ui`: the text over the wave.
pub(super) fn add_wordmark(ui: &mut egui::Ui) {
    let gsize = wordmark_text_size(ui, WORDMARK_SIZE);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(gsize.x, gsize.y + WAVE_BAND),
        egui::Sense::hover(),
    );
    paint_wordmark_text(ui, rect.min, WORDMARK_SIZE);

    // The brand wave: a solid-teal sine under the text, starting on the
    // centerline and dipping down first — the SVG path's phase.
    let over = gsize.x * WAVE_OVERHANG;
    let (x0, x1) = (rect.min.x - over, rect.max.x + over);
    let cy = rect.min.y + gsize.y + WAVE_GAP;
    let pts: Vec<egui::Pos2> = (0..=WAVE_SEGMENTS)
        .map(|i| {
            let t = i as f32 / WAVE_SEGMENTS as f32;
            let y = cy + WAVE_AMP * (t * WAVE_PERIODS * std::f32::consts::TAU).sin();
            egui::pos2(x0 + (x1 - x0) * t, y)
        })
        .collect();
    ui.painter().add(egui::Shape::line(
        pts,
        egui::Stroke::new(WAVE_STROKE, ACCENT),
    ));
}

/// The laid-out size of the wordmark's text at type `size`, wave band excluded.
pub(super) fn wordmark_text_size(ui: &egui::Ui, size: f32) -> egui::Vec2 {
    ui.ctx()
        .fonts_mut(|f| f.layout_job(wordmark_job(size)))
        .size()
}

/// Paint the wordmark's text at type `size` with its top-left at `pos`,
/// allocating nothing.
pub(super) fn paint_wordmark_text(ui: &egui::Ui, pos: egui::Pos2, size: f32) {
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(wordmark_job(size)));

    // Recolor the marker-colored `surf` vertices by height: teal over the lower
    // part, warming to coral — the stops of `_surf_gradient` in the brand SVGs.
    let ppp = ui.ctx().pixels_per_point();
    let tex = ui.ctx().fonts(|f| f.font_image_size());
    let mut mesh = egui::epaint::Mesh::default();
    let shape = egui::epaint::TextShape::new(pos, galley, INK);
    let opts = egui::epaint::TessellationOptions::default();
    egui::epaint::Tessellator::new(ppp, opts, tex, Vec::new()).tessellate_text(&shape, &mut mesh);

    // Key the gradient to the x-height, not the full vertex span: `f`'s ascender
    // reaches higher, and spanning to it left the other glyph tops short of coral.
    let ys: Vec<f32> = mesh
        .vertices
        .iter()
        .filter(|v| v.color == ACCENT)
        .map(|v| v.pos.y)
        .collect();
    let bot = ys.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    // Each glyph is one quad (4 verts); the x-height is the lowest of the glyph
    // tops (the tall `f` top is the outlier and is excluded by taking the max).
    let x_top = ys
        .chunks(4)
        .map(|g| g.iter().copied().fold(f32::INFINITY, f32::min))
        .fold(f32::NEG_INFINITY, f32::max);
    let span = (bot - x_top).max(1.0);
    for v in mesh.vertices.iter_mut().filter(|v| v.color == ACCENT) {
        let frac = ((bot - v.pos.y) / span).clamp(0.0, 1.0); // 0 baseline, 1 x-height+
        let t = ((frac - WARM_FROM) / (1.0 - WARM_FROM)).clamp(0.0, 1.0);
        v.color = lerp_color(ACCENT, SURF_WARM, t);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
}
