//! Rendering of the startup update notice (state lives in
//! [`crate::overlay::update_notice`]): a centered card over a dimmed screen.

use super::theme::{self, DIM, SCRIM};
use crate::command::{AppCommand, UpdateNoticeAction};
use crate::config::FaceLabels;
use crate::overlay::update_notice::{Choice, UpdateNotice};
use egui_sdl2::egui;

const BUTTON: egui::Vec2 = egui::vec2(110.0, 28.0);

/// The notice's layer, shared by its dimming so the two need no ordering.
fn notice_layer() -> egui::LayerId {
    egui::LayerId::new(egui::Order::Foreground, egui::Id::new("update_notice"))
}

/// Draw the notice; each button wears its pad shortcut.
pub(super) fn add_update_notice(
    ctx: &egui::Context,
    notice: &UpdateNotice,
    face: FaceLabels,
    commands: &mut Vec<AppCommand>,
) {
    let Some(version) = notice.version() else {
        return;
    };
    // Above the other Foreground overlays, which egui stacks by last interaction.
    ctx.move_to_top(notice_layer());
    ctx.layer_painter(notice_layer())
        .rect_filled(ctx.content_rect(), 0.0, SCRIM);

    egui::Area::new(egui::Id::new("update_notice"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            theme::card_frame().show(ui, |ui| {
                ui.label(
                    egui::RichText::new("Update available")
                        .color(egui::Color32::WHITE)
                        .strong(),
                );
                ui.label(egui::RichText::new(format!("retsurf {version}")).color(DIM));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let buttons = [
                        (Choice::Update, format!("{} Update", face.a)),
                        (Choice::Later, format!("{} Later", face.b)),
                    ];
                    for (choice, label) in buttons {
                        let button = egui::Button::selectable(
                            notice.selected() == choice,
                            egui::RichText::new(label).color(egui::Color32::WHITE),
                        );
                        if ui.add_sized(BUTTON, button).clicked() {
                            commands
                                .push(AppCommand::UpdateNotice(UpdateNoticeAction::Choose(choice)));
                        }
                    }
                });
            });
        });
}
