//! Rendering of the built-in start page overlay (state lives in
//! [`crate::overlay::home`]): a brand mark (the banner scene, the wordmark, or
//! none), a bar holding the search / URL field, a tab bar of filters, a grid
//! of the bookmarks and a footer naming what the buttons do.
//! Navigation is routed by [`crate::app`]; tiles open via [`HomeAction::Open`].

use super::favicon::{Icon, PageIcons};
use super::panel::ROW_RADIUS;
use super::theme::{lerp_color, ACCENT, BG, BORDER, INK, MUTED, ROW_FONT, SURFACE, SURF_WARM};
use super::OskCaret;
use crate::command::{AppCommand, HomeAction, QuickAccessAction};
use crate::config::HomeSort;
use crate::data::bookmarks::{Bookmark, Filter, Launch};
use crate::overlay::home::Home;
use crate::overlay::quick_access::Strip;
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

/// The banner scene without its plate; the start page supplies the background.
static BANNER_SVG: &[u8] = include_bytes!("../../resources/retsurf-banner-bare.svg");
/// The banner's drawn height (logical px); its width follows the SVG's aspect.
const BANNER_H: f32 = 100.0;
/// Banner bottom to the search field; the scene's own sea is its margin.
const BANNER_GAP: f32 = 12.0;
/// The search field's top with no mark above it (logical px).
const COMPACT_TOP: f32 = 16.0;

/// What heads the start page.
#[derive(Clone, Copy)]
pub(super) enum Mark<'a> {
    Banner(&'a egui::TextureHandle),
    Wordmark,
    None,
}

/// Everything the start page shows besides its own state.
pub(super) struct HomeView<'a> {
    pub tiles: Tiles<'a>,
    pub mark: Mark<'a>,
    /// Whether the search field shows.
    pub search: bool,
    pub tabs: Tabs<'a>,
    /// Whether the page carries the toolbar's strip buttons, for a page
    /// without a toolbar.
    pub strip_buttons: bool,
    /// The footer's hints; empty draws no footer.
    pub footer: &'a [FooterHint],
}

/// The tab bar: the filters offered and the one shown.
#[derive(Clone)]
pub(super) struct Tabs<'a> {
    pub filters: &'a [Filter],
    pub filter: Filter,
    /// Whether the shoulders that step them are named either side.
    pub shoulders: bool,
}

/// One footer hint: the buttons that do it, what it does, and what a click on
/// it does.
pub(super) struct FooterHint {
    pub caps: Vec<String>,
    pub label: String,
    pub click: Option<HomeAction>,
}

/// The banner rasterized for the current pixel density. It is drawn at one
/// logical size, so a new rasterization is only needed when the scale changes.
#[derive(Default)]
pub(super) struct Banner {
    texture: Option<(f32, egui::TextureHandle)>,
    /// The SVG would not rasterize; not retried every frame.
    failed: bool,
}

impl Banner {
    pub fn texture(&mut self, ctx: &egui::Context) -> Option<&egui::TextureHandle> {
        let ppp = ctx.pixels_per_point();
        if self.failed {
            return None;
        }
        if self.texture.as_ref().is_none_or(|(at, _)| *at != ppp) {
            let Some(image) = rasterize_banner((BANNER_H * ppp).round() as u32) else {
                log::warn!("start page: the banner SVG did not rasterize");
                self.failed = true;
                return None;
            };
            let texture = ctx.load_texture("home-banner", image, egui::TextureOptions::LINEAR);
            self.texture = Some((ppp, texture));
        }
        self.texture.as_ref().map(|(_, t)| t)
    }
}

/// [`BANNER_SVG`] at `height` px, its width to the SVG's aspect.
fn rasterize_banner(height: u32) -> Option<egui::ColorImage> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_data(BANNER_SVG, &usvg::Options::default()).ok()?;
    let size = tree.size();
    let scale = height as f32 / size.height();
    let width = (size.width() * scale).round() as u32;
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Some(egui::ColorImage::from_rgba_premultiplied(
        [width as usize, height as usize],
        pixmap.data(),
    ))
}

/// The bookmarks with the site icons to draw them by.
#[derive(Clone, Copy)]
pub(super) struct Tiles<'a> {
    pub bookmarks: &'a [Bookmark],
    /// Indices into `bookmarks` of the tiles shown, in their order.
    pub visible: &'a [usize],
    pub icons: &'a PageIcons,
}

/// Tile footprint (logical px) and grid spacing.
pub(super) const TILE_W: f32 = 96.0;
pub(super) const TILE_H: f32 = 84.0;
pub(super) const GAP: f32 = 12.0;

/// Wordmark type size (logical px); the wave band and the reserved height
/// derive from it.
const WORDMARK_SIZE: f32 = 36.0;
/// Letter spacing as a share of the type size, matching the SVG wordmark.
const WORDMARK_TRACKING: f32 = 0.1;
/// The wave band below the text. Ratios track the SVG, nudged up so the thin
/// stroke stays legible on a handheld.
const WAVE_GAP: f32 = WORDMARK_SIZE * 0.2; // text bottom to wave centerline
const WAVE_AMP: f32 = WORDMARK_SIZE * 0.08; // crest height
const WAVE_STROKE: f32 = WORDMARK_SIZE * 0.045;
const WAVE_BAND: f32 = WAVE_GAP + WAVE_AMP + WAVE_STROKE;

/// Clearance between the page's foot and the scrolling grid.
const FOOT: f32 = 12.0;

/// Draw the start-page overlay over the (blank) web view, confined to the
/// `webview` rect so the toolbar stays usable. Any activation is pushed as a
/// command for the app to execute.
pub(super) fn add_home(
    ctx: &egui::Context,
    home: &mut Home,
    view: HomeView,
    webview: egui::Rect,
    osk_caret: Option<OskCaret>,
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
                    const GAP_TOP_RATIO: f32 = 0.6; // mark to field, of the mark's own height

                    // The grid scrolls in what is left, so a long one can't hide
                    // rows off the page.
                    // Last frame's height: the footer is laid out after the grid.
                    let footer_id = egui::Id::new("home_footer_h");
                    let footer_h = match view.footer.is_empty() {
                        true => 0.0,
                        false => ui
                            .ctx()
                            .data(|d| d.get_temp(footer_id))
                            .unwrap_or(FOOTER_LINE_H),
                    };
                    let floor = area.bottom() - FOOT - footer_h - FOOTER_GAP;
                    let banner_size = |t: &egui::TextureHandle| {
                        let size = t.size_vec2() * (BANNER_H / t.size_vec2().y);
                        size * (area.width() / size.x).min(1.0)
                    };
                    let (mark_h, gap_top) = match view.mark {
                        Mark::Banner(t) => (banner_size(t).y, BANNER_GAP),
                        Mark::Wordmark => {
                            let h = wordmark_height(ui);
                            (h, h * GAP_TOP_RATIO)
                        }
                        Mark::None => (0.0, 0.0),
                    };
                    let top = match view.mark {
                        Mark::None => COMPACT_TOP,
                        Mark::Banner(_) | Mark::Wordmark => {
                            ((floor - area.top()) * FIELD_ANCHOR - mark_h - gap_top).max(8.0)
                        }
                    };

                    ui.vertical_centered(|ui| {
                        ui.add_space(top);
                        match view.mark {
                            Mark::Banner(t) => {
                                ui.add(egui::Image::new(t).fit_to_exact_size(banner_size(t)));
                            }
                            Mark::Wordmark => add_wordmark(ui),
                            Mark::None => {}
                        }
                        ui.add_space(gap_top);
                        let buttons = view.strip_buttons;
                        if view.search {
                            add_search_bar(ui, home, block_w, osk_caret, buttons, commands);
                            ui.add_space(ROW_GAP);
                        }
                        let tab_buttons = buttons && !view.search;
                        if view.tabs.filters.len() > 1 || tab_buttons {
                            add_tabs(ui, &view.tabs, block_w, tab_buttons, commands);
                            ui.add_space(ROW_GAP);
                        }
                        let rest = (floor - ui.cursor().top()).max(TILE_H);
                        // The viewport spans the page and the grid centers inside
                        // it; shrunk to fit, the scrollbar would push it off-center.
                        egui::ScrollArea::vertical()
                            .max_height(rest)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.vertical_centered(|ui| {
                                    add_grid(ui, home, view.tiles, block_w, cols, commands);
                                });
                            });
                    });
                    if !view.footer.is_empty() {
                        let left = area.center().x - block_w / 2.0;
                        let top = area.bottom() - FOOT - footer_h;
                        let row = egui::Rect::from_min_max(
                            egui::pos2(left, top),
                            egui::pos2(left + block_w, area.bottom()),
                        );
                        let h = add_footer(ui, row, view.footer, commands);
                        if (h - footer_h).abs() > FOOTER_SETTLE {
                            ui.ctx().data_mut(|d| d.insert_temp(footer_id, h));
                            ui.ctx().request_repaint();
                        }
                    }
                });
        });
}

/// The footer's height before it is first measured: one line and its rule.
const FOOTER_LINE_H: f32 = 32.0;
/// Grid to footer.
const FOOTER_GAP: f32 = 8.0;
/// The footer's rule to its hints.
const FOOTER_RULE_GAP: f32 = 8.0;
/// A change in the footer's height smaller than this is no change.
const FOOTER_SETTLE: f32 = 0.5;
/// Space between the page's rows.
const ROW_GAP: f32 = 12.0;
/// The search bar's height.
const BAR_H: f32 = 44.0;
/// The search bar's inner margin across.
const BAR_MARGIN_X: i8 = 6;
/// The search bar's widest stroke.
const BAR_STROKE: f32 = 2.0;
/// A strip button's side, inside the bar or at the tab bar's ends.
const STRIP_BUTTON: f32 = 32.0;
/// The bar's thin lines: its idle stroke and its dividers.
const LINE_W: f32 = 1.0;
/// A divider's height.
const DIVIDER_H: f32 = 22.0;
/// The field's own type size, its hint's too.
const FIELD_FONT: f32 = 20.0;
/// The magnifier's size, as a share of the field's type.
const GLASS_SHARE: f32 = 0.8;
/// A tab's height.
const TAB_H: f32 = 28.0;
/// Space between tabs.
const TAB_GAP: f32 = 6.0;

/// The footer in `row`: a rule, then its hints centred; returns its height.
fn add_footer(
    ui: &mut egui::Ui,
    row: egui::Rect,
    footer: &[FooterHint],
    commands: &mut Vec<AppCommand>,
) -> f32 {
    ui.painter()
        .hline(row.x_range(), row.top(), egui::Stroke::new(LINE_W, BORDER));
    let keys: Vec<Vec<&str>> = footer
        .iter()
        .map(|hint| hint.caps.iter().map(String::as_str).collect())
        .collect();
    let hints: Vec<super::keys::Hint> = keys
        .iter()
        .zip(footer)
        .map(|(caps, hint)| (caps.as_slice(), hint.label.as_str()))
        .collect();
    let width = row.width();
    let inner = egui::Rect::from_min_max(row.min + egui::vec2(0.0, FOOTER_RULE_GAP), row.max);
    let (responses, h) = ui
        .scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            super::keys::centered_hints(ui, &hints, width)
        })
        .inner;
    for (resp, hint) in responses.iter().zip(footer) {
        if let Some(action) = &hint.click {
            if resp.clicked() {
                commands.push(AppCommand::Home(action.clone()));
            }
        }
    }
    h + FOOTER_RULE_GAP
}

/// The search bar: the field, between the toolbar's strip buttons when
/// `buttons` (Quick Menu at the left, Quick Access at the right, as the toolbar
/// has them), all in one frame that lights while the field is selected.
fn add_search_bar(
    ui: &mut egui::Ui,
    home: &mut Home,
    width: f32,
    osk_caret: Option<OskCaret>,
    buttons: bool,
    commands: &mut Vec<AppCommand>,
) {
    let selected = home.search_focused();
    let frame = egui::Frame::default()
        .fill(SURFACE)
        .inner_margin(egui::Margin::symmetric(BAR_MARGIN_X, 0))
        .corner_radius(10.0)
        .stroke(egui::Stroke::new(
            if selected { BAR_STROKE } else { LINE_W },
            if selected { ACCENT } else { BORDER },
        ));
    let inner_w = width - 2.0 * (BAR_MARGIN_X as f32 + BAR_STROKE);
    frame.show(ui, |ui| {
        ui.set_width(inner_w);
        ui.set_height(BAR_H);
        ui.horizontal_centered(|ui| {
            if buttons {
                strip_button(ui, Strip::QuickMenu, commands);
                divider(ui);
            }
            let glass = egui::RichText::new(bold::MAGNIFYING_GLASS)
                .font(font(FIELD_FONT * GLASS_SHARE))
                .color(MUTED);
            ui.label(glass);
            let tail = match buttons {
                true => STRIP_BUTTON + LINE_W + 2.0 * ui.spacing().item_spacing.x,
                false => 0.0,
            };
            let field_w = (ui.available_width() - tail).max(0.0);
            add_search(ui, home, field_w, osk_caret);
            if buttons {
                divider(ui);
                strip_button(ui, Strip::QuickAccess, commands);
            }
        });
    });
}

/// A thin upright rule between the bar's parts.
fn divider(ui: &mut egui::Ui) {
    let size = egui::vec2(LINE_W, DIVIDER_H);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let stroke = egui::Stroke::new(LINE_W, BORDER);
    ui.painter().vline(rect.center().x, rect.y_range(), stroke);
}

/// The frameless button that opens `strip`, lit when hovered.
fn strip_button(ui: &mut egui::Ui, strip: Strip, commands: &mut Vec<AppCommand>) {
    let icon = match strip {
        Strip::QuickMenu => bold::LIST,
        Strip::QuickAccess | Strip::Options => bold::DOTS_THREE,
    };
    let size = egui::vec2(STRIP_BUTTON, STRIP_BUTTON);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let color = if resp.hovered() { ACCENT } else { INK };
    let center = egui::Align2::CENTER_CENTER;
    ui.painter()
        .text(rect.center(), center, icon, font(FIELD_FONT), color);
    if resp.clicked() {
        commands.push(AppCommand::QuickAccess(QuickAccessAction::Toggle(strip)));
    }
}

/// The brand wordmark: "ret" in ink, "surf" in the brand gradient. egui has no
/// gradient text fill, so the `surf` glyphs are tagged with a marker color
/// ([`ACCENT`]), tessellated here, and recolored by height.
fn wordmark_job(size: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    // Keep the wordmark on Hack (monospace) for its logo feel, independent of the
    // body `font()` (Ubuntu-Light).
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

/// The mark's laid-out height, wave band included — the layout needs it before
/// anything is drawn.
fn wordmark_height(ui: &egui::Ui) -> f32 {
    wordmark_text_size(ui, WORDMARK_SIZE).y + WAVE_BAND
}

fn add_wordmark(ui: &mut egui::Ui) {
    let gsize = wordmark_text_size(ui, WORDMARK_SIZE);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(gsize.x, gsize.y + WAVE_BAND),
        egui::Sense::hover(),
    );
    paint_wordmark_text(ui, rect.min, WORDMARK_SIZE);

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
    // ~55%, warming to coral — the stops of `_surf_gradient` in the brand SVGs.
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
        let t = ((frac - 0.55) / 0.45).clamp(0.0, 1.0);
        v.color = lerp_color(ACCENT, SURF_WARM, t);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// The search / URL field, frameless inside the bar, `width` wide. Editable
/// directly (desktop keyboard); on the handheld the OSK writes into the same
/// buffer. Enter submits it (handled in the keyboard/router layer).
fn add_search(ui: &mut egui::Ui, home: &mut Home, width: f32, osk_caret: Option<OskCaret>) {
    let edit_id = egui::Id::new(super::ids::HOME_SEARCH);
    // While the OSK types here, mirror its caret (egui won't follow the external
    // edit on its own); desktop editing is left untouched.
    if let Some(pos) = osk_caret {
        super::park_caret(ui.ctx(), edit_id, pos, home.input().chars().count());
    }
    // One binding for both, or the hint renders smaller than what is typed.
    let field_font = font(FIELD_FONT);
    let edit = egui::TextEdit::singleline(home.input_mut())
        .id(edit_id)
        .hint_text(egui::RichText::new("Search or enter address").font(field_font.clone()))
        .frame(egui::Frame::NONE)
        .background_color(egui::Color32::TRANSPARENT)
        .desired_width(width)
        .font(field_font)
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
}

/// The launch mark's badge.
const MARK_RADIUS: f32 = 10.0;
/// How far the badge's center sits inside the glyph square's corner.
const MARK_INSET: f32 = 4.0;

/// The tab bar: the filter tabs between the shoulders that step them; with
/// `buttons`, the strip buttons at either end.
fn add_tabs(
    ui: &mut egui::Ui,
    tabs: &Tabs,
    width: f32,
    buttons: bool,
    commands: &mut Vec<AppCommand>,
) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, TAB_H),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = TAB_GAP;
            if buttons {
                strip_button(ui, Strip::QuickAccess, commands);
            }
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), TAB_H),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = TAB_GAP;
                    if buttons {
                        strip_button(ui, Strip::QuickMenu, commands);
                    }
                    if tabs.filters.len() > 1 {
                        add_filter_tabs(ui, tabs, commands);
                    }
                },
            );
        },
    );
}

/// The shoulders and the filter tabs between them, in what is left of the row:
/// centred when they fit, else the tabs scrolled to keep the active one in
/// view when it changes.
fn add_filter_tabs(ui: &mut egui::Ui, tabs: &Tabs, commands: &mut Vec<AppCommand>) {
    use super::keys::{cap_size, key_cap, HINT_FONT, SHOULDERS};
    let pad = 2.0 * ui.spacing().button_padding.x;
    let tabs_w: f32 = tabs
        .filters
        .iter()
        .map(|f| tab_text_width(ui, &f.label()) + pad + TAB_GAP)
        .sum::<f32>()
        - TAB_GAP;
    let caps_w: f32 = match tabs.shoulders {
        true => SHOULDERS
            .iter()
            .map(|s| cap_size(ui, s, HINT_FONT).x + TAB_GAP)
            .sum(),
        false => 0.0,
    };
    let shoulder = |ui: &mut egui::Ui, at: usize| {
        if tabs.shoulders {
            key_cap(ui, SHOULDERS[at], HINT_FONT);
        }
    };
    let room = ui.available_width();
    let shown = egui::Id::new("home_tabs_active");
    let token = tabs.filter.token();
    let moved = ui.ctx().data(|d| d.get_temp::<String>(shown)) != Some(token.clone());
    let add = |ui: &mut egui::Ui, commands: &mut Vec<AppCommand>| {
        for filter in tabs.filters {
            let active = *filter == tabs.filter;
            let resp = tab(ui, &filter.label(), active);
            if active && moved {
                resp.scroll_to_me(None);
            }
            if resp.clicked() {
                commands.push(AppCommand::Home(HomeAction::SetFilter(filter.token())));
            }
        }
    };
    if tabs_w + caps_w <= room {
        ui.add_space((room - tabs_w - caps_w) / 2.0);
        shoulder(ui, 0);
        add(ui, commands);
        shoulder(ui, 1);
    } else {
        shoulder(ui, 0);
        egui::ScrollArea::horizontal()
            .id_salt("home_tabs")
            .max_width(room - caps_w)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = TAB_GAP;
                    add(ui, commands);
                });
            });
        shoulder(ui, 1);
    }
    if moved {
        ui.ctx().data_mut(|d| d.insert_temp(shown, token));
    }
}

/// A sort's display name.
pub(super) fn sort_label(sort: HomeSort) -> &'static str {
    HomeSort::CHOICES
        .iter()
        .find(|(_, token)| *token == sort.as_str())
        .map_or("", |(label, _)| label)
}

/// One tab: white type, the active one filled.
fn tab(ui: &mut egui::Ui, text: &str, on: bool) -> egui::Response {
    let text = egui::RichText::new(text)
        .color(egui::Color32::WHITE)
        .size(ROW_FONT);
    ui.add(
        egui::Button::selectable(on, text)
            .corner_radius(ROW_RADIUS)
            .min_size(egui::vec2(0.0, TAB_H)),
    )
}

fn tab_text_width(ui: &egui::Ui, text: &str) -> f32 {
    let font = egui::FontId::proportional(ROW_FONT);
    ui.fonts_mut(|f| f.layout_no_wrap(text.to_string(), font, INK).size().x)
}

/// Shown in place of an empty grid.
const EMPTY_HINT: &str = "Bookmark a page from Quick Access to see it here";

/// The bookmark grid: one tile per bookmark the view shows, in its order.
fn add_grid(
    ui: &mut egui::Ui,
    home: &Home,
    tiles: Tiles,
    width: f32,
    cols: usize,
    commands: &mut Vec<AppCommand>,
) {
    if tiles.visible.is_empty() {
        ui.label(
            egui::RichText::new(EMPTY_HINT)
                .font(font(14.0))
                .color(MUTED),
        );
        return;
    }
    tile_grid(ui, width, cols, tiles.visible.len(), |ui, i| {
        let bookmark = &tiles.bookmarks[tiles.visible[i]];
        let selected = home.tile() == Some(i);
        let icon = tiles.icons.icon(&bookmark.url);
        let resp = add_tile(ui, bookmark, icon, selected);
        if resp.clicked() {
            commands.push(AppCommand::Home(HomeAction::Open(i)));
        }
        if resp.secondary_clicked() {
            commands.push(AppCommand::Home(HomeAction::Options(i)));
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
/// A site icon inside the glyph square: near the 32 px cache, so it stays sharp.
const DIAL_ICON: f32 = 34.0;
/// How much of an icon's tint colours its glyph square over [`SURFACE`].
const TINT_MIX: f32 = 0.35;
/// Saturation and value of an iconless tile's fill; its hue comes from the label.
const LETTER_SAT: f32 = 0.6;
const LETTER_VAL: f32 = 0.42;

/// One speed-dial tile: a rounded "glyph" square holding the brand initial, with
/// the brand name beneath it. Custom-painted (not a Button) for the two-tier
/// look.
fn add_tile(
    ui: &mut egui::Ui,
    bookmark: &Bookmark,
    icon: Option<Icon>,
    selected: bool,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(TILE_W, TILE_H), egui::Sense::click());
    keep_visible(ui, &resp, rect, selected);
    let label = bookmark_label(bookmark);
    let active = selected || resp.hovered();
    let glyph = paint_tile(ui.painter(), rect, &label, icon, active);
    if let Some(mark) = launch_mark(bookmark.launch) {
        paint_launch_mark(ui.painter(), glyph, mark, active);
    }
    resp
}

/// What a bookmark is shown as: its name, else its site.
pub fn bookmark_label(bookmark: &Bookmark) -> String {
    match bookmark.name.is_empty() {
        true => brand_label(&bookmark.url),
        false => bookmark.name.clone(),
    }
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

/// Paint a tile's visuals into `rect`: the site `icon` on a square tinted
/// after it, else the label's initial; returns the square. The caller owns the
/// click region.
fn paint_tile(
    painter: &egui::Painter,
    rect: egui::Rect,
    label: &str,
    icon: Option<Icon>,
    active: bool,
) -> egui::Rect {
    let fill = match icon {
        Some(icon) if icon.dark => INK,
        Some(icon) => SURFACE.lerp_to_gamma(icon.tint, TINT_MIX),
        None => letter_fill(label),
    };
    let glyph = paint_glyph_square(painter, rect, active, fill);
    match icon {
        Some(Icon { bare: texture, .. }) => {
            let fit = DIAL_ICON / texture.size_vec2().max_elem();
            let at = egui::Rect::from_center_size(glyph.center(), texture.size_vec2() * fit);
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(texture.id(), at, uv, egui::Color32::WHITE);
        }
        None => {
            let initial = label
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_default();
            painter.text(
                glyph.center(),
                egui::Align2::CENTER_CENTER,
                initial,
                font(22.0),
                INK,
            );
        }
    }

    // The label under the glyph, truncated so a long one can't overflow.
    painter.text(
        egui::pos2(rect.center().x, glyph.bottom() + 14.0),
        egui::Align2::CENTER_CENTER,
        truncate(label, 12),
        font(12.0),
        if active { INK } else { MUTED },
    );
    glyph
}

/// The corner mark of a bookmark that opens as a program; `None` for a site.
fn launch_mark(launch: Launch) -> Option<&'static str> {
    match launch {
        Launch::Browser => None,
        Launch::App => Some(bold::APP_WINDOW),
        Launch::Game => Some(bold::GAME_CONTROLLER),
    }
}

/// `mark` on a badge over the glyph square's bottom-right corner.
fn paint_launch_mark(painter: &egui::Painter, glyph: egui::Rect, mark: &str, active: bool) {
    let center = glyph.right_bottom() - egui::vec2(MARK_INSET, MARK_INSET);
    painter.circle(
        center,
        MARK_RADIUS,
        BG,
        egui::Stroke::new(1.0, if active { ACCENT } else { BORDER }),
    );
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        mark,
        font(MARK_RADIUS * 1.2),
        if active { ACCENT } else { INK },
    );
}

/// An iconless tile's fill: a hue hashed from `label`, so a site keeps its
/// colour across runs and versions.
fn letter_fill(label: &str) -> egui::Color32 {
    // FNV-1a, 32-bit: stable, unlike std's hasher.
    const OFFSET: u32 = 0x811c_9dc5;
    const PRIME: u32 = 0x0100_0193;
    // Evenly spaced hues, so two tiles differ clearly or match exactly.
    const HUES: u32 = 10;
    let mut hash = label
        .bytes()
        .fold(OFFSET, |h, b| (h ^ u32::from(b)).wrapping_mul(PRIME));
    // murmur3's finalizer: FNV alone leaves short, similar labels on near hues.
    hash ^= hash >> 16;
    hash = hash.wrapping_mul(0x85eb_ca6b);
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(0xc2b2_ae35);
    hash ^= hash >> 16;
    let hue = (hash % HUES) as f32 / HUES as f32;
    egui::ecolor::HsvaGamma {
        h: hue,
        s: LETTER_SAT,
        v: LETTER_VAL,
        a: 1.0,
    }
    .into()
}

/// The rounded glyph square at a tile's top.
fn paint_glyph_square(
    painter: &egui::Painter,
    rect: egui::Rect,
    active: bool,
    fill: egui::Color32,
) -> egui::Rect {
    let glyph = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + GLYPH / 2.0 + 2.0),
        egui::vec2(GLYPH, GLYPH),
    );
    painter.rect_filled(glyph, 12.0, fill);
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
