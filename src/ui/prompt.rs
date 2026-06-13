//! Rendering of the modal page-prompt overlay (state and resolution live in
//! [`crate::overlay::prompt`]): `<select>` pickers and the JS `alert` / `confirm` /
//! `prompt` dialogs, drawn as a centered panel above everything else with the
//! page dimmed behind it.

use super::theme::ACCENT;
use crate::app::{AppCommand, PromptAction};
use crate::overlay::prompt::Prompt;
use egui_sdl2::egui;
use servo::{EmbedderControl, SelectElement, SelectElementOption, SimpleDialog};

const ROW_H: f32 = 26.0;

/// Draw the front pending control as a modal: a dimmed backdrop and a centered
/// panel. Gamepad/keyboard: ▲▼ move, A/Enter activate, B/Esc dismiss; the
/// mouse clicks rows and buttons directly.
pub(super) fn add_prompt(
    ctx: &egui::Context,
    prompt: &mut Prompt,
    osk_caret: bool,
    commands: &mut Vec<AppCommand>,
) {
    let screen = ctx.content_rect();
    // Dim what's behind so the modal reads as blocking.
    ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("prompt_dim"),
    ))
    .rect_filled(screen, 0.0, egui::Color32::from_black_alpha(140));

    // Dialog text is cloned out first so the prompt can hand out its edit
    // buffer mutably while rendering (the select arm only needs reads).
    let dialog = match prompt.front() {
        Some(EmbedderControl::SimpleDialog(d)) => {
            let (has_input, has_cancel) = match d {
                SimpleDialog::Alert(_) => (false, false),
                SimpleDialog::Confirm(_) => (false, true),
                SimpleDialog::Prompt(_) => (true, true),
            };
            Some((d.message().to_string(), has_input, has_cancel))
        }
        _ => None,
    };

    // Tooltip order puts the modal above the Foreground overlays (menu, OSK,
    // the dim layer painted above).
    egui::Area::new(egui::Id::new("prompt"))
        .order(egui::Order::Tooltip)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(egui::Color32::from_rgb(0x18, 0x18, 0x1c))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(0x55)))
                .corner_radius(10.0)
                .inner_margin(14.0)
                .show(ui, |ui| {
                    ui.set_max_width((screen.width() - 64.0).min(480.0));
                    if let Some((message, has_input, has_cancel)) = dialog {
                        add_dialog(
                            ui, screen, prompt, &message, has_input, has_cancel, osk_caret,
                            commands,
                        );
                    } else if let Some(EmbedderControl::SelectElement(select)) = prompt.front() {
                        add_select(ui, screen, prompt, select, commands);
                    }
                });
        });
}

/// The `<select>` picker: the options list (group labels and disabled options
/// shown but not focusable) and, for multi-selects, a trailing OK.
fn add_select(
    ui: &mut egui::Ui,
    screen: egui::Rect,
    prompt: &Prompt,
    select: &SelectElement,
    commands: &mut Vec<AppCommand>,
) {
    let dim = egui::Color32::from_gray(0x99);
    let multiple = select.allow_select_multiple();
    let row_w = (screen.width() - 96.0).min(448.0);
    ui.label(
        egui::RichText::new(if multiple {
            "Select options — A toggles, OK applies"
        } else {
            "Select an option"
        })
        .color(dim),
    );
    ui.add_space(6.0);

    let selected_slot = prompt.selected_slot();
    // Slot indices assigned in display order — the same flattening as
    // `slot_ids`, so they line up with the navigation.
    let mut slot = 0;
    egui::ScrollArea::vertical()
        .max_height(screen.height() * 0.6)
        .show(ui, |ui| {
            for entry in select.options() {
                match entry {
                    servo::SelectElementOptionOrOptgroup::Option(option) => {
                        add_option_row(
                            ui,
                            option,
                            row_w,
                            &mut slot,
                            selected_slot,
                            prompt,
                            multiple,
                            commands,
                        );
                    }
                    servo::SelectElementOptionOrOptgroup::Optgroup { label, options } => {
                        ui.label(egui::RichText::new(label).color(dim).small());
                        for option in options {
                            add_option_row(
                                ui,
                                option,
                                row_w,
                                &mut slot,
                                selected_slot,
                                prompt,
                                multiple,
                                commands,
                            );
                        }
                    }
                }
            }
        });

    if multiple {
        ui.add_space(6.0);
        let ok = ui.add_sized(
            [90.0, ROW_H],
            egui::Button::selectable(
                slot == selected_slot,
                egui::RichText::new("OK").color(egui::Color32::WHITE),
            ),
        );
        if ok.clicked() {
            commands.push(AppCommand::Prompt(PromptAction::ClickSlot(slot)));
        }
    }
}

/// One option row. Enabled options take the next slot; disabled ones render
/// dim and unclickable. The chosen state is marked with ☑/☐ (multi) or •
/// (single) — all cmap-verified in egui's bundled fonts.
#[allow(clippy::too_many_arguments)]
fn add_option_row(
    ui: &mut egui::Ui,
    option: &SelectElementOption,
    row_w: f32,
    slot: &mut usize,
    selected_slot: usize,
    prompt: &Prompt,
    multiple: bool,
    commands: &mut Vec<AppCommand>,
) {
    let chosen = prompt.is_chosen(option.id);
    let label = if multiple {
        format!("{} {}", if chosen { "☑" } else { "☐" }, option.label)
    } else if chosen {
        format!("• {}", option.label)
    } else {
        option.label.clone()
    };

    if option.is_disabled {
        ui.add_sized(
            [row_w, ROW_H],
            egui::Label::new(egui::RichText::new(label).color(egui::Color32::from_gray(0x66)))
                .truncate(),
        );
        return;
    }

    let this = *slot;
    *slot += 1;
    let text = if chosen {
        egui::RichText::new(label).color(ACCENT).strong()
    } else {
        egui::RichText::new(label).color(egui::Color32::WHITE)
    };
    let row = ui.add_sized(
        [row_w, ROW_H],
        egui::Button::selectable(this == selected_slot, text).truncate(),
    );
    if row.clicked() {
        commands.push(AppCommand::Prompt(PromptAction::ClickSlot(this)));
    }
}

/// A simple dialog: the page's message (scrollable when long), the `prompt()`
/// text field, and the OK / Cancel buttons (slots 0 / 1).
#[allow(clippy::too_many_arguments)]
fn add_dialog(
    ui: &mut egui::Ui,
    screen: egui::Rect,
    prompt: &mut Prompt,
    message: &str,
    has_input: bool,
    has_cancel: bool,
    osk_caret: bool,
    commands: &mut Vec<AppCommand>,
) {
    let dim = egui::Color32::from_gray(0x99);
    // Web content can't draw browser chrome — the header marks the message as
    // coming from the page so it can't impersonate the UI.
    ui.label(egui::RichText::new("The page says:").color(dim));
    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .max_height(screen.height() * 0.5)
        .show(ui, |ui| {
            ui.label(egui::RichText::new(message).color(egui::Color32::WHITE));
        });
    ui.add_space(8.0);

    if has_input {
        // The gamepad types into this buffer through the on-screen keyboard
        // (X opens it); a physical keyboard can click and type directly.
        let edit_id = egui::Id::new("prompt_input");
        // While the OSK types here, keep egui's caret at the buffer end (it
        // won't follow the external edit on its own); desktop editing is left
        // untouched.
        if osk_caret {
            super::park_caret_end(ui.ctx(), edit_id, prompt.input_mut().chars().count());
        }
        ui.add(
            egui::TextEdit::singleline(prompt.input_mut())
                .desired_width(f32::INFINITY)
                .id(edit_id),
        );
        ui.add_space(8.0);
    }

    let selected = prompt.selected_slot();
    ui.horizontal(|ui| {
        let ok = ui.add_sized(
            [90.0, ROW_H],
            egui::Button::selectable(
                selected == 0,
                egui::RichText::new("OK").color(egui::Color32::WHITE),
            ),
        );
        if ok.clicked() {
            commands.push(AppCommand::Prompt(PromptAction::ClickSlot(0)));
        }
        if has_cancel {
            let cancel = ui.add_sized(
                [90.0, ROW_H],
                egui::Button::selectable(
                    selected == 1,
                    egui::RichText::new("Cancel").color(egui::Color32::WHITE),
                ),
            );
            if cancel.clicked() {
                commands.push(AppCommand::Prompt(PromptAction::ClickSlot(1)));
            }
        }
    });
}
