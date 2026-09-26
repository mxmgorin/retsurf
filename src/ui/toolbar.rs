//! The toolbar (top or bottom, per the display config): navigation buttons, the
//! address bar, bookmark toggle, and the chips that jump into menu sections.

use super::theme;
use super::OskCaret;
use crate::browser::{BrowserCommand, BrowserState};
use crate::command::{AppCommand, MenuAction, SettingsAction};
use crate::config::ToolbarPosition;
use crate::overlay::menu::Section;
use crate::overlay::settings::SettingsSection;
use egui_phosphor::{bold, fill};
use egui_sdl2::egui::{self, Vec2};

/// Side of a toolbar icon slot (logical px), sized for a fingertip.
const SLOT: f32 = 36.0;

/// Toolbar glyph size, a step up from [`theme::ICON_SIZE`] to fill the slot.
const ICON: f32 = 19.0;

/// The address field's height, a step inside the toolbar's slots.
const FIELD_H: f32 = 28.0;

/// Glyphs inside the address field, a step under the toolbar's, and the width
/// of a button holding one.
const FIELD_ICON: f32 = 16.0;
const FIELD_BUTTON_W: f32 = 26.0;

/// The address field's text.
const FIELD_FONT: f32 = 14.0;

/// The zoom pill's text and height, sized to sit inside the address field.
const ZOOM_FONT: f32 = 12.0;
const ZOOM_PILL_H: f32 = 20.0;

/// What the empty address field offers.
const FIELD_HINT: &str = "Search or enter address";

fn icon(glyph: &str) -> egui::RichText {
    theme::icon(glyph).size(ICON)
}

fn icon_fill(glyph: &str) -> egui::RichText {
    theme::icon_fill(glyph).size(ICON)
}

/// The address field's inner margin across; its rounded ends need the air.
const FIELD_MARGIN_X: i8 = 6;

/// Row height every item centers against: `Align::Center` only knows the height
/// laid out so far.
const ROW_H: f32 = SLOT;

/// Create a frameless button with square sizing, as used in the toolbar.
#[inline]
fn new_toolbar_button<'a>(text: impl egui::IntoAtoms<'a>) -> egui::Button<'a> {
    egui::Button::new(text)
        .frame(false)
        .min_size(Vec2 { x: SLOT, y: SLOT })
}

/// Vertically centered: egui puts a TextEdit's text at the top of its box, and
/// this box is the field's height, not the text's.
#[inline]
fn new_text_edit<'a>(text: &'a mut String, id: &str) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(text)
        .id(egui::Id::new(id))
        .vertical_align(egui::Align::Center)
        .font(egui::FontId::proportional(FIELD_FONT))
}

/// A frameless button inside the address field, its glyph muted unless `lit`.
fn add_field_button(ui: &mut egui::Ui, glyph: egui::RichText, lit: bool) -> egui::Response {
    let color = if lit { theme::ACCENT } else { theme::MUTED };
    let button = egui::Button::new(glyph.size(FIELD_ICON).color(color))
        .frame(false)
        .min_size(Vec2 {
            x: FIELD_BUTTON_W,
            y: FIELD_H,
        });
    ui.add(button)
}

/// The glyph before the address: search while editing and on the start page
/// (whose own button already shows a house), else what the loaded page is.
fn site_glyph(page_url: &str, editing: bool) -> (&'static str, egui::Color32) {
    match page_url {
        url if editing || url == crate::browser::HOME_URL => (bold::MAGNIFYING_GLASS, theme::MUTED),
        url if url.starts_with("https:") => (bold::LOCK_SIMPLE, theme::MUTED),
        url if url.starts_with("http:") => (bold::LOCK_SIMPLE_OPEN, theme::WARN),
        _ => (bold::GLOBE_SIMPLE, theme::MUTED),
    }
}

/// An address as read at rest: the host without scheme or `www.`, then the rest
/// of the URL. Anything that is not an http(s) URL reads as written, and the
/// start page as nothing, so the field shows its hint.
fn display_address(location: &str) -> (String, String) {
    if location == crate::browser::HOME_URL {
        return (String::new(), String::new());
    }
    let Ok(url) = url::Url::parse(location) else {
        return (location.to_string(), String::new());
    };
    let Some(host) = url
        .host_str()
        .filter(|_| matches!(url.scheme(), "http" | "https"))
    else {
        return (location.to_string(), String::new());
    };
    let host = host.strip_prefix("www.").unwrap_or(host);
    let host = match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    };
    let start = url[..url::Position::BeforePath].len();
    let rest = &url.as_str()[start..];
    let rest = if rest == "/" { "" } else { rest };
    (host, rest.to_string())
}

/// Paint the at-rest address over the field's own (invisible) text: the host in
/// ink, the rest muted, cut to one line. An address that reads as nothing gets
/// the hint, which egui draws only for an empty buffer.
fn paint_display_address(ui: &egui::Ui, rect: egui::Rect, location: &str) {
    let (host, rest) = match display_address(location) {
        (host, _) if host.is_empty() && location.is_empty() => return,
        (host, _) if host.is_empty() => (String::new(), FIELD_HINT.to_string()),
        parts => parts,
    };
    let format = |color| egui::TextFormat {
        font_id: egui::FontId::proportional(FIELD_FONT),
        color,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    job.append(&host, 0.0, format(theme::INK));
    job.append(&rest, 0.0, format(theme::MUTED));
    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width());
    let galley = ui.painter().layout_job(job);
    let at = egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, theme::INK);
}

/// The address field: a pill with the site glyph, the address, and its own
/// buttons. Editing (egui focus or the OSK typing here) swaps those buttons for
/// a clear button and shows the raw URL.
fn add_address_field(
    ui: &mut egui::Ui,
    state: &mut std::cell::RefMut<'_, BrowserState>,
    commands: &mut Vec<AppCommand>,
    inputs: &ToolbarInputs,
) {
    let id = egui::Id::new(super::ids::LOCATION);
    let editing = ui.memory(|m| m.has_focus(id)) || inputs.osk_caret.is_some();
    let avail = ui.available_size();
    let radius = FIELD_H / 2.0;
    let mut refocus = false;
    let field = egui::Frame::new()
        .fill(ui.visuals().text_edit_bg_color())
        .stroke(ui.visuals().widgets.inactive.bg_stroke)
        .corner_radius(radius)
        .inner_margin(egui::Margin::symmetric(FIELD_MARGIN_X, 0))
        .show(ui, |ui| {
            ui.set_min_width(avail.x - 2.0 * FIELD_MARGIN_X as f32);
            ui.set_height(FIELD_H);
            // The edit and the buttons sit in *disjoint* rects: an icon overlaid
            // on the text edit raced it for the hit-test.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if editing {
                    if !state.location.is_empty()
                        && add_field_button(ui, icon(bold::X_CIRCLE), false).clicked()
                    {
                        state.location.clear();
                        // The click took focus off the edit; hand it back.
                        refocus = true;
                    }
                } else {
                    let star = match inputs.bookmarked {
                        true => icon_fill(fill::STAR),
                        false => icon(bold::STAR),
                    };
                    if add_field_button(ui, star, inputs.bookmarked).clicked() {
                        commands.push(AppCommand::ToggleBookmark);
                    }
                    if add_field_button(ui, icon(bold::BOOK_OPEN), false).clicked() {
                        commands.push(AppCommand::Browser(BrowserCommand::Reader));
                    }
                    // Shown only off the config default, ahead of the icons so
                    // they never shift; clicking resets.
                    if let Some(pct) = inputs.zoom_pct {
                        if add_zoom_pill(ui, pct).clicked() {
                            commands.push(AppCommand::Browser(BrowserCommand::Zoom(0)));
                        }
                    }
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let (glyph, color) = site_glyph(state.page_url(), editing);
                    ui.label(icon(glyph).size(FIELD_ICON).color(color));
                    if let Some(pos) = inputs.osk_caret {
                        super::park_caret(ui.ctx(), id, pos, state.location.chars().count());
                    }
                    let char_count = state.location.chars().count();
                    let hint = egui::RichText::new(FIELD_HINT).color(theme::MUTED);
                    let mut edit = new_text_edit(&mut state.location, super::ids::LOCATION)
                        .frame(egui::Frame::new())
                        .hint_text(hint);
                    // At rest the address is painted over the edit, not by it.
                    if !editing {
                        edit = edit.text_color(egui::Color32::TRANSPARENT);
                    }
                    let location = ui.add_sized(ui.available_size(), edit);
                    if !editing {
                        paint_display_address(ui, location.rect, &state.location);
                    }
                    // Focus selects the URL, so typing replaces it; skipped when
                    // the OSK owns the caret.
                    if location.gained_focus() && inputs.osk_caret.is_none() {
                        super::select_all(ui.ctx(), id, char_count);
                    }
                    if is_key_pressed(ui, location.clone(), egui::Key::Enter) {
                        commands.push(AppCommand::Browser(BrowserCommand::Load));
                    }
                });
            });
        });
    if refocus {
        ui.memory_mut(|m| m.request_focus(id));
    }
    // The accent ring while editing, matching egui's own TextEdit.
    if editing {
        ui.painter().rect_stroke(
            field.response.rect,
            radius,
            ui.visuals().selection.stroke,
            egui::StrokeKind::Inside,
        );
    }
}

/// A frameless button painting a rounded square with the tab count inside, like
/// a phone browser's. Painted rather than labelled, so the square reads as an
/// icon and not a selection.
fn add_tabs_button(ui: &mut egui::Ui, count: usize) -> egui::Response {
    // Scaled to match a glyph of [`ICON`].
    let scale = ICON / theme::ICON_SIZE;
    let (rect, resp) = ui.allocate_exact_size(
        Vec2 {
            x: 22.0 * scale,
            y: SLOT,
        },
        egui::Sense::click(),
    );
    let color = ui.style().interact(&resp).fg_stroke.color;
    let painter = ui.painter();

    // Snap the center onto a half-pixel so the 1px stroke lands on whole pixels.
    let c = rect.center().floor() + egui::vec2(0.5, 0.5);
    let half = 6.5 * scale; // square half-size
    let square = egui::Rect::from_center_size(c, Vec2::splat(half * 2.0));
    painter.rect_stroke(
        square,
        2.0,
        egui::Stroke::new(1.5, color),
        egui::StrokeKind::Inside,
    );

    // Counts past 99 won't fit — cap the label rather than overflow the square.
    let label = if count > 99 {
        "99".to_string()
    } else {
        count.to_string()
    };
    painter.text(
        c,
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(scale * if count > 9 { 8.0 } else { 10.0 }),
        color,
    );
    resp
}

/// The page zoom as a pill inside the address field.
fn add_zoom_pill(ui: &mut egui::Ui, pct: u16) -> egui::Response {
    let label = egui::RichText::new(format!("{pct}%")).size(ZOOM_FONT);
    let pill = egui::Button::new(label)
        .fill(ui.visuals().widgets.inactive.weak_bg_fill)
        .corner_radius(ZOOM_PILL_H / 2.0)
        .min_size(Vec2 {
            x: 0.0,
            y: ZOOM_PILL_H,
        });
    ui.add(pill)
}

/// "Update available" chip: a painted accent dot (can't tofu). Brightens on hover.
fn add_update_dot(ui: &mut egui::Ui) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2 { x: SLOT, y: SLOT }, egui::Sense::click());
    let color = theme::ACCENT;
    let color = if resp.hovered() {
        color.gamma_multiply(1.25)
    } else {
        color
    };
    // Half-pixel center keeps the dot's edge crisp and symmetric.
    let c = rect.center().floor() + egui::vec2(0.5, 0.5);
    ui.painter().circle_filled(c, 4.5, color);
    resp
}

#[inline]
fn is_key_pressed(ui: &mut egui::Ui, response: egui::Response, key: egui::Key) -> bool {
    response.lost_focus() && ui.input(|i| i.key_pressed(key))
}

/// What one toolbar frame shows, snapshotted by the caller — one bundle, so
/// the panel and overlay spellings can't drift argument by argument.
pub(super) struct ToolbarInputs {
    pub bookmarked: bool,
    pub tab_count: usize,
    /// Downloads still in flight; a count chip that jumps to the section.
    pub active_downloads: usize,
    /// A newer build was found; an "Update" chip that opens Settings->About.
    pub update_available: bool,
    /// Page zoom percent; `None` at the config default, where no chip shows.
    pub zoom_pct: Option<u16>,
    /// The OSK's caret, when it types here; egui's caret is parked on it.
    pub osk_caret: Option<OskCaret>,
    pub position: ToolbarPosition,
}

/// The toolbar row — nav buttons, address-bar field, and the menu/tab/download
/// chips — laid out left-to-right. Shared by the space-reserving panel
/// ([`add_toolbar`]) and the auto-hide overlay ([`add_toolbar_overlay`]).
fn toolbar_contents(
    ui: &mut egui::Ui,
    state: &mut std::cell::RefMut<'_, BrowserState>,
    commands: &mut Vec<AppCommand>,
    inputs: &ToolbarInputs,
) {
    // Height 0 so the row sizes to its content — the panel measures it,
    // and the overlay's `Area` has no bounded height to center within.
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_size().x, 0.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_height(ROW_H);
            // In the corner, the easiest place for a thumb to find.
            if ui.add(new_toolbar_button(icon(bold::HOUSE))).clicked() {
                commands.push(AppCommand::Menu(MenuAction::OpenUrl(
                    crate::browser::HOME_URL.to_string(),
                )));
            }
            if ui.add(new_toolbar_button(icon(bold::ARROW_LEFT))).clicked() {
                commands.push(AppCommand::Browser(BrowserCommand::Back));
            }
            if ui
                .add(new_toolbar_button(icon(bold::ARROW_RIGHT)))
                .clicked()
            {
                commands.push(AppCommand::Browser(BrowserCommand::Forward));
            }

            // Disabled while loading: servo's WebView exposes no stop(). Always
            // the same Button widget — another kind churns egui's id.
            let loading = state.is_loading();
            let glyph = if loading {
                bold::X
            } else {
                bold::ARROW_CLOCKWISE
            };
            if ui
                .add_enabled(!loading, new_toolbar_button(icon(glyph)))
                .clicked()
            {
                commands.push(AppCommand::Browser(BrowserCommand::Reload));
            }

            ui.add_space(2.0);
            // The chips sit at the right edge; the address bar fills the gap
            // between them and the navigation buttons.
            ui.allocate_ui_with_layout(
                ui.available_size(),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    // A freshly allocated ui does not inherit the row's floor.
                    ui.set_min_height(ROW_H);
                    if inputs.update_available && add_update_dot(ui).clicked() {
                        commands.push(AppCommand::Settings(SettingsAction::Open));
                        commands.push(AppCommand::Settings(SettingsAction::SetSection(
                            SettingsSection::About,
                        )));
                    }
                    if ui.add(new_toolbar_button(icon(bold::LIST))).clicked() {
                        commands.push(AppCommand::Menu(MenuAction::Open));
                    }
                    if ui.add(new_toolbar_button(icon(bold::GEAR))).clicked() {
                        commands.push(AppCommand::Settings(SettingsAction::Open));
                    }
                    if inputs.active_downloads > 0 {
                        let label = format!("{}{}", bold::DOWNLOAD_SIMPLE, inputs.active_downloads);
                        if ui.add(new_toolbar_button(icon(&label))).clicked() {
                            commands.push(AppCommand::Menu(MenuAction::Open));
                            commands
                                .push(AppCommand::Menu(MenuAction::SetSection(Section::Downloads)));
                        }
                    }
                    // Shown even at one tab, so the slot never moves.
                    if add_tabs_button(ui, inputs.tab_count).clicked() {
                        commands.push(AppCommand::Menu(MenuAction::Open));
                        commands.push(AppCommand::Menu(MenuAction::SetSection(Section::Tabs)));
                    }
                    add_address_field(ui, state, commands, inputs);
                },
            );
        },
    );
}

/// Thickness of the loading edge (logical px).
const LOADING_EDGE: f32 = 2.0;

/// Accent line along the toolbar's page-facing edge while the tab loads: static,
/// so unlike a spinner it repaints only when the load status flips. Painted into
/// the bar's own `layer`, so an overlay covering the toolbar covers it too.
fn paint_loading_edge(
    ctx: &egui::Context,
    layer: egui::LayerId,
    bar: egui::Rect,
    position: ToolbarPosition,
) {
    let top = match position {
        ToolbarPosition::Top => bar.bottom() - LOADING_EDGE,
        ToolbarPosition::Bottom => bar.top(),
    };
    let edge = egui::Rect::from_min_size(
        egui::pos2(bar.left(), top),
        Vec2 {
            x: bar.width(),
            y: LOADING_EDGE,
        },
    );
    ctx.layer_painter(layer)
        .rect_filled(edge, 0.0, theme::ACCENT);
}

/// Draw the toolbar as a space-reserving panel anchored to `position`'s edge
/// (the central panel takes whatever's left). Returns the panel's screen rect.
pub(super) fn add_toolbar(
    ui: &mut egui::Ui,
    state: &mut std::cell::RefMut<'_, BrowserState>,
    commands: &mut Vec<AppCommand>,
    inputs: &ToolbarInputs,
) -> egui::Rect {
    let frame = egui::Frame::default()
        .fill(ui.style().visuals.window_fill)
        .inner_margin(4.0);
    let panel = match inputs.position {
        ToolbarPosition::Top => egui::Panel::top("toolbar"),
        ToolbarPosition::Bottom => egui::Panel::bottom("toolbar"),
    };
    let response = panel
        .frame(frame)
        .show(ui, |ui| toolbar_contents(ui, state, commands, inputs))
        .response;
    let rect = response.rect;
    if state.is_loading() {
        paint_loading_edge(ui.ctx(), response.layer_id, rect, inputs.position);
    }
    rect
}

/// Draw the toolbar as a floating overlay pinned to `position`'s edge — for
/// auto-hide, where the web view stays full-height and the caller skips this
/// call to hide the bar. Returns the bar's screen rect.
pub(super) fn add_toolbar_overlay(
    ctx: &egui::Context,
    width: f32,
    state: &mut std::cell::RefMut<'_, BrowserState>,
    commands: &mut Vec<AppCommand>,
    inputs: &ToolbarInputs,
) -> egui::Rect {
    let frame = egui::Frame::default()
        .fill(ctx.global_style().visuals.window_fill)
        .inner_margin(4.0);
    let align = match inputs.position {
        ToolbarPosition::Top => egui::Align2::CENTER_TOP,
        ToolbarPosition::Bottom => egui::Align2::CENTER_BOTTOM,
    };
    let area = egui::Area::new(egui::Id::new("toolbar_overlay"))
        .order(egui::Order::Foreground)
        .anchor(align, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.set_width(width);
            frame
                .show(ui, |ui| toolbar_contents(ui, state, commands, inputs))
                .response
                .rect
        });
    let rect = area.inner;
    if state.is_loading() {
        paint_loading_edge(ctx, area.response.layer_id, rect, inputs.position);
    }
    rect
}

#[cfg(test)]
mod tests {
    use super::display_address;

    fn shown(location: &str) -> (String, String) {
        display_address(location)
    }

    #[test]
    fn the_host_drops_scheme_and_www() {
        let (host, rest) = shown("https://www.example.com/a/b?q=1#top");
        assert_eq!(
            (host.as_str(), rest.as_str()),
            ("example.com", "/a/b?q=1#top")
        );
    }

    #[test]
    fn a_bare_root_reads_as_the_host_alone() {
        assert_eq!(
            shown("http://example.com/"),
            ("example.com".into(), String::new())
        );
        let (host, _) = shown("http://localhost:8080/x");
        assert_eq!(host, "localhost:8080");
    }

    /// A half-typed draft or a non-web scheme must not be rewritten.
    #[test]
    fn anything_else_reads_as_written() {
        assert_eq!(shown("some search"), ("some search".into(), String::new()));
        let data = "data:text/html,hi";
        assert_eq!(shown(data), (data.into(), String::new()));
    }

    #[test]
    fn the_start_page_reads_as_nothing() {
        assert_eq!(
            shown(crate::browser::HOME_URL),
            (String::new(), String::new())
        );
    }
}
