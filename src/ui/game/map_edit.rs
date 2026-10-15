//! Rendering of the Game Mode map editor (state in
//! [`crate::overlay::game::map_edit`]): a row per bound source with the row that
//! adds one after them, then the layers on the base or a layer's own verbs, and
//! the list of what the focused row can send, all over the shared panel
//! chrome, so a long list scrolls the way the menu's and settings' do.

use crate::command::{AppCommand, GameMapEditAction};
use crate::overlay::game::map_edit::{
    held_label, mods_label, pad_label, target_label, CaptureFor, Device, Kind, LayerName, LayerRow,
    MapEdit, Modifier, MouseKind, Slot, Tail,
};
use crate::ui::panel::{self, ListItem, Tone};
use egui_phosphor::bold;
use egui_sdl2::egui;
use inputbind::Pad;

/// `osk_height` is the keyboard's, when it is up over the editor.
pub(in crate::ui) fn add_map_edit(
    ctx: &egui::Context,
    edit: &MapEdit,
    osk_height: f32,
    commands: &mut Vec<AppCommand>,
) {
    let closed = panel::grouped_row_list_above(
        ctx,
        "map_edit",
        &title(edit),
        items(edit),
        edit.selected(),
        osk_height,
        |index| commands.push(AppCommand::GameMapEdit(GameMapEditAction::Click(index))),
    );
    if closed {
        commands.push(AppCommand::GameMapEdit(GameMapEditAction::Close));
    }
}

/// The panel's title, worded for what the screen is doing. Nothing under it —
/// every verb here is a row, so a hint could only name them a second time.
fn title(edit: &MapEdit) -> String {
    let layer = edit.layer().map(str::to_uppercase);
    match edit.capturing() {
        Some(CaptureFor::Source) => return "PRESS A BUTTON OR KEY, OR PUSH A STICK".to_string(),
        Some(CaptureFor::Activator) => {
            return format!("PRESS WHAT HOLDS LAYER {}", layer.unwrap_or_default())
        }
        None => {}
    }
    if edit.confirming() {
        return format!("DELETE LAYER {}?", layer.unwrap_or_default());
    }
    if edit.kind_open() {
        let slot = edit.slot().map(|slot| slot.label()).unwrap_or_default();
        return match edit.device_open() {
            Some(device) => format!("{} SENDS {}", slot.to_uppercase(), device.title()),
            None => format!("{} SENDS", slot.to_uppercase()),
        };
    }
    match layer {
        Some(layer) => format!("{} - LAYER {layer}", edit.map_name().to_uppercase()),
        None => edit.map_name().to_uppercase(),
    }
}

/// Marks the row of a device list that the source sends now.
const CURRENT: &str = "current";

/// Whichever list is up. The sources arrive grouped by the table that holds
/// them, so a heading goes in wherever the group changes; the tail follows,
/// the row that asked carrying whatever its last capture had to say.
fn items(edit: &MapEdit) -> Vec<ListItem> {
    if edit.confirming() {
        return edit
            .confirm_rows()
            .iter()
            .map(|label| ListItem::Row(label.to_string(), String::new()))
            .collect();
    }
    if edit.kind_open() {
        let Some(slot) = edit.slot() else {
            return vec![];
        };
        let current = edit.current();
        // The kind the source sends now carries what exactly it sends; a device
        // list marks the matching row instead.
        let rows: Vec<(String, String)> = match edit.device_open() {
            Some(Device::Modifiers) => {
                let mods = edit.current_mods();
                Modifier::ALL
                    .iter()
                    .map(|modifier| {
                        let state = match modifier.is_in(mods) {
                            true => "On",
                            false => "Off",
                        };
                        (modifier.label().to_string(), state.to_string())
                    })
                    .collect()
            }
            Some(device) => {
                let labels: Vec<String> = match device {
                    Device::Mouse => MouseKind::all(&slot)
                        .iter()
                        .map(|kind| kind.label().to_string())
                        .collect(),
                    Device::Gamepad => Pad::ALL
                        .iter()
                        .map(|pad| pad_label(*pad).to_string())
                        .collect(),
                    Device::Layer => edit
                        .layer_choices(&slot)
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    Device::Modifiers => vec![],
                };
                labels
                    .into_iter()
                    .zip(edit.device_targets(device, &slot))
                    .map(|(label, target)| {
                        let mark = match Some(target.as_str()) == current {
                            true => CURRENT,
                            false => "",
                        };
                        (label, mark.to_string())
                    })
                    .collect()
            }
            None => edit
                .kinds(&slot)
                .into_iter()
                .map(|kind| {
                    let mods = edit.current_mods();
                    let value = match kind {
                        Kind::Modifiers => mods_label(mods),
                        _ if Some(kind) == edit.current_kind() => current
                            .map(|target| held_label(target_label(target), mods))
                            .unwrap_or_default(),
                        _ => String::new(),
                    };
                    (kind.label().to_string(), value)
                })
                .collect(),
        };
        return rows
            .into_iter()
            .map(|(label, value)| ListItem::Row(label, value))
            .collect();
    }
    let mut items = vec![];
    let mut group = None;
    for row in edit.rows() {
        let heading = heading_for(&row.slot);
        if group != Some(heading) {
            items.push(ListItem::Heading(heading.to_string()));
            group = Some(heading);
        }
        items.push(ListItem::Row(row.slot.label(), row.label()));
    }
    // While the keyboard is up, the row it was opened from carries what is
    // being typed: there is nowhere else on screen the name would show.
    let typed = |what| {
        edit.naming()
            .filter(|naming| naming.what == what)
            .map(|naming| naming.text.clone())
    };
    // The verbs part from the bindings, so the two never read as one list.
    if !edit.rows().is_empty() {
        items.push(ListItem::Divider);
    }
    for tail in edit.tail() {
        let (value, tone) = match tail {
            Tail::New => (
                edit.note(CaptureFor::Source).map(str::to_string),
                Tone::Action,
            ),
            Tail::Layer(i) => {
                if i == 0 {
                    items.push(ListItem::Heading("LAYERS".to_string()));
                }
                let layer = &edit.layers()[i];
                // The caret says the row opens a screen rather than acting.
                let value = format!("held by {}  {}", layer.held_by_label(), bold::CARET_RIGHT);
                (Some(value), Tone::Plain)
            }
            Tail::HeldBy => (
                edit.note(CaptureFor::Activator)
                    .map(str::to_string)
                    .or_else(|| edit.scoped_layer().map(LayerRow::held_by_label)),
                Tone::Plain,
            ),
            Tail::NewLayer => (typed(LayerName::New), Tone::Action),
            Tail::Rename => (typed(LayerName::Rename), Tone::Action),
            Tail::Delete => (None, Tone::Danger),
        };
        let label = match tail {
            Tail::Layer(i) => edit.layers()[i].name.clone(),
            _ => tail.label().to_string(),
        };
        items.push(ListItem::Toned(
            format!("{}  {label}", tail_icon(tail)),
            value.unwrap_or_default(),
            tone,
        ));
    }
    items
}

/// A tail row's icon: what kind of verb it is, at a glance.
fn tail_icon(tail: Tail) -> &'static str {
    match tail {
        Tail::New | Tail::NewLayer => bold::PLUS,
        Tail::Layer(_) => bold::STACK,
        Tail::HeldBy => bold::HAND_GRABBING,
        Tail::Rename => bold::PENCIL_SIMPLE,
        Tail::Delete => bold::TRASH,
    }
}

/// The heading a source sits under — the table it is written in, said as the
/// screen says things.
fn heading_for(slot: &Slot) -> &'static str {
    match slot {
        Slot::Stick(_) | Slot::Direction(..) => "STICKS",
        Slot::Button(_) => "GAMEPAD",
        Slot::Key(_) => "KEYBOARD",
    }
}
