//! Rendering of the built-in start page overlay (state lives in
//! [`crate::overlay::home`]): a wordmark, a search / URL field, a speed-dial
//! grid of the pinned shortcuts, and a bottom control-hint bar. Navigation is
//! routed by [`crate::app`]; tiles open via [`MenuAction::OpenUrl`].

use super::theme::{ACCENT, BG, BORDER, INK, MUTED, SURFACE, SURF_WARM};
use super::OskCaret;
use crate::command::{AppCommand, MenuAction};
use crate::config::FaceLabels;
use crate::data::dial::SETTINGS_PIN;
use crate::overlay::home::Home;
use egui_phosphor::bold;
use egui_sdl2::egui;
use std::cell::RefCell;
use std::collections::HashMap;

/// Typeface for the start-page body text. egui bundles only two real text faces
/// — `Ubuntu-Light` and `Hack`; swap the call below to flip between them. The
/// wordmark stays on Hack via [`add_wordmark`] regardless.
fn font(size: f32) -> egui::FontId {
    egui::FontId::proportional(size)
}

/// Tile footprint (logical px) and grid spacing. Shared with the dial editor.
pub(super) const TILE_W: f32 = 96.0;
pub(super) const TILE_H: f32 = 84.0;
pub(super) const GAP: f32 = 12.0;

/// Wordmark type size (logical px); the wave band and the reserved height
/// derive from it.
const WORDMARK_SIZE: f32 = 36.0;
/// Letter spacing, matching the SVG wordmark's tracking.
const WORDMARK_TRACKING: f32 = WORDMARK_SIZE * 0.1;
/// The wave band below the text. Ratios track the SVG, nudged up so the thin
/// stroke stays legible on a handheld.
const WAVE_GAP: f32 = WORDMARK_SIZE * 0.2; // text bottom to wave centerline
const WAVE_AMP: f32 = WORDMARK_SIZE * 0.08; // crest height
const WAVE_STROKE: f32 = WORDMARK_SIZE * 0.045;
const WAVE_BAND: f32 = WAVE_GAP + WAVE_AMP + WAVE_STROKE;

/// Hint bar geometry: centerline [`HINT_BASE`] off the page's foot, [`HINT_BAND`]
/// the strip content must stay out of.
const HINT_BASE: f32 = 18.0;
const HINT_PILL_H: f32 = 18.0;
const HINT_BAND: f32 = HINT_BASE + HINT_PILL_H / 2.0 + 8.0;

/// Draw the start-page overlay over the (blank) web view, confined to the
/// `webview` rect so the toolbar stays usable. Any activation is pushed as a
/// command for the app to execute.
pub(super) fn add_home(
    ctx: &egui::Context,
    home: &mut Home,
    pins: &[String],
    webview: egui::Rect,
    osk_caret: Option<OskCaret>,
    face: FaceLabels,
    commands: &mut Vec<AppCommand>,
) {
    let area = webview;
    egui::Area::new(egui::Id::new("home"))
        .order(egui::Order::Middle)
        .fixed_pos(area.min)
        // Don't let egui shift the area up to fit the screen — it must stay
        // pinned to the web-view rect, even if content overflows.
        .constrain(false)
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(BG)
                .inner_margin(0.0)
                .show(ui, |ui| {
                    // `Area` caches its size by id and `set_min_size` only grows,
                    // so a rotation would keep centering against the stale width.
                    ui.set_min_size(area.size());
                    ui.set_max_size(area.size());
                    // Columns first, then the field takes the grid's width: sized the
                    // other way round, the grid came out up to 92px narrower.
                    let max_w = (area.width() * 0.9).min(620.0);
                    let cols = (((max_w + GAP) / (TILE_W + GAP)).floor() as usize).max(1);
                    let block_w = (cols as f32 * TILE_W + (cols - 1) as f32 * GAP).min(max_w);
                    home.set_cols(cols);

                    // The field anchors the page. Centering the whole block moved the
                    // mark and the field whenever a pin added a grid row.
                    const FIELD_ANCHOR: f32 = 0.3; // field top, share of the height
                    const GAP_MID: f32 = 36.0; // field to grid
                    const GAP_TOP_RATIO: f32 = 0.6; // mark to field, of the mark's own height

                    let mark_h = wordmark_height(ui);
                    let gap_top = mark_h * GAP_TOP_RATIO;
                    let head_h = mark_h + gap_top; // above the field

                    // Content stops above the hint bar; the grid scrolls in what is
                    // left, so a long dial can't hide rows off the page.
                    let floor = area.bottom() - HINT_BAND;
                    let top = ((floor - area.top()) * FIELD_ANCHOR - head_h).max(8.0);

                    ui.vertical_centered(|ui| {
                        ui.add_space(top);
                        add_wordmark(ui);
                        ui.add_space(gap_top);
                        add_search(ui, home, block_w, osk_caret);
                        ui.add_space(GAP_MID);
                        let rest = (floor - ui.cursor().top()).max(TILE_H);
                        // The viewport spans the page and the grid centers inside
                        // it; shrunk to fit, the scrollbar would push it off-center.
                        egui::ScrollArea::vertical()
                            .max_height(rest)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.vertical_centered(|ui| {
                                    add_dial(ui, home, pins, block_w, cols, commands);
                                });
                            });
                    });
                    add_hint_bar(ui, area, face);
                });
        });
}

/// The bottom control-hint bar: key-cap pills with their action. Painted rather
/// than laid out in the flow, so the tile count can't move it.
fn add_hint_bar(ui: &egui::Ui, area: egui::Rect, face: FaceLabels) {
    let hints = [(face.a, "Open"), (bold::LIST, "Menu")];
    const PAD: f32 = 6.0; // pill horizontal padding around the key glyph
    const GAP_KL: f32 = 6.0; // key pill to its label
    const GAP_SEG: f32 = 18.0; // between hint segments
    let key_font = font(12.0);
    let label_font = font(12.0);
    let painter = ui.painter();

    // Lay out every glyph first so the row can be centered as a whole.
    let segs: Vec<_> = hints
        .iter()
        .map(|(key, label)| {
            let kg = painter.layout_no_wrap(key.to_string(), key_font.clone(), INK);
            let lg = painter.layout_no_wrap(label.to_string(), label_font.clone(), MUTED);
            let pill_w = kg.size().x + PAD * 2.0;
            let seg_w = pill_w + GAP_KL + lg.size().x;
            (kg, lg, pill_w, seg_w)
        })
        .collect();
    let total: f32 = segs.iter().map(|s| s.3).sum::<f32>() + GAP_SEG * (segs.len() - 1) as f32;

    let cy = area.bottom() - HINT_BASE;
    let mut x = area.center().x - total / 2.0;
    for (kg, lg, pill_w, seg_w) in segs {
        let pill = egui::Rect::from_min_size(
            egui::pos2(x, cy - HINT_PILL_H / 2.0),
            egui::vec2(pill_w, HINT_PILL_H),
        );
        painter.rect_filled(pill, 5.0, SURFACE);
        painter.rect_stroke(
            pill,
            5.0,
            egui::Stroke::new(1.0, BORDER),
            egui::StrokeKind::Inside,
        );
        painter.galley(pill.center() - kg.size() / 2.0, kg, INK);
        painter.galley(
            egui::pos2(x + pill_w + GAP_KL, cy - lg.size().y / 2.0),
            lg,
            MUTED,
        );
        x += seg_w + GAP_SEG;
    }
}

/// The brand wordmark: "ret" in ink, "surf" in the brand gradient. egui has no
/// gradient text fill, so the `surf` glyphs are tagged with a marker color
/// ([`ACCENT`]), tessellated here, and recolored by height.
fn wordmark_job() -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    // Keep the wordmark on Hack (monospace) for its logo feel, independent of the
    // body `font()` (Ubuntu-Light).
    let fmt = |color: egui::Color32| egui::TextFormat {
        font_id: egui::FontId::monospace(WORDMARK_SIZE),
        color,
        extra_letter_spacing: WORDMARK_TRACKING,
        ..Default::default()
    };
    job.append("ret", 0.0, fmt(INK));
    // epaint skips extra_letter_spacing on a section's first glyph, so the
    // ret/surf joint needs it back. ACCENT is a marker recolored below.
    job.append("surf", WORDMARK_TRACKING, fmt(ACCENT));
    job
}

/// The mark's laid-out height, wave band included — the layout needs it before
/// anything is drawn.
fn wordmark_height(ui: &egui::Ui) -> f32 {
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(wordmark_job()));
    galley.size().y + WAVE_BAND
}

fn add_wordmark(ui: &mut egui::Ui) {
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(wordmark_job()));
    let gsize = galley.size();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(gsize.x, gsize.y + WAVE_BAND),
        egui::Sense::hover(),
    );

    // Recolor the marker-colored `surf` vertices by height: teal over the lower
    // ~55%, warming to coral — the stops of `_surf_gradient` in the brand SVGs.
    let ppp = ui.ctx().pixels_per_point();
    let tex = ui.ctx().fonts(|f| f.font_image_size());
    let mut mesh = egui::epaint::Mesh::default();
    let shape = egui::epaint::TextShape::new(rect.min, galley, INK);
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
        let t = ((frac - 0.55) / 0.45).clamp(0.0, 1.0);
        v.color = lerp_color(ACCENT, SURF_WARM, t);
    }
    ui.painter().add(egui::Shape::mesh(mesh));

    // The brand wave: a solid-teal sine under the text, ~3 periods, starting on
    // the centerline and dipping down first — the SVG path's phase.
    const N: usize = 120;
    const PERIODS: f32 = 3.0;
    let over = gsize.x * 0.02;
    let (x0, x1) = (rect.min.x - over, rect.max.x + over);
    let cy = rect.min.y + gsize.y + WAVE_GAP;
    let pts: Vec<egui::Pos2> = (0..=N)
        .map(|i| {
            let t = i as f32 / N as f32;
            let y = cy + WAVE_AMP * (t * PERIODS * std::f32::consts::TAU).sin();
            egui::pos2(x0 + (x1 - x0) * t, y)
        })
        .collect();
    ui.painter().add(egui::Shape::line(
        pts,
        egui::Stroke::new(WAVE_STROKE, ACCENT),
    ));
}

/// Component-wise sRGB lerp. Fine for a subtle brand tint (no need for linear space).
fn lerp_color(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    egui::Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// The hero search / URL field. Editable directly (desktop keyboard); on the
/// handheld the OSK writes into the same buffer. Enter submits it (handled in
/// the keyboard/router layer).
fn add_search(ui: &mut egui::Ui, home: &mut Home, width: f32, osk_caret: Option<OskCaret>) {
    let selected = home.search_focused();
    let edit_id = egui::Id::new(super::ids::HOME_SEARCH);
    // While the OSK types here, mirror its caret (egui won't follow the external
    // edit on its own); desktop editing is left untouched.
    if let Some(pos) = osk_caret {
        super::park_caret(ui.ctx(), edit_id, pos, home.input().chars().count());
    }
    let frame = egui::Frame::default()
        .fill(SURFACE)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .corner_radius(10.0)
        .stroke(egui::Stroke::new(
            if selected { 2.0 } else { 1.0 },
            if selected { ACCENT } else { BORDER },
        ));
    frame.show(ui, |ui| {
        ui.set_width(width);
        // One binding for both, or the hint renders smaller than what is typed.
        let field_font = font(20.0);
        let edit = egui::TextEdit::singleline(home.input_mut())
            .id(edit_id)
            .hint_text(egui::RichText::new("Search or enter address").font(field_font.clone()))
            .frame(egui::Frame::NONE)
            .background_color(egui::Color32::TRANSPARENT)
            .desired_width(f32::INFINITY)
            .font(field_font)
            // egui puts the text at the top; the frame's margins make the box taller.
            .vertical_align(egui::Align::Center)
            .text_color(INK);
        let resp = ui.add(edit);
        // Clicking the field selects it (so the highlight follows the mouse).
        if resp.gained_focus() {
            home.focus_search();
        }
        // Keep egui focus in sync with the selection, so a keyboard types at once
        // and the arrows navigate again once the selection leaves the field.
        if home.search_focused() {
            // Not on Android, where it pops the soft keyboard on every visit; and
            // only if nothing else holds focus, or the address bar goes unclickable.
            #[cfg(not(target_os = "android"))]
            if ui.ctx().memory(|m| m.focused()).is_none() {
                resp.request_focus();
            }
        } else if resp.has_focus() {
            resp.surrender_focus();
        }
    });
}

/// Grid slots for `pins`: one per pin plus the trailing "Edit" tile. Shared with
/// the navigation side ([`crate::ui::AppUi`]) so paint and nav can't drift.
pub(super) fn slot_count(pins: &[String]) -> usize {
    pins.len() + 1
}

/// The speed-dial grid: one tile per pinned shortcut (the brand initial over its
/// name), followed by a trailing "Edit" tile that opens the speed-dial editor.
fn add_dial(
    ui: &mut egui::Ui,
    home: &Home,
    pins: &[String],
    width: f32,
    cols: usize,
    commands: &mut Vec<AppCommand>,
) {
    tile_grid(ui, width, cols, slot_count(pins), |ui, i| {
        let selected = home.tile() == Some(i);
        match pins.get(i) {
            Some(url) => {
                if add_tile(ui, url, selected).clicked() {
                    commands.push(AppCommand::Menu(MenuAction::OpenUrl(url.clone())));
                }
            }
            // i == pins.len(): the trailing "Edit" tile.
            None => {
                if add_edit_tile(ui, selected).clicked() {
                    commands.push(AppCommand::Menu(MenuAction::DialEdit));
                }
            }
        }
    });
}

/// Lay out `total` tiles in rows of `cols` centred within `width`. Each row is
/// allocated at its exact content width so a partial last row stays centred — a
/// plain `ui.horizontal` would take the full width and left-align.
pub(super) fn tile_grid(
    ui: &mut egui::Ui,
    width: f32,
    cols: usize,
    total: usize,
    mut tile: impl FnMut(&mut egui::Ui, usize),
) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            for (row, row_start) in (0..total).step_by(cols).enumerate() {
                // Between rows only: a trailing gap would pad the grid's foot.
                if row > 0 {
                    ui.add_space(GAP);
                }
                let n = (total - row_start).min(cols);
                let row_w = n as f32 * TILE_W + (n.saturating_sub(1)) as f32 * GAP;
                ui.allocate_ui_with_layout(
                    egui::vec2(row_w, TILE_H),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = GAP;
                        for slot in row_start..row_start + n {
                            tile(ui, slot);
                        }
                    },
                );
            }
        },
    );
}

/// Glyph-square side length within a tile.
pub(super) const GLYPH: f32 = 52.0;

/// One speed-dial tile: a rounded "glyph" square holding the brand initial, with
/// the brand name beneath it. Custom-painted (not a Button) for the two-tier
/// look.
fn add_tile(ui: &mut egui::Ui, url: &str, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(TILE_W, TILE_H), egui::Sense::click());
    keep_visible(ui, &resp, rect, selected);
    paint_tile(ui.painter(), rect, url, selected || resp.hovered());
    resp
}

/// Scroll a selected tile into view on the frame the selection reaches it. Going
/// by visibility alone would fight a manual scroll, re-centring every frame.
/// No-op outside a scroll area, where the clip rect is the whole page.
fn keep_visible(ui: &egui::Ui, resp: &egui::Response, rect: egui::Rect, selected: bool) {
    let key = egui::Id::new(("dial_tile", resp.layer_id));
    if !selected || ui.ctx().data(|d| d.get_temp::<egui::Id>(key)) == Some(resp.id) {
        return;
    }
    ui.ctx().data_mut(|d| d.insert_temp(key, resp.id));
    if !ui.clip_rect().contains_rect(rect) {
        ui.scroll_to_rect(rect, None);
    }
}

/// Paint a speed-dial tile's visuals into `rect`; the caller owns the click
/// region and any extra overlay.
pub(super) fn paint_tile(painter: &egui::Painter, rect: egui::Rect, url: &str, active: bool) {
    let glyph = paint_glyph_square(painter, rect, active, Some(SURFACE));

    // The settings sentinel isn't a real address: show a gear glyph and "Settings"
    // rather than the garbage `brand_label` would derive from `retsurf:settings`.
    let (glyph_text, name) = if url == SETTINGS_PIN {
        (bold::GEAR.to_string(), "Settings".to_string())
    } else {
        let label = brand_label(url);
        let initial = label
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default();
        (initial, label)
    };
    painter.text(
        glyph.center(),
        egui::Align2::CENTER_CENTER,
        glyph_text,
        font(22.0),
        INK,
    );

    // Brand name under the glyph (truncated so a long name can't overflow).
    painter.text(
        egui::pos2(rect.center().x, glyph.bottom() + 14.0),
        egui::Align2::CENTER_CENTER,
        truncate(&name, 12),
        font(12.0),
        if active { INK } else { MUTED },
    );
}

/// The rounded glyph square at a tile's top. `None` leaves it unfilled, for an
/// action slot's outline.
fn paint_glyph_square(
    painter: &egui::Painter,
    rect: egui::Rect,
    active: bool,
    fill: Option<egui::Color32>,
) -> egui::Rect {
    let glyph = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + GLYPH / 2.0 + 2.0),
        egui::vec2(GLYPH, GLYPH),
    );
    if let Some(fill) = fill {
        painter.rect_filled(glyph, 12.0, fill);
    }
    painter.rect_stroke(
        glyph,
        12.0,
        egui::Stroke::new(
            if active { 2.0 } else { 1.0 },
            if active { ACCENT } else { BORDER },
        ),
        egui::StrokeKind::Inside,
    );
    glyph
}

/// An action slot: an unfilled glyph square holding `icon`, captioned `label`.
pub(super) fn paint_action_tile(
    painter: &egui::Painter,
    rect: egui::Rect,
    icon: &str,
    label: &str,
    active: bool,
) {
    let glyph = paint_glyph_square(painter, rect, active, None);
    painter.text(
        glyph.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        font(24.0),
        if active { ACCENT } else { MUTED },
    );
    painter.text(
        egui::pos2(rect.center().x, glyph.bottom() + 14.0),
        egui::Align2::CENTER_CENTER,
        label,
        font(12.0),
        if active { INK } else { MUTED },
    );
}

/// The trailing "Edit" tile: an action slot holding a pencil. Opens the editor.
fn add_edit_tile(ui: &mut egui::Ui, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(TILE_W, TILE_H), egui::Sense::click());
    keep_visible(ui, &resp, rect, selected);
    let active = selected || resp.hovered();
    paint_action_tile(ui.painter(), rect, bold::PENCIL_SIMPLE, "Edit", active);
    resp
}

/// Trim a label to `max` characters, appending `…` when shortened.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    }
}

thread_local! {
    /// Memoizes [`brand_label`] per URL: it parses a `Url`, and runs for every
    /// visible tile on each home repaint. Single-threaded — egui is main-thread.
    static BRAND_LABELS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// A short label for a tile or a menu URL row (memoized wrapper over
/// [`compute_brand_label`]).
pub(super) fn brand_label(url: &str) -> String {
    BRAND_LABELS.with(|cache| {
        if let Some(label) = cache.borrow().get(url) {
            return label.clone();
        }
        let label = compute_brand_label(url);
        cache.borrow_mut().insert(url.to_owned(), label.clone());
        label
    })
}

/// A short label for a tile: the registrable domain name (`en.wikipedia.org` ->
/// `wikipedia`, `bbc.co.uk` -> `bbc`), falling back to the host, then the raw string.
fn compute_brand_label(url: &str) -> String {
    let Ok(parsed) = url::Url::parse(url) else {
        return url.to_string();
    };
    let Some(host) = parsed.host() else {
        return url.to_string();
    };
    // An address has no registrable domain to shorten to — splitting on dots
    // would label `127.0.0.1` as "0".
    if !matches!(host, url::Host::Domain(_)) {
        return host.to_string();
    }
    let host = host.to_string();
    let host = host.trim_start_matches("www.");
    let parts: Vec<&str> = host.split('.').filter(|s| !s.is_empty()).collect();
    let n = parts.len();
    if n <= 1 {
        return host.to_string();
    }
    let suffix_len = if n >= 3 && parts[n - 2].len() <= 3 && parts[n - 1].len() == 2 {
        2
    } else {
        1
    };
    parts[n - suffix_len - 1].to_string()
}

#[cfg(test)]
mod tests {
    use super::compute_brand_label;

    #[test]
    fn brand_label_takes_the_registrable_domain() {
        assert_eq!(
            compute_brand_label("https://en.wikipedia.org/wiki/X"),
            "wikipedia"
        );
        assert_eq!(compute_brand_label("https://www.google.com/"), "google");
        assert_eq!(compute_brand_label("https://bbc.co.uk"), "bbc");
        assert_eq!(compute_brand_label("http://localhost:8099/x"), "localhost");
    }

    /// An address has no registrable domain; splitting on dots used to yield "0".
    #[test]
    fn brand_label_keeps_addresses_whole() {
        assert_eq!(compute_brand_label("http://127.0.0.1:8099/x"), "127.0.0.1");
        assert_eq!(compute_brand_label("http://[::1]:8099/x"), "[::1]");
    }

    /// The internal pages (and anything unparseable) fall back to the raw string.
    #[test]
    fn brand_label_falls_back_to_the_input() {
        assert_eq!(compute_brand_label("retsurf:home"), "retsurf:home");
        assert_eq!(compute_brand_label("not a url"), "not a url");
    }
}
