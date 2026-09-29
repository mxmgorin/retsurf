//! Shared chrome for the menu and settings overlays: panel metrics, the pinned
//! frame with its close button, the section tab bar, the bounded section scroll.

use super::theme::{close_button, ACCENT, CLOSE_SIZE, DIM, PANEL_FILL, ROW_FONT};
use egui_sdl2::egui;
use egui_sdl2::egui::AtomExt as _;

/// Shared row metrics so every overlay's list reads alike.
pub(super) const ROW_RADIUS: f32 = 6.0;
pub(super) const ROW_GAP: f32 = 4.0;
pub(super) const ROW_H: f32 = 30.0;

/// Panel inner padding; the sides get more room than the top and bottom.
/// [`SIDES`] is the pair, subtracted from the screen width for row widths.
pub(super) const PAD_X: f32 = 30.0;
pub(super) const PAD_Y: f32 = 16.0;
pub(super) const SIDES: f32 = PAD_X * 2.0;

/// Section-bar tab height.
const TAB_H: f32 = 28.0;

/// Group headings sit under the rows they name, in size as in weight.
const HEADING_FONT: f32 = ROW_FONT - 3.0;

/// The full-screen panel shell. `constrain(false)`: the frame fills the screen
/// exactly, so an egui "fit" shift would cancel the left padding. The close
/// button is painted outside the content flow so it can't shift it.
pub(super) fn panel(
    ctx: &egui::Context,
    id: &str,
    screen: egui::Rect,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> bool {
    let mut closed = false;
    // An area's first show is an invisible sizing pass, which would leave the
    // page on screen for that frame; a painted layer needs no such pass.
    ctx.layer_painter(egui::LayerId::background())
        .rect_filled(screen, 0.0, PANEL_FILL);
    egui::Area::new(egui::Id::new(id))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .constrain(false)
        // egui fades a freshly shown area in, which over a full-screen panel is
        // the page showing through it for the length of the fade.
        .fade_in(false)
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(PANEL_FILL)
                .inner_margin(egui::Margin::symmetric(PAD_X as i8, PAD_Y as i8))
                .show(ui, |ui| {
                    ui.set_min_size(screen.size() - egui::vec2(SIDES, PAD_Y * 2.0));
                    let close_rect = egui::Rect::from_min_size(
                        egui::pos2(screen.right() - PAD_X - CLOSE_SIZE, screen.top() + PAD_Y),
                        egui::vec2(CLOSE_SIZE, CLOSE_SIZE),
                    );
                    closed = close_button(ui, close_rect, egui::Id::new((id, "close"))).clicked();
                    add_contents(ui);
                });
        });
    closed
}

/// A selectable row: `label` left, `value` (in the accent) pushed to the
/// trailing edge — one shape, so the highlight reads identically everywhere.
pub(super) fn row(
    ui: &mut egui::Ui,
    width: f32,
    selected: bool,
    label: &str,
    value: &str,
) -> egui::Response {
    let (label, value) = row_texts(label, value);
    ui.add_sized(
        [width, ROW_H],
        egui::Button::selectable(selected, (label, egui::Atom::grow(), value))
            .corner_radius(ROW_RADIUS)
            .truncate(),
    )
}

/// A [`row`] whose value gives way first, for a short label beside a value of
/// any length (a name the user chose).
pub(super) fn named_row(
    ui: &mut egui::Ui,
    width: f32,
    selected: bool,
    label: &str,
    value: &str,
) -> egui::Response {
    let (label, value) = row_texts(label, value);
    let value = value.atom_shrink(true);
    ui.add_sized(
        [width, ROW_H],
        egui::Button::selectable(selected, (label, egui::Atom::grow(), value))
            .corner_radius(ROW_RADIUS)
            .truncate(),
    )
}

fn row_texts(label: &str, value: &str) -> (egui::RichText, egui::RichText) {
    let label = egui::RichText::new(label)
        .color(egui::Color32::WHITE)
        .size(ROW_FONT);
    (
        label,
        egui::RichText::new(value).color(ACCENT).size(ROW_FONT),
    )
}

/// One entry of a [`row_list`]: a row to select, or a heading over the rows
/// that follow it. A heading is not selectable, so the index the list is given
/// and the one it reports both count rows alone.
pub(super) enum ListItem {
    Heading(String),
    Row(String, String),
}

/// A titled full-screen panel over one scrolling list of `(label, value)` rows;
/// `on_click` gets the index of a clicked row. Returns whether it was closed.
pub(super) fn row_list(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    rows: Vec<(String, String)>,
    selected: usize,
    on_click: impl FnMut(usize),
) -> bool {
    let items = rows
        .into_iter()
        .map(|(label, value)| ListItem::Row(label, value))
        .collect();
    grouped_row_list(ctx, id, title, items, selected, on_click)
}

/// [`row_list`] over items that may carry headings — a long list reads as its
/// groups rather than as one run of rows.
pub(super) fn grouped_row_list(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    items: Vec<ListItem>,
    selected: usize,
    mut on_click: impl FnMut(usize),
) -> bool {
    let screen = ctx.content_rect();
    let width = screen.width() - SIDES;
    panel(ctx, id, screen, |ui| {
        ui.label(
            egui::RichText::new(title)
                .color(ACCENT)
                .size(ROW_FONT)
                .strong(),
        );
        ui.add_space(ROW_GAP * 3.0);
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        section_scroll(ui, screen).show(ui, |ui| {
            let mut index = 0;
            for item in items {
                let (label, value) = match item {
                    ListItem::Heading(text) => {
                        heading(ui, &text);
                        continue;
                    }
                    ListItem::Row(label, value) => (label, value),
                };
                let is_selected = index == selected;
                let resp = row(ui, width, is_selected, &label, &value);
                if is_selected {
                    center_selected(&resp);
                }
                if resp.clicked() {
                    on_click(index);
                }
                index += 1;
            }
        });
    })
}

/// A heading over the rows that follow it: dim and small, and indented to the
/// text of a row rather than to the panel's edge.
fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(ROW_GAP * 2.0);
    let indent = ui.spacing().button_padding.x;
    ui.horizontal(|ui| {
        ui.add_space(indent);
        ui.label(
            egui::RichText::new(text)
                .color(DIM)
                .size(HEADING_FONT)
                .strong(),
        );
    });
}

/// Space between two tabs of a [`section_bar`].
const TAB_GAP: f32 = 6.0;

/// A [`section_bar`] tab down to its icon: wider than the glyph, so the icons
/// stand apart and each is a target a thumb can hit.
const ICON_TAB_W: f32 = 40.0;

/// The top section bar: a tab per section (icon and label, or just the icon on
/// an inactive tab where not all fit), then `trailing`. Returns the clicked one.
pub(super) fn section_bar<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    screen: egui::Rect,
    sections: &[T],
    active: T,
    label: fn(T) -> &'static str,
    icon: fn(T) -> &'static str,
    trailing: impl FnOnce(&mut egui::Ui),
) -> Option<T> {
    let mut clicked = None;
    let full = |section: T| format!("{}  {}", icon(section), label(section));
    let font = egui::FontId::proportional(ROW_FONT);
    let pad = 2.0 * ui.spacing().button_padding.x + TAB_GAP;
    let width = |text: String| {
        ui.fonts_mut(|f| f.layout_no_wrap(text, font.clone(), egui::Color32::WHITE))
            .size()
            .x
            + pad
    };
    let wanted: f32 = sections.iter().map(|&s| width(full(s))).sum();
    let fits = wanted <= screen.width() - SIDES - (CLOSE_SIZE + 8.0);
    ui.horizontal(|ui| {
        // The gap turns the flush button row into a segmented control.
        ui.spacing_mut().item_spacing.x = TAB_GAP;
        for &section in sections {
            let (text, min_w) = match fits || section == active {
                true => (full(section), 0.0),
                false => (icon(section).to_string(), ICON_TAB_W),
            };
            let tab = egui::Button::selectable(
                section == active,
                egui::RichText::new(text)
                    .color(egui::Color32::WHITE)
                    .size(ROW_FONT),
            )
            .corner_radius(ROW_RADIUS)
            .min_size(egui::vec2(min_w, TAB_H));
            if ui.add(tab).clicked() {
                clicked = Some(section);
            }
        }
        // Width from `screen`; `available_width()` runs past the visible edge.
        // Reserve the close button's footprint so `trailing` sits left of it.
        let remaining = screen.width() - SIDES - ui.min_rect().width() - (CLOSE_SIZE + 8.0);
        ui.allocate_ui_with_layout(
            egui::vec2(remaining.max(1.0), TAB_H),
            egui::Layout::right_to_left(egui::Align::Center),
            trailing,
        );
    });
    clicked
}

/// Bring the highlighted row into view on the frame the highlight moves: egui
/// re-applies a scroll request every frame it is asked, which pins the list so
/// no drag can leave that row. Keyed per overlay — two can be open at once.
pub(super) fn center_selected(resp: &egui::Response) {
    let key = egui::Id::new(("centered_row", resp.layer_id));
    if resp.ctx.data(|d| d.get_temp::<egui::Id>(key)) == Some(resp.id) {
        return;
    }
    resp.ctx.data_mut(|d| d.insert_temp(key, resp.id));
    resp.scroll_to_me(Some(egui::Align::Center));
}

/// A section's scroll area, capped to the room down to the screen bottom: the
/// panel's `Area` auto-sizes, so an unbounded `ScrollArea` would grow past the
/// screen and clip instead of scrolling.
pub(super) fn section_scroll(ui: &egui::Ui, screen: egui::Rect) -> egui::ScrollArea {
    let max_h = (screen.bottom() - PAD_Y - ui.cursor().top()).max(0.0);
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .max_height(max_h)
}
