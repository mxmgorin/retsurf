//! Rendering of the edge strips (state lives in [`crate::overlay::quick_access`]):
//! Quick Access at the right edge and Quick Menu at the left, each full height
//! with no backdrop dim, so the page or game beside it shows what a quick row
//! just changed. Up/Down move, A / Left/Right act on the focused row, B or a tap
//! beside the strip closes.

use crate::command::{AppCommand, QuickAccessAction};
use crate::event::bindings::Action;
use crate::overlay::menu::Section;
use crate::overlay::quick_access::{Entry, QuickAccess, Strip};
use crate::overlay::settings::Settings;
use crate::ui::panel::{self, center_selected, ROW_GAP};
use crate::ui::theme::{self, ACCENT, ROW_FONT};
use egui_phosphor::bold;
use egui_sdl2::egui;

/// The strip's width where the screen has room for it; a narrower screen gets
/// [`MAX_SHARE`] of its width.
const STRIP_W: f32 = 280.0;

/// The most of the screen's width the strip may cover.
const MAX_SHARE: f32 = 0.5;

/// Opacity of the strip's fill: enough to read rows over any page.
const STRIP_OPACITY: f32 = 0.92;

pub(in crate::ui) fn add_quick_access(
    ctx: &egui::Context,
    panel: &QuickAccess,
    commands: &mut Vec<AppCommand>,
) {
    let screen = ctx.content_rect();
    let frame = theme::card_frame()
        .fill(theme::PANEL_FILL.gamma_multiply(STRIP_OPACITY))
        .corner_radius(0.0);
    let margin = frame.inner_margin.sum();
    let outer = STRIP_W.min(screen.width() * MAX_SHARE);
    let width = outer - margin.x;
    // Each on the side of the button that opens it.
    let (left, title) = match panel.strip() {
        Strip::QuickAccess => (screen.right() - outer, "QUICK ACCESS"),
        Strip::QuickMenu => (screen.left(), "QUICK MENU"),
    };
    add_backdrop(ctx, screen, commands);
    egui::Area::new(egui::Id::new("quick_access"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(left, screen.top()))
        // A fit shift would feed back into the list's height bound below.
        .constrain(false)
        .show(ctx, |ui| {
            frame.show(ui, |ui| {
                ui.set_width(width);
                ui.set_min_height(screen.height() - margin.y);
                add_header(ui, title);
                ui.spacing_mut().item_spacing.y = ROW_GAP;
                // The area auto-sizes, so the list scrolls only under a set bound.
                let max_h = screen.bottom() - frame.inner_margin.bottom as f32 - ui.cursor().top();
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .max_height(max_h.max(0.0))
                    .show(ui, |ui| add_rows(ui, width, panel, commands));
            });
        });
}

fn add_rows(ui: &mut egui::Ui, width: f32, panel: &QuickAccess, commands: &mut Vec<AppCommand>) {
    for (index, &entry) in panel.rows().iter().enumerate() {
        let selected = index == panel.selected();
        let label = format!("{}  {}", glyph(entry), panel.label(index));
        let value = match selected && entry.steps() {
            true => format!(
                "{} {} {}",
                bold::CARET_LEFT,
                panel.value(index),
                bold::CARET_RIGHT
            ),
            false => panel.value(index).to_string(),
        };
        let resp = panel::named_row(ui, width, selected, &label, &value);
        if selected {
            center_selected(&resp);
        }
        if resp.clicked() {
            commands.push(AppCommand::QuickAccess(QuickAccessAction::Click(index)));
        }
    }
}

/// An unpainted layer under the strip: a tap beside it closes the strip and
/// never reaches the page.
fn add_backdrop(ctx: &egui::Context, screen: egui::Rect, commands: &mut Vec<AppCommand>) {
    egui::Area::new(egui::Id::new("quick_access_backdrop"))
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .constrain(false)
        .show(ctx, |ui| {
            let resp = ui.allocate_response(screen.size(), egui::Sense::click());
            if resp.clicked() {
                commands.push(AppCommand::QuickAccess(QuickAccessAction::Close));
            }
        });
}

/// The strip's title, one per strip in either mode: the rows say which is on.
fn add_header(ui: &mut egui::Ui, title: &str) {
    ui.label(
        egui::RichText::new(title)
            .color(ACCENT)
            .size(ROW_FONT)
            .strong(),
    );
    ui.add_space(ROW_GAP * 2.0);
}

/// What a row with no icon of its own shows; the tests keep every row off it.
const NO_ICON: &str = bold::DOT;

/// A row's icon. A quick row goes by its settings row's label, since the
/// strips share that row rather than a copy of it.
fn glyph(entry: Entry) -> &'static str {
    match entry {
        Entry::Enter => bold::GAME_CONTROLLER,
        Entry::Exit => bold::SIGN_OUT,
        Entry::InputMap => bold::JOYSTICK,
        Entry::Osk => bold::KEYBOARD,
        Entry::Settings => bold::GEAR,
        Entry::Quit => bold::POWER,
        Entry::Run(Action::Reader) => bold::BOOK_OPEN,
        Entry::Run(Action::Bookmark) => bold::STAR,
        Entry::Run(Action::Home) => bold::HOUSE,
        Entry::Run(_) => NO_ICON,
        Entry::List(Section::Tabs) => bold::TABS,
        Entry::List(Section::Bookmarks) => bold::BOOKMARKS,
        Entry::List(Section::History) => bold::CLOCK_COUNTER_CLOCKWISE,
        Entry::List(Section::Downloads) => bold::DOWNLOAD_SIMPLE,
        Entry::Quick(field) => match Settings::fields()[field].quick_label() {
            "Scaling" => bold::FRAME_CORNERS,
            "Shader" => bold::TELEVISION_SIMPLE,
            "Page theme" => bold::CIRCLE_HALF,
            "Ad blocker" => bold::SHIELD_CHECK,
            "User agent" => bold::DEVICES,
            _ => NO_ICON,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::TabMode;
    use crate::config::AppConfig;

    /// A row added to either strip, or a newly flagged quick row, must pick
    /// an icon of its own.
    #[test]
    fn every_row_has_an_icon() {
        let mut panel = QuickAccess::new();
        for strip in [Strip::QuickAccess, Strip::QuickMenu] {
            for mode in [TabMode::Page, TabMode::Reader, TabMode::Game] {
                panel.open(strip, mode, &AppConfig::default());
                for &row in panel.rows() {
                    assert_ne!(glyph(row), NO_ICON, "{row:?}");
                }
            }
        }
    }
}
