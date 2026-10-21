//! Rendering of the built-in start page overlay (state lives in
//! [`crate::overlay::home`]): a brand mark (the banner scene, the wordmark, or
//! none), a bar holding the search / URL field, a tab bar of filters and a grid
//! of the bookmarks, scrolling as one stack over a footer naming what the
//! buttons do. Navigation is routed by [`crate::app`]; tiles open via
//! [`HomeAction::Open`].

use super::favicon::{Icon, PageIcons};
use super::keys;
use super::labels::{bookmark_label, site_label};
use super::panel::{fade, scroll_offset, scrolls_on, settle, EPSILON, FADE};
use super::theme::{ACCENT, BG, BORDER, INK, MUTED, ROW_FONT, SURFACE, SURF_WARM};
use super::wordmark;
use super::OskCaret;
use crate::command::{AppCommand, HomeAction, QuickAccessAction};
use crate::config::{HomeLayout, HomeSize};
use crate::data::bookmarks::{Bookmark, Filter, Launch};
use crate::overlay::home::Home;
use crate::overlay::quick_access::Strip;
use egui_phosphor::bold;
use egui_sdl2::egui;

/// Typeface for the start-page body text. egui bundles only two real text faces
/// — `Ubuntu-Light` and `Hack`; swap the call below to flip between them. The
/// wordmark stays on Hack via [`wordmark`] regardless.
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
    pub layout: HomeLayout,
    pub size: HomeSize,
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
#[derive(Clone, Copy)]
pub(super) struct Tabs<'a> {
    pub filters: &'a [Filter],
    pub filter: &'a Filter,
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

/// A grid tile's geometry: its width, its glyph square, the label lines under
/// it and the space between tiles.
#[derive(Clone, Copy)]
struct TileStyle {
    w: f32,
    glyph: Glyph,
    label_rows: usize,
    gap: f32,
}

impl TileStyle {
    const fn h(&self) -> f32 {
        GLYPH_TOP + self.glyph.side + LABEL_GAP + self.label_rows as f32 * LABEL_LINE_H
    }
}

/// A glyph square: its side and corner, the site icon and monogram inside it,
/// and the launch badge's radius.
#[derive(Clone, Copy)]
struct Glyph {
    side: f32,
    radius: f32,
    /// Near the icon cache's size: far past it an icon blurs.
    icon: f32,
    monogram: f32,
    mark: f32,
}

/// The grid's tiles per size.
const fn tile_style(size: HomeSize) -> TileStyle {
    match size {
        HomeSize::Small => TileStyle {
            w: 72.0,
            glyph: Glyph {
                side: 40.0,
                radius: 10.0,
                icon: 26.0,
                monogram: 16.0,
                mark: 9.0,
            },
            label_rows: 1,
            gap: 8.0,
        },
        HomeSize::Medium => GRID,
        HomeSize::Large => TileStyle {
            w: 120.0,
            glyph: Glyph {
                side: 68.0,
                radius: 14.0,
                icon: 44.0,
                monogram: 26.0,
                mark: 14.0,
            },
            label_rows: 2,
            gap: 14.0,
        },
    }
}

/// The medium tile; its grid sets the page's width.
const GRID: TileStyle = TileStyle {
    w: 96.0,
    glyph: Glyph {
        side: 52.0,
        radius: 12.0,
        icon: 34.0,
        monogram: 20.0,
        mark: 12.0,
    },
    label_rows: 2,
    gap: 12.0,
};

/// A list row: its height, its glyph square, its name's type and whether its
/// site shows under the name.
#[derive(Clone, Copy)]
struct RowStyle {
    h: f32,
    glyph: Glyph,
    name: f32,
    site: bool,
}

/// The list's rows per size.
const fn row_style(size: HomeSize) -> RowStyle {
    match size {
        HomeSize::Small => RowStyle {
            h: 40.0,
            glyph: Glyph {
                side: 28.0,
                radius: 6.0,
                icon: 20.0,
                monogram: 12.0,
                mark: 9.0,
            },
            name: ROW_FONT,
            site: false,
        },
        HomeSize::Medium => RowStyle {
            h: 48.0,
            glyph: Glyph {
                side: 36.0,
                radius: 8.0,
                icon: 24.0,
                monogram: 14.0,
                mark: 10.0,
            },
            name: ROW_FONT,
            site: true,
        },
        HomeSize::Large => RowStyle {
            h: 60.0,
            glyph: Glyph {
                side: 48.0,
                radius: 10.0,
                icon: 32.0,
                monogram: 18.0,
                mark: 12.0,
            },
            name: LARGE_NAME_FONT,
            site: true,
        },
    }
}

/// How one bookmark is drawn: a grid tile or a list row.
#[derive(Clone, Copy)]
enum Look {
    Tile(TileStyle),
    Row(RowStyle),
}

/// A list row's corner, inner margin and the space between rows.
const ROW_RADIUS: f32 = 8.0;
const ROW_PAD: f32 = 8.0;
const ROW_SPACING: f32 = 4.0;
/// A large row's name type.
const LARGE_NAME_FONT: f32 = 17.0;
/// A row's site line under its name.
const SITE_FONT: f32 = 12.0;
/// A tag pill: its type, its margin across, height and spacing.
const TAG_FONT: f32 = 11.0;
const TAG_PAD_X: f32 = 6.0;
const TAG_H: f32 = 18.0;
const TAG_GAP: f32 = 4.0;
/// The most of a row the tags may take.
const TAGS_SHARE: f32 = 0.4;

/// How many tiles `w` wide, `gap` apart, fit in `width`; at least one.
fn fit(width: f32, w: f32, gap: f32) -> usize {
    (((width + gap) / (w + gap)).floor() as usize).max(1)
}

/// The width of `cols` tiles `w` wide, `gap` apart.
fn span(cols: usize, w: f32, gap: f32) -> f32 {
    cols as f32 * w + cols.saturating_sub(1) as f32 * gap
}

/// Clearance between the page's foot and the scrolling stack.
const FOOT: f32 = 12.0;

/// Draw the start-page overlay over the (blank) web view, confined to `area`,
/// the web view's rect, so the toolbar stays usable. Any activation is pushed
/// as a command for the app to execute.
pub(super) fn add_home(
    ctx: &egui::Context,
    home: &mut Home,
    view: HomeView,
    area: egui::Rect,
    osk_caret: Option<OskCaret>,
    commands: &mut Vec<AppCommand>,
) {
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
                    // The medium grid's width in every layout and size, so the
                    // field keeps its own.
                    let grid_cols = fit(max_w, GRID.w, GRID.gap);
                    let block_w = span(grid_cols, GRID.w, GRID.gap).min(max_w);
                    let cols = match view.layout {
                        HomeLayout::Grid => {
                            let style = tile_style(view.size);
                            fit(block_w, style.w, style.gap)
                        }
                        HomeLayout::List => 1,
                    };
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
                            let h = wordmark::wordmark_height(ui);
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
                                    Mark::Wordmark => wordmark::add_wordmark(ui),
                                    Mark::None => {}
                                }
                                ui.add_space(gap_top);
                                let buttons = view.strip_buttons;
                                if view.search {
                                    add_search_bar(ui, home, block_w, osk_caret, buttons, commands);
                                    ui.add_space(ROW_GAP);
                                }
                                let tab_buttons = buttons && !view.search;
                                add_tabs(ui, view.tabs, block_w, tab_buttons, commands);
                                ui.add_space(ROW_GAP);
                                let tiles = view.tiles;
                                let look = (view.layout, view.size);
                                add_grid(ui, home, tiles, look, block_w, cols, commands);
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
                        if (h - footer_h).abs() > EPSILON {
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
/// Space between the page's rows.
const ROW_GAP: f32 = 12.0;
/// The search bar's height, and its corners.
const BAR_H: f32 = 44.0;
const BAR_RADIUS: f32 = 10.0;
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
    let hints: Vec<keys::Hint> = footer
        .iter()
        .map(|hint| (hint.caps.as_slice(), hint.label))
        .collect();
    let width = row.width();
    let inner = egui::Rect::from_min_max(row.min + egui::vec2(0.0, FOOTER_RULE_GAP), row.max);
    let h = ui
        .scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            keys::centered_hints(ui, &hints, width)
        })
        .inner;
    h + FOOTER_RULE_GAP
}

/// The scroll areas' ids: the stack, and the tabs row when it overflows.
const STACK_SALT: &str = "home_stack";
const TABS_SALT: &str = "home_tabs";

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
        .corner_radius(BAR_RADIUS)
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
        Strip::QuickAccess => bold::DOTS_THREE,
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

/// How far the badge's center sits inside the glyph square's corner, as a
/// share of its radius: under one, so the badge breaks the corner.
const MARK_INSET: f32 = 0.5;
/// The badge's size, as a share of the style's badge radius.
const MARK_SCALE: f32 = 0.85;
/// The badge's glyph, as a share of its radius.
const MARK_GLYPH: f32 = 1.2;

/// The tab bar: the filter tabs centred; with `buttons`, the strip buttons at
/// either end.
fn add_tabs(
    ui: &mut egui::Ui,
    tabs: Tabs,
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

/// The filter tabs in what is left of the row, a rule parting the kinds from
/// the tags: centred when they fit, else scrolled to keep the active one in
/// view when it changes, the clipped end fading out.
fn add_filter_tabs(ui: &mut egui::Ui, tabs: Tabs, commands: &mut Vec<AppCommand>) {
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
            let active = filter == tabs.filter;
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

/// Shown in place of an empty grid, in this type.
const EMPTY_HINT: &str = "Bookmark a page from Quick Access to see it here";
const EMPTY_FONT: f32 = 14.0;

/// The bookmarks the view shows, in its order: a tile each in `cols` columns
/// centred in `width`, or a row each across it.
fn add_grid(
    ui: &mut egui::Ui,
    home: &Home,
    tiles: Tiles,
    (layout, size): (HomeLayout, HomeSize),
    width: f32,
    cols: usize,
    commands: &mut Vec<AppCommand>,
) {
    if tiles.visible.is_empty() {
        ui.label(
            egui::RichText::new(EMPTY_HINT)
                .font(font(EMPTY_FONT))
                .color(MUTED),
        );
        return;
    }
    let look = match layout {
        HomeLayout::Grid => Look::Tile(tile_style(size)),
        HomeLayout::List => Look::Row(row_style(size)),
    };
    let (w, h, gap) = match look {
        Look::Tile(tile) => (span(cols, tile.w, tile.gap), tile.h(), tile.gap),
        Look::Row(row) => (width, row.h, ROW_SPACING),
    };
    tile_grid(ui, w, cols, tiles.visible.len(), h, gap, |ui, i| {
        let bookmark = &tiles.bookmarks[tiles.visible[i]];
        let selected = home.tile() == Some(i);
        let icon = tiles.icons.icon(&bookmark.url);
        let resp = match look {
            Look::Tile(tile) => add_tile(ui, tile, bookmark, icon, selected),
            Look::Row(row) => add_row(ui, row, width, bookmark, icon, selected),
        };
        if resp.clicked() {
            commands.push(AppCommand::Home(HomeAction::Open(i)));
        }
        if resp.secondary_clicked() {
            commands.push(AppCommand::Home(HomeAction::Options(i)));
        }
    });
}

/// Lay out `total` tiles in rows of `cols`, `row_h` tall and `gap` apart, within
/// `width`. A partial last row keeps its columns under the ones above: Down
/// lands on the tile drawn below.
fn tile_grid(
    ui: &mut egui::Ui,
    width: f32,
    cols: usize,
    total: usize,
    row_h: f32,
    gap: f32,
    mut tile: impl FnMut(&mut egui::Ui, usize),
) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            for (row, row_start) in (0..total).step_by(cols).enumerate() {
                // Between rows only: a trailing gap would pad the grid's foot.
                if row > 0 {
                    ui.add_space(gap);
                }
                let n = (total - row_start).min(cols);
                ui.allocate_ui_with_layout(
                    egui::vec2(width, row_h),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for slot in row_start..row_start + n {
                            tile(ui, slot);
                        }
                    },
                );
            }
        },
    );
}

/// The glyph square's inset from the tile's top.
const GLYPH_TOP: f32 = 2.0;
/// How much of an icon's tint colours its glyph square over [`SURFACE`].
const TINT_MIX: f32 = 0.35;
/// Saturation and value of an iconless tile's fill; its hue comes from the label.
const LETTER_SAT: f32 = 0.6;
const LETTER_VAL: f32 = 0.42;
/// An iconless tile's monogram: the initials of this many words.
const MONOGRAM_WORDS: usize = 2;
/// A word this short (an article, a preposition) lends the monogram no initial.
const MONOGRAM_SKIP: usize = 2;
/// The label under the glyph square: its type, its line, and the gap over it.
const LABEL_FONT: f32 = 12.0;
const LABEL_LINE_H: f32 = 15.0;
const LABEL_GAP: f32 = 8.0;

/// One speed-dial tile: a rounded "glyph" square holding the brand initial, with
/// the brand name beneath it. Custom-painted (not a Button) for the two-tier
/// look.
fn add_tile(
    ui: &mut egui::Ui,
    style: TileStyle,
    bookmark: &Bookmark,
    icon: Option<Icon>,
    selected: bool,
) -> egui::Response {
    let size = egui::vec2(style.w, style.h());
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    keep_visible(ui, resp.id, rect, selected);
    let label = bookmark_label(bookmark);
    let active = selected || resp.hovered();
    let glyph = paint_tile(ui.painter(), rect, style, &label, icon, active);
    paint_launch_mark(ui.painter(), glyph, style.glyph.mark, bookmark.launch);
    resp
}

/// One list row `width` wide: the glyph square, the bookmark's name over its
/// site where the style shows it, and its tags at the far end.
fn add_row(
    ui: &mut egui::Ui,
    style: RowStyle,
    width: f32,
    bookmark: &Bookmark,
    icon: Option<Icon>,
    selected: bool,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, style.h), egui::Sense::click());
    keep_visible(ui, resp.id, rect, selected);
    let label = bookmark_label(bookmark);
    let active = selected || resp.hovered();
    let painter = ui.painter();
    if active {
        painter.rect_filled(rect, ROW_RADIUS, SURFACE);
    }
    let glyph = egui::Rect::from_min_size(
        egui::pos2(
            rect.left() + ROW_PAD,
            rect.center().y - style.glyph.side / 2.0,
        ),
        egui::vec2(style.glyph.side, style.glyph.side),
    );
    paint_glyph(painter, glyph, style.glyph, &label, icon, active);
    paint_launch_mark(painter, glyph, style.glyph.mark, bookmark.launch);

    let right = rect.right() - ROW_PAD;
    let tags_left = paint_tags(
        painter,
        &bookmark.tags,
        right,
        rect.center().y,
        width * TAGS_SHARE,
    );
    let left = glyph.right() + ROW_PAD;
    let room = (tags_left - ROW_PAD - left).max(0.0);
    let line = |text: String, size: f32, color: egui::Color32| {
        let mut job = egui::text::LayoutJob::simple_singleline(text, font(size), color);
        job.wrap.max_width = room;
        job.wrap.max_rows = 1;
        painter.layout_job(job)
    };
    let name = line(label, style.name, INK);
    let site = style
        .site
        .then(|| line(site_label(&bookmark.url), SITE_FONT, MUTED));
    let site_h = site.as_ref().map_or(0.0, |g| g.size().y);
    let top = rect.center().y - (name.size().y + site_h) / 2.0;
    let name_h = name.size().y;
    painter.galley(egui::pos2(left, top), name, INK);
    if let Some(site) = site {
        painter.galley(egui::pos2(left, top + name_h), site, MUTED);
    }
    resp
}

/// Paint `tags` as pills ending at `right`, centred on `y`, within `budget`
/// across; the ones that do not fit count in a closing `+N` pill. Returns the
/// left edge drawn to, or `right` with no tags.
fn paint_tags(painter: &egui::Painter, tags: &[String], right: f32, y: f32, budget: f32) -> f32 {
    let pill = |text: String| painter.layout_no_wrap(text, font(TAG_FONT), MUTED);
    let width = |g: &std::sync::Arc<egui::Galley>| g.size().x + 2.0 * TAG_PAD_X;
    let mut pills: Vec<_> = Vec::new();
    let mut used = 0.0;
    for (at, tag) in tags.iter().enumerate() {
        let galley = pill(tag.clone());
        let rest = tags.len() - at - 1;
        let reserve = match rest {
            0 => 0.0,
            n => width(&pill(format!("+{n}"))) + TAG_GAP,
        };
        if used + width(&galley) + reserve > budget {
            break;
        }
        used += width(&galley) + TAG_GAP;
        pills.push(galley);
    }
    if pills.len() < tags.len() {
        pills.push(pill(format!("+{}", tags.len() - pills.len())));
    }
    let mut x = right;
    for galley in pills.into_iter().rev() {
        let rect = egui::Rect::from_min_max(
            egui::pos2(x - width(&galley), y - TAG_H / 2.0),
            egui::pos2(x, y + TAG_H / 2.0),
        );
        let stroke = egui::Stroke::new(LINE_W, BORDER);
        painter.rect_stroke(rect, TAG_H / 2.0, stroke, egui::StrokeKind::Inside);
        let at = rect.center() - galley.size() / 2.0;
        painter.galley(at, galley, MUTED);
        x = rect.left() - TAG_GAP;
    }
    match x == right {
        true => right,
        false => x + TAG_GAP,
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

/// Paint a tile's visuals into `rect`: its glyph square over its label;
/// returns the square. The caller owns the click region.
fn paint_tile(
    painter: &egui::Painter,
    rect: egui::Rect,
    style: TileStyle,
    label: &str,
    icon: Option<Icon>,
    active: bool,
) -> egui::Rect {
    let side = style.glyph.side;
    let glyph = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + GLYPH_TOP + side / 2.0),
        egui::vec2(side, side),
    );
    paint_glyph(painter, glyph, style.glyph, label, icon, active);

    // The label under the glyph, wrapped by word and elided past its lines.
    let color = if active { INK } else { MUTED };
    let mut job =
        egui::text::LayoutJob::simple(label.to_string(), font(LABEL_FONT), color, style.w);
    job.halign = egui::Align::Center;
    job.wrap.max_rows = style.label_rows;
    job.wrap.break_anywhere = false;
    let galley = painter.layout_job(job);
    painter.galley(
        egui::pos2(rect.center().x, glyph.bottom() + LABEL_GAP),
        galley,
        color,
    );
    glyph
}

/// Paint the glyph square `glyph`: the site `icon` on a square tinted after it,
/// else the label's monogram on a fill hashed from it.
fn paint_glyph(
    painter: &egui::Painter,
    glyph: egui::Rect,
    style: Glyph,
    label: &str,
    icon: Option<Icon>,
    active: bool,
) {
    let fill = match icon {
        Some(icon) if icon.dark => INK,
        Some(icon) => SURFACE.lerp_to_gamma(icon.tint, TINT_MIX),
        None => letter_fill(label),
    };
    paint_glyph_square(painter, glyph, style.radius, active, fill);
    match icon {
        Some(Icon { bare: texture, .. }) => {
            let fit = style.icon / texture.size_vec2().max_elem();
            let at = egui::Rect::from_center_size(glyph.center(), texture.size_vec2() * fit);
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(texture.id(), at, uv, egui::Color32::WHITE);
        }
        None => {
            painter.text(
                glyph.center(),
                egui::Align2::CENTER_CENTER,
                monogram(label),
                font(style.monogram),
                INK,
            );
        }
    }
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

/// The badge over the glyph square's bottom-right corner naming how the
/// bookmark opens: a page-coloured disc, its glyph muted for a site, in the
/// accent for an app and in the brand's warm end for a game.
fn paint_launch_mark(painter: &egui::Painter, glyph: egui::Rect, radius: f32, launch: Launch) {
    let (mark, ink) = match launch {
        Launch::Browser => (bold::GLOBE, MUTED),
        Launch::App => (bold::APP_WINDOW, ACCENT),
        Launch::Game => (bold::GAME_CONTROLLER, SURF_WARM),
    };
    let radius = radius * MARK_SCALE;
    let inset = radius * MARK_INSET;
    let center = glyph.right_bottom() - egui::vec2(inset, inset);
    painter.circle(center, radius, BG, egui::Stroke::new(LINE_W, BORDER));
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        mark,
        font(radius * MARK_GLYPH),
        ink,
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

/// The rounded glyph square at `glyph`, its corners `radius`.
fn paint_glyph_square(
    painter: &egui::Painter,
    glyph: egui::Rect,
    radius: f32,
    active: bool,
    fill: egui::Color32,
) {
    painter.rect_filled(glyph, radius, fill);
    painter.rect_stroke(
        glyph,
        radius,
        egui::Stroke::new(
            if active { 2.0 } else { 1.0 },
            if active { ACCENT } else { BORDER },
        ),
        egui::StrokeKind::Inside,
    );
}
