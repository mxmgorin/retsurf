//! Rendering of the full-screen settings overlay (state lives in
//! [`crate::overlay::settings`]): a section bar like the menu's over the active
//! section's field rows. L1/R1 switch section, Up/Down move, Left/Right adjust,
//! A edit, B save & close — all of it without an analog stick.

use super::panel::{self, section_scroll, ROW_GAP, ROW_RADIUS, SIDES};
use super::theme::{ACCENT, DIM, ROW_FONT, WARN};
use crate::app::{AppCommand, SettingsAction};
use crate::data::downloads::format_size;
use crate::overlay::settings::{Settings, SettingsSection, RESET_ROWS};
use crate::update::{Offer, UpdateState};
use egui_sdl2::egui;
use inputbind::editor::{Bound, Row};
use inputbind::Action as _;

/// The reset rows, in the order the Controls list counts them.
const RESETS: [&str; RESET_ROWS] = ["Restore gamepad defaults", "Restore keyboard defaults"];

/// Row height, tighter than the menu's so the long field lists fit.
const ROW_H: f32 = 30.0;
/// The square step buttons trailing a numeric row.
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

/// A left/right step button for a numeric row, accent on the focused row.
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
            offer: Offer::Open,
            page,
            ..
        } => page.clone().map(SettingsAction::OpenLink),
        UpdateState::Installed { .. } => Some(SettingsAction::QuitForUpdate),
        UpdateState::Checking | UpdateState::Downloading { .. } | UpdateState::Installing => None,
    }
}

/// The release page to link to ("View release notes on GitHub") when an update is
/// available and carries one. `None` otherwise — the CI channel and non-available
/// states have nothing to link to. Keeps the About focus nav, the renderer, and
/// [`super::AppUi::about_activate`] agreeing on whether the link row exists.
pub(super) fn release_link(state: &UpdateState) -> Option<String> {
    match state {
        UpdateState::Available {
            page: Some(page), ..
        } => Some(page.clone()),
        _ => None,
    }
}

/// How many gamepad-focusable rows the update block contributes to the About tab:
/// the action row, plus a "View release notes" link row when [`release_link`] exists.
pub(super) fn update_row_count(state: &UpdateState) -> usize {
    1 + release_link(state).is_some() as usize
}

/// Trim release notes to a compact preview for the About tab (the full text is one
/// tap away via the "View on GitHub" link): drop CRs, cap the length, and add an
/// ellipsis on truncation.
fn notes_preview(body: &str) -> String {
    const MAX: usize = 600;
    let body = body.replace('\r', "");
    let body = body.trim();
    if body.len() <= MAX {
        return body.to_string();
    }
    let mut end = MAX;
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", body[..end].trim_end())
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
            ..
        } => (format!("Install {version}"), format_size(*size)),
        UpdateState::Available {
            version,
            offer: Offer::Open,
            ..
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
        UpdateState::Installed { version } => (
            format!("Update ready ({version})"),
            "Quit to apply".to_string(),
        ),
        UpdateState::Error(e) => (format!("Update failed: {e}"), "Retry".to_string()),
    }
}

/// Render the self-update block on the About tab and return how many focusable rows
/// it drew (see [`update_row_count`]): a header, the selectable action row (About
/// focus index 0), and — when an update is available — the release notes (read-only)
/// followed by a "View release notes on GitHub" link row (focus index 1). Its
/// label/action depend on the update state; gamepad A goes through
/// [`super::AppUi::about_activate`], a click pushes the same command. Shown on every
/// platform — in-place install where supported, else a "Download" that opens the page.
fn add_update(
    ui: &mut egui::Ui,
    full_w: f32,
    sel: usize,
    update: &UpdateState,
    commands: &mut Vec<AppCommand>,
) -> usize {
    ui.add_space(10.0);
    ui.label(
        egui::RichText::new("Updates")
            .color(ACCENT)
            .strong()
            .size(13.0),
    );

    // Row 0: the primary action (check / install / download / quit-to-apply / retry).
    let (label, value) = update_row_text(update);
    let resp = setting_row(ui, full_w, sel == 0, label, value);
    if sel == 0 {
        resp.scroll_to_me(Some(egui::Align::Center));
    }
    if resp.clicked() {
        if let Some(action) = update_command(update) {
            commands.push(AppCommand::Settings(action));
        }
    }

    // When an update is available: its notes (read-only), then a link row that opens
    // the release page on GitHub (About focus index 1). The notes text isn't a focus
    // target, so it doesn't shift the row indices.
    if let UpdateState::Available {
        notes: Some(body), ..
    } = update
    {
        ui.label(
            egui::RichText::new(notes_preview(body))
                .color(DIM)
                .size(12.0),
        );
    }
    if let Some(page) = release_link(update) {
        let selected = sel == 1;
        let resp = setting_row(
            ui,
            full_w,
            selected,
            "View release notes on GitHub".to_string(),
            "Open page".to_string(),
        );
        if selected {
            resp.scroll_to_me(Some(egui::Align::Center));
        }
        if resp.clicked() {
            commands.push(AppCommand::Settings(SettingsAction::OpenLink(page)));
        }
    }

    update_row_count(update)
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
    section_scroll(ui, screen).show(ui, |ui| {
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

        // Self-update block: About focus rows 0..; returns its row count so the
        // links below can offset past it.
        let update_rows = add_update(ui, full_w, sel, update, commands);

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
        // Links are About focus rows `update_rows..`, selectable so the gamepad can
        // open them (A -> OpenLink); the scheme is stripped to read as a link.
        for (i, (label, url)) in info.links.iter().enumerate() {
            let selected = sel == update_rows + i;
            let shown = url
                .trim_start_matches("https://")
                .trim_start_matches("http://");
            let resp = setting_row(ui, full_w, selected, label.to_string(), shown.to_string());
            if selected {
                resp.scroll_to_me(Some(egui::Align::Center));
            }
            if resp.clicked() {
                commands.push(AppCommand::Settings(SettingsAction::OpenLink(
                    url.to_string(),
                )));
            }
        }
    });
}

/// Focus and act in one click, keeping the focused row scrolled into view.
fn control_row(
    ui: &mut egui::Ui,
    width: f32,
    index: usize,
    selected: usize,
    label: String,
    value: String,
    commands: &mut Vec<AppCommand>,
) {
    let focused = index == selected;
    let resp = setting_row(ui, width, focused, label, value);
    if focused {
        resp.scroll_to_me(Some(egui::Align::Center));
    }
    if resp.clicked() {
        commands.push(AppCommand::Settings(SettingsAction::Select(index)));
        commands.push(AppCommand::Settings(SettingsAction::Activate));
    }
}

fn gesture_summary(gestures: &[Bound]) -> String {
    if gestures.is_empty() {
        return "unbound".to_string();
    }
    gestures
        .iter()
        .map(|b| {
            if b.suppressed {
                format!("{} (off)", b.text)
            } else {
                b.text.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("  ")
}

/// A line per action, and for the open one a row per gesture plus its Add row.
/// State lives in [`crate::overlay::settings::Settings`].
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
    section_scroll(ui, screen).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        for (i, row) in rows.iter().enumerate() {
            // A group is a label; every other row goes through one click path.
            let (label, value) = match row {
                Row::Group(name) => {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(*name).color(ACCENT).strong().size(13.0));
                    continue;
                }
                Row::Command {
                    action,
                    gestures,
                    open,
                } => {
                    // The glyphs the section hint uses, so egui's fonts have them.
                    let marker = if *open { "⏷" } else { "⏵" };
                    (
                        format!("{marker} {}", action.display()),
                        gesture_summary(gestures),
                    )
                }
                Row::Gesture { text, source } => {
                    (format!("      {}", source.label()), text.clone())
                }
                Row::Suppressed { text, surface } => {
                    (format!("      {surface}"), format!("{text} (off)"))
                }
                Row::Add(action) => {
                    let value = if capturing == Some(*action) {
                        "press a button or key..."
                    } else {
                        ""
                    };
                    ("      + Add".to_string(), value.to_string())
                }
            };
            control_row(ui, full_w, i, sel, label, value, commands);
        }
        for (offset, label) in RESETS.iter().enumerate() {
            let index = rows.len() + offset;
            control_row(
                ui,
                full_w,
                index,
                sel,
                (*label).to_string(),
                String::new(),
                commands,
            );
        }
    });
}

/// Draw the settings overlay: the section bar, a control hint, and the active
/// section's field list. See the module docs for the controls.
pub(super) fn add_settings(
    ctx: &egui::Context,
    settings: &Settings,
    update: &UpdateState,
    commands: &mut Vec<AppCommand>,
) {
    let screen = ctx.content_rect();
    let dim = DIM;
    let closed = panel::panel(ctx, "settings", screen, |ui| {
        let active = settings.section();
        let clicked = panel::section_bar(
            ui,
            screen,
            SettingsSection::ALL,
            active,
            SettingsSection::label,
            |_| {},
        );
        if let Some(section) = clicked {
            commands.push(AppCommand::Settings(SettingsAction::SetSection(section)));
        }
        let hint = if settings.capturing() {
            "Press a button or key to bind      Esc cancel"
        } else if settings.is_info_section() {
            "L1/R1 section   ⏶⏷ move   A select   B close"
        } else if settings.is_controls_section() {
            "L1/R1 section   ⏶⏷ move   A open / bind / remove   B save & close"
        } else {
            "L1/R1 section   ⏶⏷ move   ⏴⏵ adjust   A edit   B save & close      * needs restart"
        };
        ui.label(egui::RichText::new(hint).color(dim));
        if let Some(note) = settings.controls_note() {
            ui.label(egui::RichText::new(note).color(WARN));
        }
        ui.add_space(8.0);

        // The About tab is read-only info, not a FIELDS list.
        if settings.is_info_section() {
            add_about(ui, screen, dim, settings.selected(), update, commands);
            return;
        }

        // Controls is a dynamic binding list, not FIELDS.
        if settings.is_controls_section() {
            add_controls(ui, settings, screen, commands);
            return;
        }

        // Sub-headers (the field's `cat`) only where a section folds several
        // config groups together — the tab name alone wouldn't say which.
        let rows: Vec<(usize, &_)> = Settings::fields()
            .iter()
            .enumerate()
            .filter(|(_, f)| f.section == active)
            .collect();
        let multi_cat = rows.iter().any(|(_, f)| f.cat != rows[0].1.cat);

        let full_w = screen.width() - SIDES;
        let num_w = full_w - 2.0 * STEP_W - 8.0;

        section_scroll(ui, screen).show(ui, |ui| {
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
                    // Keep the focused row in view — no cursor to drag the bar.
                    if selected {
                        resp.scroll_to_me(Some(egui::Align::Center));
                    }
                    // Clicking focuses and activates (or opens the OSK).
                    if resp.clicked() {
                        commands.push(AppCommand::Settings(SettingsAction::Select(i)));
                        commands.push(AppCommand::Settings(SettingsAction::Activate));
                    }
                    if steppable {
                        if step_button(ui, "⏴", selected, dim).clicked() {
                            commands.push(AppCommand::Settings(SettingsAction::Select(i)));
                            commands.push(AppCommand::Settings(SettingsAction::Adjust(-1)));
                        }
                        if step_button(ui, "⏵", selected, dim).clicked() {
                            commands.push(AppCommand::Settings(SettingsAction::Select(i)));
                            commands.push(AppCommand::Settings(SettingsAction::Adjust(1)));
                        }
                    }
                });
            }
        });
    });
    // Both close paths (B and the corner button) save the draft on the way out.
    if closed {
        commands.push(AppCommand::Settings(SettingsAction::Close));
    }
}

#[cfg(test)]
mod tests {
    use super::notes_preview;

    /// A short body is returned trimmed, verbatim (no ellipsis).
    #[test]
    fn short_body_is_verbatim() {
        assert_eq!(notes_preview("  ## Fixes\n- a bug  "), "## Fixes\n- a bug");
    }

    /// Carriage returns are stripped so CRLF release notes don't render blank lines.
    #[test]
    fn strips_carriage_returns() {
        assert_eq!(notes_preview("line1\r\nline2"), "line1\nline2");
    }

    /// A body past the cap is truncated with a trailing ellipsis.
    #[test]
    fn long_body_is_truncated() {
        let out = notes_preview(&"a".repeat(1000));
        assert!(out.ends_with("..."));
        assert!(out.len() < 1000);
    }

    /// Truncation lands on a char boundary even when the cap falls mid-codepoint —
    /// the multi-byte walk-back must not panic or split a `char`.
    #[test]
    fn truncation_respects_char_boundary() {
        // 598 ASCII + 3-byte chars puts byte 600 inside a codepoint.
        let body = format!("{}{}", "a".repeat(598), "\u{65e5}".repeat(20));
        let out = notes_preview(&body);
        assert!(out.ends_with("..."));
        // Round-trips as valid UTF-8 (would have panicked on a bad split).
        assert!(out.chars().count() > 0);
    }
}
