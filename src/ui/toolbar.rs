//! The toolbar (top or bottom, per the display config): navigation buttons, the
//! address bar, bookmark toggle, and the chips that jump into menu sections.

use super::theme;
use crate::browser::{BrowserCommand, BrowserState};
use crate::command::{AppCommand, MenuAction, SettingsAction};
use crate::config::ToolbarPosition;
use crate::overlay::menu::Section;
use crate::overlay::settings::SettingsSection;
use egui_phosphor::{bold, fill};
use egui_sdl2::egui::{self, Vec2};

/// Side of a toolbar icon slot (logical px).
const SLOT: f32 = 20.0;

/// The address-bar frame's inner margin: across, then down.
const FIELD_MARGIN_X: i8 = 4;
const FIELD_MARGIN_Y: i8 = 2;

/// Row height every item centers against: `Align::Center` only knows the height
/// laid out so far, and the tallest item (the field) comes last.
const ROW_H: f32 = SLOT + 2.0 * FIELD_MARGIN_Y as f32;

/// Create a frameless button with square sizing, as used in the toolbar. Takes
/// icon text from [`theme::icon`] as readily as a plain label (the zoom chip).
#[inline]
fn new_toolbar_button<'a>(text: impl egui::IntoAtoms<'a>) -> egui::Button<'a> {
    egui::Button::new(text)
        .frame(false)
        .min_size(Vec2 { x: SLOT, y: SLOT })
}

/// Vertically centered: egui puts a TextEdit's text at the top of its box, and
/// this box is the icon slot's height, not the text's.
#[inline]
fn new_text_edit<'a>(text: &'a mut String, id: &str) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(text)
        .id(egui::Id::new(id))
        .vertical_align(egui::Align::Center)
}

/// A frameless button painting a rounded square with the tab count inside, like
/// a phone browser's. Painted rather than labelled, so the square reads as an
/// icon and not a selection.
fn add_tabs_button(ui: &mut egui::Ui, count: usize) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2 { x: 22.0, y: SLOT }, egui::Sense::click());
    let color = ui.style().interact(&resp).fg_stroke.color;
    let painter = ui.painter();

    // Snap the center onto a half-pixel so the 1px stroke lands on whole pixels.
    let c = rect.center().floor() + egui::vec2(0.5, 0.5);
    let half = 6.5; // square half-size
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
        egui::FontId::proportional(if count > 9 { 8.0 } else { 10.0 }),
        color,
    );
    resp
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
    pub osk_caret: Option<usize>,
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
            if ui
                .add(new_toolbar_button(theme::icon(bold::ARROW_LEFT)))
                .clicked()
            {
                commands.push(AppCommand::Browser(BrowserCommand::Back));
            }
            if ui
                .add(new_toolbar_button(theme::icon(bold::ARROW_RIGHT)))
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
                .add_enabled(!loading, new_toolbar_button(theme::icon(glyph)))
                .clicked()
            {
                commands.push(AppCommand::Browser(BrowserCommand::Reload));
            }

            if ui
                .add(new_toolbar_button(theme::icon(bold::HOUSE)))
                .clicked()
            {
                commands.push(AppCommand::Menu(MenuAction::OpenUrl(
                    crate::browser::HOME_URL.to_string(),
                )));
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
                    if ui
                        .add(new_toolbar_button(theme::icon(bold::LIST)))
                        .clicked()
                    {
                        commands.push(AppCommand::Menu(MenuAction::Open));
                    }
                    if ui
                        .add(new_toolbar_button(theme::icon(bold::GEAR)))
                        .clicked()
                    {
                        commands.push(AppCommand::Settings(SettingsAction::Open));
                    }
                    if inputs.active_downloads > 0 {
                        let label = format!("{}{}", bold::DOWNLOAD_SIMPLE, inputs.active_downloads);
                        if ui.add(new_toolbar_button(theme::icon(&label))).clicked() {
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
                    // Shown only off the config default; clicking resets.
                    if let Some(pct) = inputs.zoom_pct {
                        let label = format!("{pct}%");
                        if ui.add(new_toolbar_button(label)).clicked() {
                            commands.push(AppCommand::Browser(BrowserCommand::Zoom(0)));
                        }
                    }
                    let star = if inputs.bookmarked {
                        theme::icon_fill(fill::STAR)
                    } else {
                        theme::icon(bold::STAR)
                    };
                    if ui.add(new_toolbar_button(star)).clicked() {
                        commands.push(AppCommand::ToggleBookmark);
                    }
                    // The edit and the reader toggle sit in *disjoint* rects: an
                    // icon overlaid on the text edit raced it for the hit-test.
                    let avail = ui.available_size();
                    let radius = ui.visuals().widgets.inactive.corner_radius;
                    let field = egui::Frame::new()
                        .fill(ui.visuals().text_edit_bg_color())
                        .stroke(ui.visuals().widgets.inactive.bg_stroke)
                        .corner_radius(radius)
                        .inner_margin(egui::Margin::symmetric(FIELD_MARGIN_X, FIELD_MARGIN_Y))
                        .show(ui, |ui| {
                            // Span the gap; the height stays one text row.
                            ui.set_min_width(avail.x - 2.0 * FIELD_MARGIN_X as f32);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui
                                    .add(new_toolbar_button(theme::icon(bold::BOOK_OPEN)))
                                    .clicked()
                                {
                                    commands.push(AppCommand::Browser(BrowserCommand::Reader));
                                }
                                if let Some(pos) = inputs.osk_caret {
                                    super::park_caret(
                                        ui.ctx(),
                                        egui::Id::new(super::ids::LOCATION),
                                        pos,
                                        state.location.chars().count(),
                                    );
                                }
                                let char_count = state.location.chars().count();
                                let location = ui.add_sized(
                                    ui.available_size(),
                                    new_text_edit(&mut state.location, super::ids::LOCATION)
                                        .frame(egui::Frame::new()),
                                );
                                // Focus selects the URL, so typing replaces it;
                                // skipped when the OSK owns the caret.
                                if location.gained_focus() && inputs.osk_caret.is_none() {
                                    super::select_all(
                                        ui.ctx(),
                                        egui::Id::new(super::ids::LOCATION),
                                        char_count,
                                    );
                                }
                                if is_key_pressed(ui, location.clone(), egui::Key::Enter) {
                                    commands.push(AppCommand::Browser(BrowserCommand::Load));
                                }
                                location.has_focus()
                            })
                            .inner
                        });
                    // The accent ring on focus, matching egui's own TextEdit.
                    if field.inner {
                        ui.painter().rect_stroke(
                            field.response.rect,
                            radius,
                            ui.visuals().selection.stroke,
                            egui::StrokeKind::Inside,
                        );
                    }
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
