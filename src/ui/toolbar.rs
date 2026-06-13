//! The top toolbar: navigation buttons, the address bar, bookmark toggle, and
//! the chips that jump into menu sections (tab position, active downloads).

use crate::app::{AppCommand, MenuAction, SettingsAction};
use crate::browser::{BrowserCommand, BrowserState};
use crate::overlay::menu::Section;
use egui_sdl2::egui::{self, Vec2};

/// Create a frameless button with square sizing, as used in the toolbar.
#[inline]
fn new_toolbar_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(text)
        .frame(false)
        .min_size(Vec2 { x: 20.0, y: 20.0 })
}

#[inline]
fn new_text_edit<'a>(text: &'a mut String, id: &str) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(text).id(egui::Id::new(id))
}

/// A frameless toolbar button painting a house *silhouette* (egui's fonts have
/// no monochrome house glyph). Solid shapes — a filled roof triangle and body
/// with the door cut back out in the toolbar's background — read crisply at icon
/// size, where thin outlines look broken. Brightens on hover; returns its click
/// response.
fn add_home_button(ui: &mut egui::Ui) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2 { x: 22.0, y: 20.0 }, egui::Sense::click());
    let color = ui.style().interact(&resp).fg_stroke.color;
    let bg = ui.style().visuals.window_fill;
    let painter = ui.painter();

    // Snap the center onto a half-pixel: every offset below is a half (±5.5
    // body, ±1.5 door), so a half-pixel center lands all those edges on whole
    // pixels — crisp and symmetric. (Rounding the center to a whole pixel did
    // the opposite, feathering the door's narrow cut and reading as off-center.)
    let c = rect.center().floor() + egui::vec2(0.5, 0.5);
    let half = 5.5; // house half-width / half-height
    let (left, right) = (c.x - half, c.x + half);
    let (top, bottom) = (c.y - half, c.y + half);
    let eaves = c.y - half * 0.30; // where the roof meets the walls

    // The whole house as one filled polygon (apex → eaves → base), so the roof
    // and body share no seam between them.
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(c.x, top),
            egui::pos2(right, eaves),
            egui::pos2(right, bottom),
            egui::pos2(left, bottom),
            egui::pos2(left, eaves),
        ],
        color,
        egui::Stroke::NONE,
    ));
    // Door: cut back out in the background color.
    let dw = 3.0;
    painter.rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(c.x - dw / 2.0, bottom - 4.0),
            egui::pos2(c.x + dw / 2.0, bottom),
        ),
        0.0,
        bg,
    );
    resp
}

#[inline]
fn is_key_pressed(ui: &mut egui::Ui, response: egui::Response, key: egui::Key) -> bool {
    response.lost_focus() && ui.input(|i| i.key_pressed(key))
}

#[inline]
pub(super) fn add_toolbar(
    ui: &mut egui::Ui,
    state: &mut std::cell::RefMut<'_, BrowserState>,
    commands: &mut Vec<AppCommand>,
    bookmarked: bool,
    // 1-based active tab index and total tab count, e.g. `(2, 3)` → "2/3".
    tab_pos: (usize, usize),
    // Downloads still in flight; shown as a `⬇N` chip that jumps to the section.
    active_downloads: usize,
    // Active tab's page zoom percent when off the config default (chip hidden at it).
    zoom_pct: Option<u16>,
) {
    let frame = egui::Frame::default()
        .fill(ui.style().visuals.window_fill)
        .inner_margin(4.0);
    egui::Panel::top("toolbar")
        .frame(frame)
        .show_inside(ui, |ui| {
            ui.allocate_ui_with_layout(
                ui.available_size(),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    if ui.add(new_toolbar_button("←")).clicked() {
                        commands.push(AppCommand::Browser(BrowserCommand::Back));
                    }
                    if ui.add(new_toolbar_button("→")).clicked() {
                        commands.push(AppCommand::Browser(BrowserCommand::Foward));
                    }

                    // Reload, disabled (greyed, non-interactive) while loading —
                    // servo's WebView exposes no stop()/cancel, so there's nothing
                    // to click mid-load. Always the SAME Button widget: toggling
                    // enabledness keeps egui's widget id stable for this slot, where
                    // swapping to a different widget kind churned the id between
                    // passes and tripped the red id-clash outline. Static on purpose
                    // — an animated spinner would force continuous repaints, which we
                    // avoid on handheld hardware.
                    // ↻ rendered a touch below the 13.0 default so it reads lighter
                    // than the arrows; the button keeps the 20×20 footprint so the
                    // slot (and its widget id) is unchanged.
                    let reload = egui::Button::new(egui::RichText::new("↻").size(11.0))
                        .frame(false)
                        .min_size(Vec2 { x: 20.0, y: 20.0 });
                    if ui.add_enabled(!state.is_loading(), reload).clicked() {
                        commands.push(AppCommand::Browser(BrowserCommand::Reload));
                    }

                    // Navigate the active tab to the built-in start page.
                    if add_home_button(ui).clicked() {
                        commands.push(AppCommand::Menu(MenuAction::OpenUrl(
                            crate::browser::HOME_URL.to_string(),
                        )));
                    }

                    ui.add_space(2.0);
                    // The bookmark icons sit at the right edge; the address bar fills
                    // the gap between them and the navigation buttons. ★ toggles the
                    // current page (filled when saved); ☰ opens the menu.
                    ui.allocate_ui_with_layout(
                        ui.available_size(),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if ui.add(new_toolbar_button("☰")).clicked() {
                                commands.push(AppCommand::Menu(MenuAction::Open));
                            }
                            // ⚙ U+2699 (in egui's emoji-icon-font, like ☰) opens
                            // the settings overlay.
                            if ui.add(new_toolbar_button("⚙")).clicked() {
                                commands.push(AppCommand::Settings(SettingsAction::Open));
                            }
                            // ⬇ U+2B07 (not ↓ U+2193): egui's default fonts lack the
                            // plain arrow, only the emoji one renders.
                            if active_downloads > 0 {
                                let label = format!("⬇{active_downloads}");
                                if ui.add(new_toolbar_button(&label)).clicked() {
                                    commands.push(AppCommand::Menu(MenuAction::Open));
                                    commands.push(AppCommand::Menu(MenuAction::SetSection(
                                        Section::Downloads,
                                    )));
                                }
                            }
                            // Active tab position, bracketed (e.g. "[2/3]") beside the
                            // menu button — a full border would read as a selection.
                            // Shown only with multiple tabs; clicking it opens the
                            // menu's Tabs section (like the ⬇ chip for downloads).
                            if tab_pos.1 > 1 {
                                let label = format!("[{}/{}]", tab_pos.0, tab_pos.1);
                                if ui.add(new_toolbar_button(&label)).clicked() {
                                    commands.push(AppCommand::Menu(MenuAction::Open));
                                    commands.push(AppCommand::Menu(MenuAction::SetSection(
                                        Section::Tabs,
                                    )));
                                }
                            }
                            // Page-zoom chip (e.g. "125%"), shown only while the
                            // active tab is off the config default; clicking resets.
                            if let Some(pct) = zoom_pct {
                                let label = format!("{pct}%");
                                if ui.add(new_toolbar_button(&label)).clicked() {
                                    commands.push(AppCommand::Browser(BrowserCommand::Zoom(0)));
                                }
                            }
                            if ui
                                .add(new_toolbar_button(if bookmarked { "★" } else { "☆" }))
                                .clicked()
                            {
                                commands.push(AppCommand::ToggleBookmark);
                            }
                            // 🖹 "document with text" — the page-with-lines reader
                            // glyph; lives in egui's emoji-icon-font (cmap-checked;
                            // most other reader-ish glyphs are tofu).
                            if ui.add(new_toolbar_button("🖹")).clicked() {
                                commands.push(AppCommand::Browser(BrowserCommand::Reader));
                            }
                            let location = ui.add_sized(
                                ui.available_size(),
                                new_text_edit(state.get_location_mut(), "location"),
                            );
                            if is_key_pressed(ui, location, egui::Key::Enter) {
                                commands.push(AppCommand::Browser(BrowserCommand::Load));
                            }
                        },
                    );
                },
            );
        });
}
