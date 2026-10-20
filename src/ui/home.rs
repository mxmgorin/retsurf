//! Rendering of the built-in start page overlay (state lives in
//! [`crate::overlay::home`]): a brand mark (the banner scene, the wordmark, or
//! none), a bar holding the search / URL field, a tab bar of filters and a grid
//! of the bookmarks, scrolling as one stack over a footer naming what the
//! buttons do. Navigation is routed by [`crate::app`]; tiles open via
//! [`HomeAction::Open`].

use super::favicon::{Icon, PageIcons};
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
    pub sort: HomeSort,
}

/// One footer hint: the buttons that do it, then what it does.
pub(super) struct FooterHint {
    pub caps: Vec<&'static str>,
    pub label: &'static str,
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

/// Tile footprint (logical px) and grid spacing. A tile is its glyph square
/// over a label of up to [`LABEL_ROWS`] lines.
pub(super) const TILE_W: f32 = 96.0;
pub(super) const TILE_H: f32 = GLYPH_TOP + GLYPH + LABEL_GAP + LABEL_ROWS as f32 * LABEL_LINE_H;
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

/// Clearance between the page's foot and the scrolling stack.
const FOOT: f32 = 12.0;
/// The band over a scroll viewport's edge that dissolves into the page while
/// more waits beyond it.
const FADE: f32 = 24.0;

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

                    // The stack scrolls over the footer, so a long grid can't hide
                    // rows off the page.
                    // Last frame's height: the footer is laid out after the stack.
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

                    // One fade, ours below: egui's own is half-strength.
                    ui.spacing_mut().scroll.fade.strength = 0.0;
                    // The viewport spans the page and the stack centers inside
                    // it; shrunk to fit, the scrollbar would push it off-center.
                    let drawn = scroll_offset(ui, STACK_SALT);
                    let stack = egui::ScrollArea::vertical()
                        .id_salt(STACK_SALT)
                        .max_height(floor - area.top())
                        .auto_shrink([false, false])
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.add_space(top);
                                match view.mark {
                                    Mark::Banner(t) => {
                                        let image = egui::Image::new(t);
                                        ui.add(image.fit_to_exact_size(banner_size(t)));
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
                                add_tabs(ui, &view.tabs, block_w, tab_buttons, commands);
                                ui.add_space(ROW_GAP);
                                add_grid(ui, home, view.tiles, block_w, cols, commands);
                            });
                        });
                    settle(ui, drawn, &stack);
                    if scrolls_on(&stack, egui::Direction::TopDown) {
                        let foot = stack.inner_rect.bottom();
                        let band = egui::Rect::from_min_max(
                            egui::pos2(stack.inner_rect.left(), foot - FADE),
                            egui::pos2(stack.inner_rect.right(), foot),
                        );
                        fade(ui.painter(), band, egui::Direction::TopDown);
                    }
                    if !view.footer.is_empty() {
                        let left = area.center().x - block_w / 2.0;
                        let top = area.bottom() - FOOT - footer_h;
                        let row = egui::Rect::from_min_max(
                            egui::pos2(left, top),
                            egui::pos2(left + block_w, area.bottom()),
                        );
                        let h = add_footer(ui, row, view.footer);
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
/// A tab's height, its underline included.
const TAB_H: f32 = 30.0;
/// The active tab's underline.
const UNDERLINE_W: f32 = 3.0;
/// Space between tabs.
const TAB_GAP: f32 = 6.0;

/// The footer in `row`: a rule, then its hints centred; returns its height.
fn add_footer(ui: &mut egui::Ui, row: egui::Rect, footer: &[FooterHint]) -> f32 {
    ui.painter()
        .hline(row.x_range(), row.top(), egui::Stroke::new(LINE_W, BORDER));
    let hints: Vec<super::keys::Hint> = footer
        .iter()
        .map(|hint| (hint.caps.as_slice(), hint.label))
        .collect();
    let width = row.width();
    let inner = egui::Rect::from_min_max(row.min + egui::vec2(0.0, FOOTER_RULE_GAP), row.max);
    let h = ui
        .scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            super::keys::centered_hints(ui, &hints, width)
        })
        .inner;
    h + FOOTER_RULE_GAP
}

/// The scroll areas' ids: the stack, and the tabs row when it overflows.
const STACK_SALT: &str = "home_stack";
const TABS_SALT: &str = "home_tabs";

/// The offset the scroll area salted `salt` under `ui` was last drawn at.
fn scroll_offset(ui: &egui::Ui, salt: &str) -> Option<egui::Vec2> {
    let id = ui.make_persistent_id(salt);
    egui::scroll_area::State::load(ui.ctx(), id).map(|state| state.offset)
}

/// Repaint when `scroll` ended at another offset than `drawn`: egui clamps
/// after shrunken content, or reaches a target, only once the frame is built.
fn settle<R>(
    ui: &egui::Ui,
    drawn: Option<egui::Vec2>,
    scroll: &egui::scroll_area::ScrollAreaOutput<R>,
) {
    if drawn != Some(scroll.state.offset) {
        ui.ctx().request_repaint();
    }
}

/// Whether `scroll` holds content past its viewport's edge in `dir`: the far
/// edge of a top-down or left-to-right scroll, the near edge of the reverse.
fn scrolls_on<R>(scroll: &egui::scroll_area::ScrollAreaOutput<R>, dir: egui::Direction) -> bool {
    let (offset, span, content) = match dir {
        egui::Direction::TopDown | egui::Direction::BottomUp => (
            scroll.state.offset.y,
            scroll.inner_rect.height(),
            scroll.content_size.y,
        ),
        egui::Direction::LeftToRight | egui::Direction::RightToLeft => (
            scroll.state.offset.x,
            scroll.inner_rect.width(),
            scroll.content_size.x,
        ),
    };
    match dir {
        egui::Direction::TopDown | egui::Direction::LeftToRight => {
            offset + span < content - FOOTER_SETTLE
        }
        egui::Direction::BottomUp | egui::Direction::RightToLeft => offset > FOOTER_SETTLE,
    }
}

/// Dissolve `band` into the page along `dir`: clear where it starts, solid
/// where it ends, so clipped content fades instead of being cut.
fn fade(painter: &egui::Painter, band: egui::Rect, dir: egui::Direction) {
    let clear = egui::Color32::TRANSPARENT;
    // Corners in mesh order: left-top, right-top, right-bottom, left-bottom.
    let colors = match dir {
        egui::Direction::TopDown => [clear, clear, BG, BG],
        egui::Direction::BottomUp => [BG, BG, clear, clear],
        egui::Direction::LeftToRight => [clear, BG, BG, clear],
        egui::Direction::RightToLeft => [BG, clear, clear, BG],
    };
    let corners = [
        band.left_top(),
        band.right_top(),
        band.right_bottom(),
        band.left_bottom(),
    ];
    let mut mesh = egui::Mesh::default();
    for (pos, color) in corners.into_iter().zip(colors) {
        mesh.colored_vertex(pos, color);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
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
    let bar = frame.show(ui, |ui| {
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
    keep_visible(ui, bar.response.id, bar.response.rect, selected);
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
const MARK_RADIUS: f32 = 12.0;
/// How far the badge's center sits inside the glyph square's corner: under the
/// radius, so the badge breaks the corner.
const MARK_INSET: f32 = 6.0;
/// The badge's rim in the page's colour, parting it from the square.
const MARK_RIM: f32 = 2.0;
/// The badge's glyph, as a share of its radius.
const MARK_GLYPH: f32 = 1.2;

/// The tab bar: the filter tabs at the left, the sort at the right; with
/// `buttons`, the strip buttons at either end.
fn add_tabs(
    ui: &mut egui::Ui,
    tabs: &Tabs,
    width: f32,
    buttons: bool,
    commands: &mut Vec<AppCommand>,
) {
    // Painted under the tabs once the row's place is known.
    let rule = ui.painter().add(egui::Shape::Noop);
    let row = ui.allocate_ui_with_layout(
        egui::vec2(width, TAB_H),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = TAB_GAP;
            if buttons {
                strip_button(ui, Strip::QuickAccess, commands);
            }
            if sort_button(ui, tabs.sort).clicked() {
                commands.push(AppCommand::Home(HomeAction::StepSort(1)));
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
    let rect = row.response.rect;
    let y = rect.bottom() - LINE_W / 2.0;
    let stroke = egui::Stroke::new(LINE_W, BORDER);
    ui.painter()
        .set(rule, egui::Shape::hline(rect.x_range(), y, stroke));
}

/// The sort as it stands, muted unless hovered.
fn sort_button(ui: &mut egui::Ui, sort: HomeSort) -> egui::Response {
    let text = format!("{}  {}", bold::SORT_ASCENDING, sort_label(sort));
    let pad = 2.0 * ui.spacing().button_padding.x;
    let size = egui::vec2(tab_text_width(ui, &text) + pad, TAB_H);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let color = if resp.hovered() { INK } else { MUTED };
    let center = egui::pos2(rect.center().x, rect.center().y - UNDERLINE_W / 2.0);
    let font = egui::FontId::proportional(ROW_FONT);
    ui.painter()
        .text(center, egui::Align2::CENTER_CENTER, text, font, color);
    resp
}

/// The filter tabs in what is left of the row, a rule parting the kinds from
/// the tags: centred when they fit, else scrolled to keep the active one in
/// view when it changes, the clipped end fading out.
fn add_filter_tabs(ui: &mut egui::Ui, tabs: &Tabs, commands: &mut Vec<AppCommand>) {
    let pad = 2.0 * ui.spacing().button_padding.x;
    let labels: Vec<String> = tabs.filters.iter().map(Filter::label).collect();
    let parted = |at: usize| {
        at > 0
            && matches!(tabs.filters[at], Filter::Tag(_))
            && !matches!(tabs.filters[at - 1], Filter::Tag(_))
    };
    let tabs_w: f32 = labels
        .iter()
        .enumerate()
        .map(|(at, label)| {
            let rule = if parted(at) { LINE_W + TAB_GAP } else { 0.0 };
            tab_text_width(ui, label) + pad + TAB_GAP + rule
        })
        .sum::<f32>()
        - TAB_GAP;
    let room = ui.available_width();
    let shown = egui::Id::new("home_tabs_active");
    let token = tabs.filter.token();
    let moved = ui.ctx().data(|d| d.get_temp::<String>(shown)) != Some(token.clone());
    let add = |ui: &mut egui::Ui, commands: &mut Vec<AppCommand>| {
        for (at, (filter, label)) in tabs.filters.iter().zip(&labels).enumerate() {
            if parted(at) {
                divider(ui);
            }
            let active = *filter == tabs.filter;
            let resp = tab(ui, label, active);
            if active && moved {
                resp.scroll_to_me(None);
            }
            if resp.clicked() {
                commands.push(AppCommand::Home(HomeAction::SetFilter(filter.token())));
            }
        }
    };
    if tabs_w <= room {
        ui.add_space((room - tabs_w) / 2.0);
        add(ui, commands);
    } else {
        let drawn = scroll_offset(ui, TABS_SALT);
        let scroll = egui::ScrollArea::horizontal()
            .id_salt(TABS_SALT)
            .max_width(room)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = TAB_GAP;
                    add(ui, commands);
                });
            });
        settle(ui, drawn, &scroll);
        // Above the rule, which runs whole.
        let view = scroll
            .inner_rect
            .with_max_y(scroll.inner_rect.bottom() - LINE_W);
        if scrolls_on(&scroll, egui::Direction::RightToLeft) {
            let band = view.with_max_x(view.left() + FADE);
            fade(ui.painter(), band, egui::Direction::RightToLeft);
        }
        if scrolls_on(&scroll, egui::Direction::LeftToRight) {
            let band = view.with_min_x(view.right() - FADE);
            fade(ui.painter(), band, egui::Direction::LeftToRight);
        }
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

/// One tab: its name, muted unless active or hovered, the active one
/// underlined in the accent along the tab bar's rule.
fn tab(ui: &mut egui::Ui, text: &str, on: bool) -> egui::Response {
    let pad = 2.0 * ui.spacing().button_padding.x;
    let size = egui::vec2(tab_text_width(ui, text) + pad, TAB_H);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let color = match (on, resp.hovered()) {
        (true, _) => egui::Color32::WHITE,
        (false, true) => INK,
        (false, false) => MUTED,
    };
    let center = egui::pos2(rect.center().x, rect.center().y - UNDERLINE_W / 2.0);
    let font = egui::FontId::proportional(ROW_FONT);
    ui.painter()
        .text(center, egui::Align2::CENTER_CENTER, text, font, color);
    if on {
        let y = rect.bottom() - UNDERLINE_W / 2.0;
        let stroke = egui::Stroke::new(UNDERLINE_W, ACCENT);
        ui.painter().hline(rect.x_range(), y, stroke);
    }
    resp
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

/// Lay out `total` tiles in rows of `cols` within `width`. Every row takes the
/// full width from its left, so a partial last row keeps its columns under the
/// ones above: Down lands on the tile drawn below.
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
                ui.allocate_ui_with_layout(
                    egui::vec2(width, TILE_H),
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

/// Glyph-square side length within a tile, and its inset from the tile's top.
pub(super) const GLYPH: f32 = 52.0;
const GLYPH_TOP: f32 = 2.0;
/// A site icon inside the glyph square: near the 32 px cache, so it stays sharp.
const DIAL_ICON: f32 = 34.0;
/// How much of an icon's tint colours its glyph square over [`SURFACE`].
const TINT_MIX: f32 = 0.35;
/// Saturation and value of an iconless tile's fill; its hue comes from the label.
const LETTER_SAT: f32 = 0.6;
const LETTER_VAL: f32 = 0.42;
/// An iconless tile's monogram: the initials of this many words, in this type.
const MONOGRAM_WORDS: usize = 2;
const MONOGRAM_FONT: f32 = 20.0;
/// A word this short (an article, a preposition) lends the monogram no initial.
const MONOGRAM_SKIP: usize = 2;
/// The label under the glyph square: its type, its line, its lines, and the
/// gap over it.
const LABEL_FONT: f32 = 12.0;
const LABEL_LINE_H: f32 = 15.0;
const LABEL_ROWS: usize = 2;
const LABEL_GAP: f32 = 8.0;

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
    keep_visible(ui, resp.id, rect, selected);
    let label = bookmark_label(bookmark);
    let active = selected || resp.hovered();
    let glyph = paint_tile(ui.painter(), rect, &label, icon, active);
    paint_launch_mark(ui.painter(), glyph, bookmark.launch);
    resp
}

/// What a bookmark is shown as: its name, else its site.
pub fn bookmark_label(bookmark: &Bookmark) -> String {
    match bookmark.name.is_empty() {
        true => brand_label(&bookmark.url),
        false => bookmark.name.clone(),
    }
}

/// Scroll the selected item (`id` at `rect`) into view on the frame the
/// selection reaches it: going by visibility alone would fight a manual scroll.
/// No-op outside a scroll area, where the clip rect is the whole page.
fn keep_visible(ui: &egui::Ui, id: egui::Id, rect: egui::Rect, selected: bool) {
    let key = egui::Id::new(("home_selected", ui.layer_id()));
    if !selected || ui.ctx().data(|d| d.get_temp::<egui::Id>(key)) == Some(id) {
        return;
    }
    ui.ctx().data_mut(|d| d.insert_temp(key, id));
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
            painter.text(
                glyph.center(),
                egui::Align2::CENTER_CENTER,
                monogram(label),
                font(MONOGRAM_FONT),
                INK,
            );
        }
    }

    // The label under the glyph, wrapped by word and elided past its lines.
    let color = if active { INK } else { MUTED };
    let mut job = egui::text::LayoutJob::simple(label.to_string(), font(LABEL_FONT), color, TILE_W);
    job.halign = egui::Align::Center;
    job.wrap.max_rows = LABEL_ROWS;
    job.wrap.break_anywhere = false;
    let galley = painter.layout_job(job);
    painter.galley(
        egui::pos2(rect.center().x, glyph.bottom() + LABEL_GAP),
        galley,
        color,
    );
    glyph
}

/// The letters standing in for a missing icon: the initials of the label's
/// first words, skipping the short ones; its first letter when that leaves none.
fn monogram(label: &str) -> String {
    let initial = |word: &str| word.chars().next().into_iter().flat_map(char::to_uppercase);
    let initials: String = label
        .split_whitespace()
        .filter(|word| word.chars().count() > MONOGRAM_SKIP)
        .take(MONOGRAM_WORDS)
        .flat_map(initial)
        .collect();
    match initials.is_empty() {
        true => label
            .chars()
            .next()
            .into_iter()
            .flat_map(char::to_uppercase)
            .collect(),
        false => initials,
    }
}

/// The badge over the glyph square's bottom-right corner of a bookmark that
/// opens as a program: the accent for a game, muted for an app; none for a site.
fn paint_launch_mark(painter: &egui::Painter, glyph: egui::Rect, launch: Launch) {
    let (mark, fill) = match launch {
        Launch::Browser => return,
        Launch::App => (bold::APP_WINDOW, MUTED),
        Launch::Game => (bold::GAME_CONTROLLER, ACCENT),
    };
    let center = glyph.right_bottom() - egui::vec2(MARK_INSET, MARK_INSET);
    painter.circle(center, MARK_RADIUS, fill, egui::Stroke::new(MARK_RIM, BG));
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        mark,
        font(MARK_RADIUS * MARK_GLYPH),
        BG,
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
        egui::pos2(rect.center().x, rect.top() + GLYPH_TOP + GLYPH / 2.0),
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
