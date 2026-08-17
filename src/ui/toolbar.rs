//! The toolbar (top or bottom, per the display config): navigation buttons, the
//! address bar, bookmark toggle, and the chips that jump into menu sections (tab
//! count, active downloads).

use super::theme;
use crate::app::{AppCommand, MenuAction, SettingsAction};
use crate::browser::{BrowserCommand, BrowserState};
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
/// laid out so far, and the tallest item (the field) comes last. Keep it equal to
/// the field's own height.
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

/// A frameless toolbar button painting a rounded square outline with the tab
/// count centered inside — like a phone browser's tab counter. Drawn (rather
/// than a bracketed label) so the square reads as an icon, not a selection.
/// Brightens on hover; returns its click response.
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

/// The toolbar row — nav buttons, address-bar field, and the menu/tab/download
/// chips — laid out left-to-right. Shared by the space-reserving panel
/// ([`add_toolbar`]) and the auto-hide overlay ([`add_toolbar_overlay`]).
#[allow(clippy::too_many_arguments)]
fn toolbar_contents(
    ui: &mut egui::Ui,
    state: &mut std::cell::RefMut<'_, BrowserState>,
    commands: &mut Vec<AppCommand>,
    bookmarked: bool,
    // Open tabs, shown in the tab chip.
    tab_count: usize,
    // Downloads still in flight; shown as a download-icon + count chip that jumps
    // to the section.
    active_downloads: usize,
    // A newer build was found; shown as an "Update" chip that opens Settings->About.
    update_available: bool,
    // Active tab's page zoom percent when off the config default (chip hidden at it).
    zoom_pct: Option<u16>,
    // When the OSK types into the address bar, its caret position — park egui's
    // caret here so it tracks the external edit (it won't follow on its own).
    osk_caret: Option<usize>,
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

            // Reload, disabled (greyed, non-interactive) while loading —
            // servo's WebView exposes no stop()/cancel, so there's nothing
            // to click mid-load. While loading it also swaps to an X (muted by
            // the disabled state) to read as "can't reload yet" rather than a
            // live reload affordance. Always the SAME Button widget, only its
            // label changes: toggling enabledness/text keeps egui's widget id
            // stable for this slot, where swapping to a different widget kind
            // churned the id between passes and tripped the red id-clash
            // outline. Static on purpose — an animated spinner would force
            // continuous repaints, which we avoid on handheld hardware.
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

            // Navigate the active tab to the built-in start page.
            if ui
                .add(new_toolbar_button(theme::icon(bold::HOUSE)))
                .clicked()
            {
                commands.push(AppCommand::Menu(MenuAction::OpenUrl(
                    crate::browser::HOME_URL.to_string(),
                )));
            }

            ui.add_space(2.0);
            // The bookmark icons sit at the right edge; the address bar fills
            // the gap between them and the navigation buttons. The star toggles
            // the current page (filled when saved); the list opens the menu.
            ui.allocate_ui_with_layout(
                ui.available_size(),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    // A freshly allocated ui does not inherit the row's floor.
                    ui.set_min_height(ROW_H);
                    // Update chip: far-right accent dot when a newer build is found; opens Settings -> About.
                    if update_available && add_update_dot(ui).clicked() {
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
                    if active_downloads > 0 {
                        let label = format!("{}{active_downloads}", bold::DOWNLOAD_SIMPLE);
                        if ui.add(new_toolbar_button(theme::icon(&label))).clicked() {
                            commands.push(AppCommand::Menu(MenuAction::Open));
                            commands
                                .push(AppCommand::Menu(MenuAction::SetSection(Section::Downloads)));
                        }
                    }
                    // Tab counter: a square icon with the total tab count
                    // inside, beside the menu button. Always shown (even at
                    // "1"); clicking it opens the menu's Tabs section (like
                    // the download chip for downloads).
                    if add_tabs_button(ui, tab_count).clicked() {
                        commands.push(AppCommand::Menu(MenuAction::Open));
                        commands.push(AppCommand::Menu(MenuAction::SetSection(Section::Tabs)));
                    }
                    // Page-zoom chip (e.g. "125%"), shown only while the
                    // active tab is off the config default; clicking resets.
                    if let Some(pct) = zoom_pct {
                        let label = format!("{pct}%");
                        if ui.add(new_toolbar_button(label)).clicked() {
                            commands.push(AppCommand::Browser(BrowserCommand::Zoom(0)));
                        }
                    }
                    let star = if bookmarked {
                        theme::icon_fill(fill::STAR)
                    } else {
                        theme::icon(bold::STAR)
                    };
                    if ui.add(new_toolbar_button(star)).clicked() {
                        commands.push(AppCommand::ToggleBookmark);
                    }
                    // The address bar fills the remaining width. We draw our
                    // own field frame (styled like egui's TextEdit) holding a
                    // frameless text edit plus the reader-mode toggle at its
                    // right edge — Firefox/Safari style. The two sit in
                    // *disjoint* rects (no overlap), so the icon click is
                    // reliable; an icon overlaid on the text edit raced it for
                    // the hit-test and clicked unreliably.
                    let avail = ui.available_size();
                    let radius = ui.visuals().widgets.inactive.corner_radius;
                    let field = egui::Frame::new()
                        .fill(ui.visuals().text_edit_bg_color())
                        .stroke(ui.visuals().widgets.inactive.bg_stroke)
                        .corner_radius(radius)
                        .inner_margin(egui::Margin::symmetric(FIELD_MARGIN_X, FIELD_MARGIN_Y))
                        .show(ui, |ui| {
                            // Fill the toolbar's remaining width (minus the
                            // frame's own margins) so the bar spans the gap;
                            // height stays natural (one text row).
                            ui.set_min_width(avail.x - 2.0 * FIELD_MARGIN_X as f32);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                // Reader toggle — its own slot at the field's
                                // right edge.
                                if ui
                                    .add(new_toolbar_button(theme::icon(bold::BOOK_OPEN)))
                                    .clicked()
                                {
                                    commands.push(AppCommand::Browser(BrowserCommand::Reader));
                                }
                                if let Some(pos) = osk_caret {
                                    super::park_caret(
                                        ui.ctx(),
                                        egui::Id::new("location"),
                                        pos,
                                        state.get_location().chars().count(),
                                    );
                                }
                                let char_count = state.get_location().chars().count();
                                let location = ui.add_sized(
                                    ui.available_size(),
                                    new_text_edit(state.get_location_mut(), "location")
                                        .frame(egui::Frame::new()),
                                );
                                // Focusing the bar selects the URL, so typing
                                // replaces it. Skipped while the OSK types here:
                                // it owns the caret (parked just above).
                                if location.gained_focus() && osk_caret.is_none() {
                                    super::select_all(
                                        ui.ctx(),
                                        egui::Id::new("location"),
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
                    // Repaint the frame's ring in the accent stroke while the
                    // address bar is focused, matching egui's own TextEdit.
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

/// Accent line along the toolbar's page-facing edge while the tab loads — the
/// "busy" signal we can afford: static, so it repaints only when the load status
/// flips, unlike a spinner.
///
/// Painted into the bar's own `layer`, not a `Foreground` one of its own: an
/// overlay that covers the toolbar (menu, settings) must cover the line too.
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
    ctx.layer_painter(layer).rect_filled(edge, 0.0, theme::ACCENT);
}

/// Draw the toolbar as a space-reserving panel anchored to `position`'s edge
/// (the central panel takes whatever's left). Returns the panel's screen rect.
#[allow(clippy::too_many_arguments)]
pub(super) fn add_toolbar(
    ui: &mut egui::Ui,
    state: &mut std::cell::RefMut<'_, BrowserState>,
    commands: &mut Vec<AppCommand>,
    bookmarked: bool,
    tab_count: usize,
    active_downloads: usize,
    update_available: bool,
    zoom_pct: Option<u16>,
    osk_caret: Option<usize>,
    position: ToolbarPosition,
) -> egui::Rect {
    let frame = egui::Frame::default()
        .fill(ui.style().visuals.window_fill)
        .inner_margin(4.0);
    let panel = match position {
        ToolbarPosition::Top => egui::Panel::top("toolbar"),
        ToolbarPosition::Bottom => egui::Panel::bottom("toolbar"),
    };
    let response = panel
        .frame(frame)
        .show(ui, |ui| {
            toolbar_contents(
                ui,
                state,
                commands,
                bookmarked,
                tab_count,
                active_downloads,
                update_available,
                zoom_pct,
                osk_caret,
            )
        })
        .response;
    let rect = response.rect;
    if state.is_loading() {
        paint_loading_edge(ui.ctx(), response.layer_id, rect, position);
    }
    rect
}

/// Draw the toolbar as a floating overlay pinned to `position`'s edge — for
/// auto-hide, where the web view stays full-height (no reflow) and the bar is
/// drawn only while shown (the caller skips this call to hide it). `width`
/// spans the window. Returns the bar's screen rect.
#[allow(clippy::too_many_arguments)]
pub(super) fn add_toolbar_overlay(
    ctx: &egui::Context,
    width: f32,
    state: &mut std::cell::RefMut<'_, BrowserState>,
    commands: &mut Vec<AppCommand>,
    bookmarked: bool,
    tab_count: usize,
    active_downloads: usize,
    update_available: bool,
    zoom_pct: Option<u16>,
    osk_caret: Option<usize>,
    position: ToolbarPosition,
) -> egui::Rect {
    let frame = egui::Frame::default()
        .fill(ctx.global_style().visuals.window_fill)
        .inner_margin(4.0);
    let align = match position {
        ToolbarPosition::Top => egui::Align2::CENTER_TOP,
        ToolbarPosition::Bottom => egui::Align2::CENTER_BOTTOM,
    };
    let area = egui::Area::new(egui::Id::new("toolbar_overlay"))
        .order(egui::Order::Foreground)
        .anchor(align, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.set_width(width);
            frame
                .show(ui, |ui| {
                    toolbar_contents(
                        ui,
                        state,
                        commands,
                        bookmarked,
                        tab_count,
                        active_downloads,
                        update_available,
                        zoom_pct,
                        osk_caret,
                    )
                })
                .response
                .rect
        });
    let rect = area.inner;
    if state.is_loading() {
        paint_loading_edge(ctx, area.response.layer_id, rect, position);
    }
    rect
}
