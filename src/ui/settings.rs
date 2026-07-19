//! Rendering of the full-screen settings overlay (state lives in
//! [`crate::overlay::settings`]): a section bar (Browser · Display · Input ·
//! Content · Advanced) like the menu's, over the active section's field rows —
//! each showing its label and current value. Gamepad / keyboard: L1/R1 switch
//! section, ▲▼ move, ◀▶ adjust the focused value, A toggle / cycle / edit text,
//! B save & close; the mouse can click a tab, a row, its ◀▶ step buttons, or
//! Close. All of it works without an analog stick.

use super::theme::{close_button, ACCENT, CLOSE_SIZE, DIM, PANEL_FILL, ROW_FONT};
use crate::app::{AppCommand, SettingsAction};
use crate::data::downloads::format_size;
use crate::overlay::settings::{CtrlRow, Settings, SettingsSection};
use crate::update::{Offer, UpdateState};
use egui_sdl2::egui;

/// Rows and the section bar share the menu's metrics (height, radius, font, gap)
/// so the two full-screen overlays feel like one chrome.
const ROW_H: f32 = 30.0;
const ROW_RADIUS: f32 = 6.0;
const ROW_GAP: f32 = 4.0;
const PAD_X: f32 = 30.0;
const PAD_Y: f32 = 16.0;
const SIDES: f32 = PAD_X * 2.0;
/// The square ◀▶ step buttons trailing a numeric row.
const STEP_W: f32 = 26.0;

/// A selectable row showing `label` on the left and `value` (in the accent) on
/// the right, the value pushed to the trailing edge by a grow atom — same shape
/// as the menu's rows so the cursor highlight reads identically.
fn setting_row(
    ui: &mut egui::Ui,
    width: f32,
    selected: bool,
    label: String,
    value: String,
) -> egui::Response {
    let label = egui::RichText::new(label)
        .color(egui::Color32::WHITE)
        .size(ROW_FONT);
    let value = egui::RichText::new(value).color(ACCENT).size(ROW_FONT);
    ui.add_sized(
        [width, ROW_H],
        egui::Button::selectable(selected, (label, egui::Atom::grow(), value))
            .corner_radius(ROW_RADIUS)
            .truncate(),
    )
}

/// A ◀ / ▶ step button for a numeric row: accent on the focused row, dim
/// otherwise, sized to line up with the menu's trailing action buttons.
fn step_button(
    ui: &mut egui::Ui,
    glyph: &str,
    selected: bool,
    dim: egui::Color32,
) -> egui::Response {
    let color = if selected { ACCENT } else { dim };
    ui.add_sized(
        [STEP_W, ROW_H],
        egui::Button::new(egui::RichText::new(glyph).color(color)).corner_radius(ROW_RADIUS),
    )
}

/// One read-only `label : value` line on the About tab — label in white, value
/// in the accent, pushed to the trailing edge so the values line up like the
/// field rows do (but without the selectable button chrome).
fn info_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .color(egui::Color32::WHITE)
                .size(ROW_FONT),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(value).color(ACCENT).size(ROW_FONT));
        });
    });
}

/// The [`SettingsAction`] the About tab's update row triggers on A / click for the
/// current `state`, or `None` while a check/download/install is in progress. Shared
/// by the renderer and the gamepad activation path ([`super::AppUi::about_activate`])
/// so the two never drift.
pub(super) fn update_command(state: &UpdateState) -> Option<SettingsAction> {
    match state {
        UpdateState::Idle | UpdateState::UpToDate { .. } | UpdateState::Error(_) => {
            Some(SettingsAction::CheckUpdate)
        }
        UpdateState::Available {
            offer: Offer::Install { .. },
            ..
        } => Some(SettingsAction::InstallUpdate),
        UpdateState::Available {
            offer: Offer::Open { page },
            ..
        } => Some(SettingsAction::OpenLink(page.clone())),
        UpdateState::Installed { .. } => Some(SettingsAction::QuitForUpdate),
        UpdateState::Checking | UpdateState::Downloading { .. } | UpdateState::Installing => None,
    }
}

/// `(label, value)` for the About tab's single update row, folding any status into
/// one line so the row stays one stable gamepad-focus target across states.
fn update_row_text(state: &UpdateState) -> (String, String) {
    match state {
        UpdateState::Idle => ("Check for updates".to_string(), String::new()),
        UpdateState::Checking => ("Checking for updates".to_string(), "...".to_string()),
        UpdateState::UpToDate { current } => {
            (format!("Up to date ({current})"), "Check again".to_string())
        }
        UpdateState::Available {
            version,
            offer: Offer::Install { size, .. },
        } => (format!("Install {version}"), format_size(*size)),
        UpdateState::Available {
            version,
            offer: Offer::Open { .. },
        } => (format!("Download {version}"), "Open page".to_string()),
        UpdateState::Downloading { received, total } => {
            let value = if *total > 0 {
                format!(
                    "{}%  ·  {} / {}",
                    received * 100 / total,
                    format_size(*received),
                    format_size(*total)
                )
            } else {
                format_size(*received)
            };
            ("Downloading update".to_string(), value)
        }
        UpdateState::Installing => ("Installing update".to_string(), "...".to_string()),
        UpdateState::Installed { version } => {
            (format!("Update ready ({version})"), "Quit to apply".to_string())
        }
        UpdateState::Error(e) => (format!("Update failed: {e}"), "Retry".to_string()),
    }
}

/// Render the self-update block on the About tab: a header and one selectable row
/// (About focus index 0). Its label/action depend on the update state; gamepad A
/// goes through [`super::AppUi::about_activate`], a click pushes the same command.
/// Shown on every platform — in-place install where supported, else a "Download"
/// that opens the release page.
fn add_update(
    ui: &mut egui::Ui,
    full_w: f32,
    selected: bool,
    update: &UpdateState,
    commands: &mut Vec<AppCommand>,
) {
    ui.add_space(10.0);
    ui.label(
        egui::RichText::new("Updates")
            .color(ACCENT)
            .strong()
            .size(13.0),
    );
    let (label, value) = update_row_text(update);
    let resp = setting_row(ui, full_w, selected, label, value);
    if selected {
        resp.scroll_to_me(Some(egui::Align::Center));
    }
    if resp.clicked() {
        if let Some(action) = update_command(update) {
            commands.push(AppCommand::Settings(action));
        }
    }
}

/// Render the read-only About tab (pulls its facts from
/// [`crate::overlay::settings::about_info`]): the build identity, a table of
/// resolved component versions, the attribution block, and clickable links.
fn add_about(
    ui: &mut egui::Ui,
    screen: egui::Rect,
    dim: egui::Color32,
    sel: usize,
    update: &UpdateState,
    commands: &mut Vec<AppCommand>,
) {
    let info = crate::overlay::settings::about_info();
    let full_w = screen.width() - SIDES;
    let max_h = (screen.bottom() - PAD_Y - ui.cursor().top()).max(0.0);
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .max_height(max_h)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = ROW_GAP;

            ui.label(
                egui::RichText::new(format!("retsurf {}", info.version))
                    .color(egui::Color32::WHITE)
                    .strong()
                    .size(20.0),
            );
            for line in info.description {
                ui.label(egui::RichText::new(*line).color(dim).size(13.0));
            }
            ui.add_space(10.0);

            info_row(ui, "Build", info.git_hash);
            info_row(ui, "Date", info.build_date);

            // Self-update block — About focus row 0.
            add_update(ui, full_w, sel == 0, update, commands);

            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("Components")
                    .color(ACCENT)
                    .strong()
                    .size(13.0),
            );
            for (name, version) in info.components {
                info_row(ui, name, version);
            }

            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("Credits")
                    .color(ACCENT)
                    .strong()
                    .size(13.0),
            );
            for line in info.credits {
                ui.label(egui::RichText::new(*line).color(dim).size(13.0));
            }

            ui.add_space(10.0);
            ui.label(
                egui::RichText::new("Links")
                    .color(ACCENT)
                    .strong()
                    .size(13.0),
            );
            // Links are About focus rows 1.., rendered selectable so the gamepad
            // can highlight and open them (A -> OpenLink); the scheme is stripped
            // to read as a link, shown in the accent like the field-row values.
            for (i, (label, url)) in info.links.iter().enumerate() {
                let selected = sel == 1 + i;
                let shown = url
                    .trim_start_matches("https://")
                    .trim_start_matches("http://");
                let resp = setting_row(ui, full_w, selected, label.to_string(), shown.to_string());
                if selected {
                    resp.scroll_to_me(Some(egui::Align::Center));
                }
                if resp.clicked() {
                    commands.push(AppCommand::Settings(SettingsAction::OpenLink(url.to_string())));
                }
            }
        });
}

/// Render the dynamic Controls section: per action, a header then a selectable
/// row for each existing binding (gamepad / keyboard) and an "add" row, plus the
/// two reset rows. A on a binding removes it, A on "add" starts capture (press a
/// button or key), A on a reset restores defaults. State lives in
/// [`crate::overlay::settings::Settings`].
fn add_controls(
    ui: &mut egui::Ui,
    settings: &Settings,
    screen: egui::Rect,
    commands: &mut Vec<AppCommand>,
) {
    let rows = settings.controls_rows();
    let sel = settings.selected();
    let capturing = settings.capturing_action();
    let full_w = screen.width() - SIDES;
    let max_h = (screen.bottom() - PAD_Y - ui.cursor().top()).max(0.0);
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .max_height(max_h)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = ROW_GAP;
            for (i, row) in rows.iter().enumerate() {
                // A header is a label, not a row; everything else is a selectable
                // `label : value` line pushed through one click path.
                let (label, value) = match row {
                    CtrlRow::Header(name) => {
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(*name).color(ACCENT).strong().size(13.0));
                        continue;
                    }
                    CtrlRow::Binding {
                        gesture, keyboard, ..
                    } => {
                        let device = if *keyboard { "Keyboard" } else { "Gamepad" };
                        (format!("    {device}"), gesture.clone())
                    }
                    CtrlRow::Add(action) => {
                        let listening = capturing == Some(*action);
                        let value = if listening {
                            "press a button or key..."
                        } else {
                            ""
                        };
                        ("    + Add binding".to_string(), value.to_string())
                    }
                    CtrlRow::GamepadReset => {
                        ("Restore gamepad defaults".to_string(), String::new())
                    }
                    CtrlRow::KeyboardReset => {
                        ("Restore keyboard defaults".to_string(), String::new())
                    }
                };

                let selected = i == sel;
                let resp = setting_row(ui, full_w, selected, label, value);
                if selected {
                    resp.scroll_to_me(Some(egui::Align::Center));
                }
                // Clicking focuses the row and activates it (add / remove / reset).
                if resp.clicked() {
                    commands.push(AppCommand::Settings(SettingsAction::Select(i)));
                    commands.push(AppCommand::Settings(SettingsAction::Activate));
                }
            }
        });
}

/// Draw the full-screen settings overlay: a dark panel with the section bar, the
/// close ✖, a one-line control hint, and the active section's field list. See
/// the module docs for the controls.
pub(super) fn add_settings(
    ctx: &egui::Context,
    settings: &Settings,
    update: &UpdateState,
    commands: &mut Vec<AppCommand>,
) {
    let screen = ctx.content_rect();
    let dim = DIM;
    egui::Area::new(egui::Id::new("settings"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        // Pin top-left and fill the screen exactly (see the menu for why egui must
        // not be allowed to shift the panel to "fit").
        .constrain(false)
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(PANEL_FILL)
                .inner_margin(egui::Margin::symmetric(PAD_X as i8, PAD_Y as i8))
                .show(ui, |ui| {
                    ui.set_min_size(screen.size() - egui::vec2(SIDES, PAD_Y * 2.0));

                    // Mouse-only ✖ close pinned to the top-right (B closes on the
                    // gamepad); both save the draft on the way out.
                    let close_rect = egui::Rect::from_min_size(
                        egui::pos2(screen.right() - PAD_X - CLOSE_SIZE, screen.top() + PAD_Y),
                        egui::vec2(CLOSE_SIZE, CLOSE_SIZE),
                    );
                    if close_button(ui, close_rect, egui::Id::new("settings_close")).clicked() {
                        commands.push(AppCommand::Settings(SettingsAction::Close));
                    }

                    // Section bar (active tab highlighted), mirroring the menu's.
                    let active = settings.section();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        for section in SettingsSection::ALL {
                            let tab = egui::Button::selectable(
                                section == active,
                                egui::RichText::new(section.label())
                                    .color(egui::Color32::WHITE)
                                    .size(ROW_FONT),
                            )
                            .corner_radius(ROW_RADIUS)
                            .min_size(egui::vec2(0.0, 28.0));
                            if ui.add(tab).clicked() {
                                commands.push(AppCommand::Settings(SettingsAction::SetSection(
                                    section,
                                )));
                            }
                        }
                    });
                    let hint = if settings.capturing() {
                        "Press a button or key to bind      Esc cancel"
                    } else if settings.is_info_section() {
                        "L1/R1 section   ⏶⏷ move   A select   B close"
                    } else if settings.is_controls_section() {
                        "L1/R1 section   ⏶⏷ move   A add / remove   B save & close"
                    } else {
                        "L1/R1 section   ⏶⏷ move   ⏴⏵ adjust   A edit   B save & close      * needs restart"
                    };
                    ui.label(egui::RichText::new(hint).color(dim));
                    ui.add_space(8.0);

                    // The About tab is read-only info, not a field list — render
                    // it and stop (it has no FIELDS to iterate).
                    if settings.is_info_section() {
                        add_about(ui, screen, dim, settings.selected(), update, commands);
                        return;
                    }

                    // The Controls section is a dynamic action list (gamepad +
                    // keyboard bindings), not FIELDS — render it and stop.
                    if settings.is_controls_section() {
                        add_controls(ui, settings, screen, commands);
                        return;
                    }

                    // The active section's rows. A sub-header (the field's `cat`)
                    // is shown only in sections that fold several config groups
                    // together — Content, Advanced — where the tab name alone
                    // wouldn't say which group a row belongs to.
                    let rows: Vec<(usize, &_)> = Settings::fields()
                        .iter()
                        .enumerate()
                        .filter(|(_, f)| f.section == active)
                        .collect();
                    let multi_cat = rows.iter().any(|(_, f)| f.cat != rows[0].1.cat);

                    let full_w = screen.width() - SIDES;
                    let num_w = full_w - 2.0 * STEP_W - 8.0;

                    // The Area auto-sizes to its content, so the scroll area has no
                    // bounded height of its own — cap it to the space left down to
                    // the screen's bottom margin and disable auto-shrink so a long
                    // section (Input) actually scrolls.
                    let max_h = (screen.bottom() - PAD_Y - ui.cursor().top()).max(0.0);
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .max_height(max_h)
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = ROW_GAP;
                            let mut last_cat = "";
                            for (i, field) in rows {
                                if multi_cat && field.cat != last_cat {
                                    last_cat = field.cat;
                                    ui.add_space(6.0);
                                    ui.label(
                                        egui::RichText::new(field.cat)
                                            .color(ACCENT)
                                            .strong()
                                            .size(13.0),
                                    );
                                }

                                let selected = i == settings.selected();
                                let label = if field.restart {
                                    format!("{} *", field.label)
                                } else {
                                    field.label.to_string()
                                };
                                let value = settings.value_str(i);
                                let steppable = settings.is_steppable(i);

                                ui.horizontal(|ui| {
                                    let row_w = if steppable { num_w } else { full_w };
                                    let resp = setting_row(ui, row_w, selected, label, value);
                                    // Keep the focused row in view — there's no
                                    // cursor to drag the scrollbar.
                                    if selected {
                                        resp.scroll_to_me(Some(egui::Align::Center));
                                    }
                                    // Clicking a row focuses it and activates
                                    // (toggle / cycle / step, or open the OSK on a
                                    // text row).
                                    if resp.clicked() {
                                        commands
                                            .push(AppCommand::Settings(SettingsAction::Select(i)));
                                        commands
                                            .push(AppCommand::Settings(SettingsAction::Activate));
                                    }
                                    if steppable {
                                        if step_button(ui, "⏴", selected, dim).clicked() {
                                            commands.push(AppCommand::Settings(
                                                SettingsAction::Select(i),
                                            ));
                                            commands.push(AppCommand::Settings(
                                                SettingsAction::Adjust(-1),
                                            ));
                                        }
                                        if step_button(ui, "⏵", selected, dim).clicked() {
                                            commands.push(AppCommand::Settings(
                                                SettingsAction::Select(i),
                                            ));
                                            commands.push(AppCommand::Settings(
                                                SettingsAction::Adjust(1),
                                            ));
                                        }
                                    }
                                });
                            }
                        });
                });
        });
}
