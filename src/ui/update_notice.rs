//! Rendering of the update notice (state lives in
//! [`crate::overlay::update_notice`]): a centered card over a dimmed screen. Its
//! header names the offered build against the running one; the body below follows
//! the live [`UpdateState`], from the release summary through download and install
//! to the quit that applies it.

use super::theme::{self, ACCENT, DIM, SCRIM, WARN};
use crate::command::{AppCommand, UpdateNoticeAction};
use crate::data::downloads::{format_progress, format_size};
use crate::overlay::update_notice::{choices, Choice, UpdateNotice};
use crate::update::{Offer, UpdateState};
use egui_phosphor::bold;
use egui_sdl2::egui;

const BUTTON: egui::Vec2 = egui::vec2(110.0, 28.0);
/// The card's content width at most; narrower screens keep SCREEN_MARGIN a side.
const CARD_W: f32 = 400.0;
const SCREEN_MARGIN: f32 = 32.0;
const TITLE_FONT: f32 = 17.0;
const BADGE_SIZE: f32 = 30.0;
/// Rows of release summary shown before it is cut with an ellipsis.
const SUMMARY_ROWS: usize = 3;
/// A nightly version's suffix ahead of its `YYYYMMDD` build date.
const NIGHTLY_SUFFIX: &str = "-nightly.";
/// Where the month and the day sit in that date.
const MONTH_DIGITS: std::ops::Range<usize> = 4..6;
const DAY_DIGITS: std::ops::Range<usize> = 6..8;
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The notice's layer, shared by its dimming so the two need no ordering.
fn notice_layer() -> egui::LayerId {
    egui::LayerId::new(egui::Order::Foreground, egui::Id::new("update_notice"))
}

/// Draw the notice for the updater's current `state`.
pub(super) fn add_update_notice(
    ctx: &egui::Context,
    notice: &UpdateNotice,
    state: &UpdateState,
    commands: &mut Vec<AppCommand>,
) {
    let Some(version) = notice.version() else {
        return;
    };
    // Above the other Foreground overlays, which egui stacks by last interaction.
    ctx.move_to_top(notice_layer());
    let screen = ctx.content_rect();
    ctx.layer_painter(notice_layer())
        .rect_filled(screen, 0.0, SCRIM);
    let width = (screen.width() - 2.0 * SCREEN_MARGIN).min(CARD_W);

    egui::Area::new(egui::Id::new("update_notice"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            theme::card_frame().show(ui, |ui| {
                ui.set_width(width);
                add_header(ui, version, state);
                ui.separator();
                add_status(ui, state);
                ui.add_space(10.0);
                add_buttons(ui, notice, state, commands);
            });
        });
}

/// Draw the header: the offered build over the running one, and the download
/// size of an in-place install.
fn add_header(ui: &mut egui::Ui, version: &str, state: &UpdateState) {
    ui.horizontal(|ui| {
        ui.label(
            theme::icon(bold::ARROW_CIRCLE_UP)
                .size(BADGE_SIZE)
                .color(ACCENT),
        );
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(format!("retsurf {}", display_version(version)))
                    .size(TITLE_FONT)
                    .color(egui::Color32::WHITE)
                    .strong(),
            );
            let current = display_version(env!("RETSURF_VERSION"));
            ui.label(egui::RichText::new(format!("You have {current}")).color(DIM));
        });
        if let UpdateState::Available {
            offer: Offer::Install { size, .. },
            ..
        } = state
        {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                ui.label(egui::RichText::new(format_size(*size)).color(DIM));
            });
        }
    });
}

/// Draw where the update stands: the release summary, the download, or the outcome.
fn add_status(ui: &mut egui::Ui, state: &UpdateState) {
    match state {
        UpdateState::Available { notes, .. } => {
            if let Some(summary) = notes.as_deref().and_then(summary) {
                let format = egui::TextFormat {
                    font_id: egui::TextStyle::Body.resolve(ui.style()),
                    color: ui.visuals().text_color(),
                    ..Default::default()
                };
                let mut job = egui::text::LayoutJob::single_section(summary, format);
                job.wrap.max_rows = SUMMARY_ROWS;
                ui.label(job);
            }
        }
        UpdateState::Downloading { received, total } => {
            let fraction = match *total {
                0 => 0.0,
                total => *received as f32 / total as f32,
            };
            ui.add(egui::ProgressBar::new(fraction).text(format_progress(*received, *total)));
        }
        UpdateState::Checking => {
            ui.label(egui::RichText::new("Checking for updates...").color(DIM));
        }
        UpdateState::Installing => {
            ui.label(egui::RichText::new("Installing...").color(DIM));
        }
        UpdateState::Installed { .. } => {
            ui.label(egui::RichText::new("Installed. Quit to apply.").color(egui::Color32::WHITE));
        }
        UpdateState::Error(e) => {
            ui.label(egui::RichText::new(format!("Update failed: {e}")).color(WARN));
        }
        UpdateState::UpToDate { .. } => {
            ui.label(egui::RichText::new("Up to date").color(DIM));
        }
        UpdateState::Idle => {}
    }
}

/// Draw the buttons for `state`, right-aligned in their left-to-right order.
fn add_buttons(
    ui: &mut egui::Ui,
    notice: &UpdateNotice,
    state: &UpdateState,
    commands: &mut Vec<AppCommand>,
) {
    let selected = notice.selected(state);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        for &choice in choices(state).iter().rev() {
            let button = egui::Button::selectable(
                selected == choice,
                egui::RichText::new(label(choice)).color(egui::Color32::WHITE),
            );
            if ui.add_sized(BUTTON, button).clicked() {
                commands.push(AppCommand::UpdateNotice(UpdateNoticeAction::Choose(choice)));
            }
        }
    });
}

fn label(choice: Choice) -> &'static str {
    match choice {
        Choice::Install => "Install",
        Choice::Open => "Open page",
        Choice::Retry => "Retry",
        Choice::Quit => "Quit",
        Choice::Later => "Later",
        Choice::Hide => "Hide",
    }
}

/// The release notes' opening paragraph on one line; `None` when they are blank.
fn summary(notes: &str) -> Option<String> {
    let notes = notes.replace('\r', "");
    let paragraph = notes.trim().split("\n\n").next()?;
    let line = paragraph.split_whitespace().collect::<Vec<_>>().join(" ");
    (!line.is_empty()).then_some(line)
}

/// `version` for display: a nightly as its build date, others as given.
fn display_version(version: &str) -> String {
    nightly_date(version).unwrap_or_else(|| version.to_string())
}

/// A nightly version's build date as month and day; `None` for any other.
fn nightly_date(version: &str) -> Option<String> {
    let (_, date) = version.split_once(NIGHTLY_SUFFIX)?;
    let month: usize = date.get(MONTH_DIGITS)?.parse().ok()?;
    let day: u32 = date.get(DAY_DIGITS)?.parse().ok()?;
    let month = MONTHS.get(month.checked_sub(1)?)?;
    Some(format!("Nightly {month} {day}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_is_the_opening_paragraph() {
        let notes = "Game scaling and shaders,\r\nand Quick Access.\r\n\r\n**Added:**\r\n- a";
        assert_eq!(
            summary(notes).as_deref(),
            Some("Game scaling and shaders, and Quick Access.")
        );
        assert_eq!(summary("  \n "), None);
    }

    #[test]
    fn nightly_shows_its_build_date() {
        assert_eq!(display_version("0.10.0-nightly.20261025"), "Nightly Oct 25");
        assert_eq!(display_version("0.11.0"), "0.11.0");
        assert_eq!(display_version("0.11.0-beta.1"), "0.11.0-beta.1");
        assert_eq!(display_version("nightly abc1234"), "nightly abc1234");
    }
}
