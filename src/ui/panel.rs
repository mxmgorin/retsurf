//! Shared chrome for the Hub and settings overlays: panel metrics, the pinned
//! frame with its close button, the section bar, the bounded section scroll.

use super::keys::{centered_button_hints, ButtonHint, BUTTON_HINT_FONT};
use super::mark::{self, Mark};
use super::theme::{close_button, icon_fill, palette, CLOSE_SIZE, ROW_FONT};
use egui_sdl2::egui;
use egui_sdl2::egui::AtomExt as _;

/// Shared row metrics so every overlay's list reads alike.
pub(super) const ROW_RADIUS: f32 = 6.0;
pub(super) const ROW_GAP: f32 = 4.0;
pub(super) const ROW_H: f32 = 30.0;

/// An on/off switch's track (logical px); the knob is inset by `SWITCH_INSET`.
const SWITCH_W: f32 = 34.0;
const SWITCH_H: f32 = 18.0;
const SWITCH_INSET: f32 = 3.0;

/// Panel inner padding; the sides get more room than the top and bottom.
/// [`SIDES`] is the pair, subtracted from the screen width for row widths.
pub(super) const PAD_X: f32 = 30.0;
pub(super) const PAD_Y: f32 = 16.0;
pub(super) const SIDES: f32 = PAD_X * 2.0;

/// A section button's height.
const SECTION_H: f32 = 28.0;

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
        .rect_filled(screen, 0.0, palette().panel_fill);
    egui::Area::new(egui::Id::new(id))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .constrain(false)
        // egui fades a freshly shown area in, which over a full-screen panel is
        // the page showing through it for the length of the fade.
        .fade_in(false)
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(palette().panel_fill)
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
    toned_row(ui, width, selected, label, value, Tone::Plain)
}

/// An on/off switch at the trailing edge of `row`, where a value would sit.
pub(super) fn paint_switch(ui: &egui::Ui, row: egui::Rect, on: bool) {
    let right = row.right() - ui.spacing().button_padding.x;
    let track = egui::Rect::from_min_size(
        egui::pos2(right - SWITCH_W, row.center().y - SWITCH_H / 2.0),
        egui::vec2(SWITCH_W, SWITCH_H),
    );
    let (fill, knob_fill) = if on {
        (palette().accent, palette().text)
    } else {
        (palette().hairline, palette().dim)
    };
    let painter = ui.painter();
    painter.rect_filled(track, SWITCH_H / 2.0, fill);
    let r = SWITCH_H / 2.0 - SWITCH_INSET;
    let x = if on {
        track.right() - SWITCH_H / 2.0
    } else {
        track.left() + SWITCH_H / 2.0
    };
    // A fully rounded rect, not a circle: it must square along with the track.
    let knob =
        egui::Rect::from_center_size(egui::pos2(x, track.center().y), egui::vec2(r, r) * 2.0);
    painter.rect_filled(knob, r, knob_fill);
}

/// A [`row`] whose value gives way first, for a short label beside a value of
/// any length (a name the user chose), its label in `tone`'s colour.
pub(super) fn toned_named_row(
    ui: &mut egui::Ui,
    width: f32,
    selected: bool,
    label: &str,
    value: &str,
    tone: Tone,
) -> egui::Response {
    let (label, value) = toned_texts(label, value, tone);
    let value = value.atom_shrink(true);
    ui.add_sized(
        [width, ROW_H],
        egui::Button::selectable(selected, (label, egui::Atom::grow(), value))
            .corner_radius(ROW_RADIUS)
            .truncate(),
    )
}

/// The least of a row's width its label keeps beside a value shown whole.
const LABEL_KEEP: f32 = 1.0 / 3.0;

/// A [`marked_row`]'s dot: small beside the label's type.
const DOT_SIZE: f32 = ROW_FONT / 2.0;

/// What a [`marked_row`] shows at its trailing edge: its value, then a glyph
/// that never gives way.
#[derive(Clone, Copy, Default)]
pub(super) struct Trail<'a> {
    pub value: &'a str,
    pub after: Option<&'a str>,
}

/// What a [`marked_row`] shows at its leading edge: an icon, its label, and
/// with `dot` an accent dot after the label.
pub(super) struct Lead<'a> {
    pub icon: RowIcon<'a>,
    pub label: &'a str,
    pub dot: bool,
}

/// A row's leading icon: a glyph in the label's colour, a picture, or a mark.
pub(super) enum RowIcon<'a> {
    Glyph(&'static str),
    /// An [`egui_phosphor::fill`] glyph in the accent: the row's "on" state.
    Fill(&'static str),
    Image(egui::Atom<'a>),
    Mark(Mark),
}

impl Lead<'_> {
    /// Its icon's mark, when the icon is one.
    fn mark(&self) -> Option<Mark> {
        match self.icon {
            RowIcon::Mark(mark) => Some(mark),
            RowIcon::Glyph(_) | RowIcon::Fill(_) | RowIcon::Image(_) => None,
        }
    }
}

/// The custom atom a [`Mark`] is painted into.
fn mark_id() -> egui::Id {
    egui::Id::new("mark")
}

/// Show a row's `button` at `width` as `add_sized` would, painting `mark` into
/// its icon's column in `tone`'s colour: only `atom_ui` reports that rect.
fn add_row_button(
    ui: &mut egui::Ui,
    width: f32,
    button: egui::Button<'_>,
    mark: Option<Mark>,
    tone: Tone,
) -> egui::AtomLayoutResponse {
    let layout = egui::Layout::centered_and_justified(ui.layout().main_dir());
    let resp = ui
        .allocate_ui_with_layout(egui::vec2(width, ROW_H), layout, |ui| button.atom_ui(ui))
        .inner;
    if let (Some(mark), Some(rect)) = (mark, resp.rect(mark_id())) {
        mark.paint(ui.painter(), rect, ROW_FONT, tone_color(tone));
    }
    resp
}

/// A row with `lead` at its leading edge and `trail` at its trailing one, its
/// label in `tone`'s colour. The wider of label and trail gives way, but
/// selected the trail shows whole while it leaves the label [`LABEL_KEEP`].
pub(super) fn marked_row(
    ui: &mut egui::Ui,
    width: f32,
    selected: bool,
    lead: Lead,
    trail: Trail,
    tone: Tone,
) -> egui::Response {
    let font = egui::FontId::proportional(ROW_FONT);
    let text_w = |text: String| {
        ui.fonts_mut(|f| f.layout_no_wrap(text, font.clone(), egui::Color32::WHITE))
            .size()
            .x
    };
    let trail_w = text_w(format!(
        "{}{}",
        trail.value,
        trail.after.unwrap_or_default()
    ));
    let label_w = text_w(lead.label.to_string()) + icon_column().x;
    let label_gives = (selected && trail_w <= width * (1.0 - LABEL_KEEP)) || label_w > trail_w;
    let value = value_text(trail.value);
    let mark = lead.mark();
    let icon_id = ui.next_auto_id().with("icon");
    let mut atoms = lead_atoms(lead, tone, label_gives, icon_id);
    atoms.push_right(egui::Atom::grow());
    atoms.push_right(match label_gives {
        true => value.into(),
        false => value.atom_shrink(true),
    });
    if let Some(glyph) = trail.after {
        atoms.push_right(
            egui::RichText::new(glyph)
                .color(palette().accent)
                .size(ROW_FONT),
        );
    }
    let button = egui::Button::selectable(selected, atoms)
        .corner_radius(ROW_RADIUS)
        .truncate();
    add_row_button(ui, width, button, mark, tone).response
}

/// The share of a [`stepper_row`]'s width its stepper takes: each side a
/// target a thumb can hit.
const STEPPER_SHARE: f32 = 0.5;

/// A row with `lead` at its leading edge and a stepper over its trailing
/// [`STEPPER_SHARE`], `value` centred between carets. Returns the row's response
/// and a click's step on the stepper: -1 left of the value's centre, +1 right.
pub(super) fn stepper_row(
    ui: &mut egui::Ui,
    width: f32,
    selected: bool,
    lead: Lead,
    value: &str,
    tone: Tone,
) -> (egui::Response, Option<i32>) {
    let id = egui::Id::new("stepper");
    let stepper_w = width * STEPPER_SHARE - ui.spacing().button_padding.x;
    let mark = lead.mark();
    let icon_id = ui.next_auto_id().with("icon");
    let mut atoms = lead_atoms(lead, tone, true, icon_id);
    atoms.push_right(egui::Atom::grow());
    atoms.push_right(egui::Atom::custom(id, egui::vec2(stepper_w, ROW_FONT)));
    let button = egui::Button::selectable(selected, atoms)
        .corner_radius(ROW_RADIUS)
        .truncate();
    let resp = add_row_button(ui, width, button, mark, tone);
    let Some(rect) = resp.rect(id) else {
        return (resp.response, None);
    };
    let painter = ui.painter();
    let font = egui::FontId::proportional(ROW_FONT);
    let accent = palette().accent;
    let (left, right) = (
        egui::pos2(rect.left(), rect.center().y),
        egui::pos2(rect.right(), rect.center().y),
    );
    let before = painter.text(
        left,
        egui::Align2::LEFT_CENTER,
        egui_phosphor::bold::CARET_LEFT,
        font.clone(),
        accent,
    );
    let after = painter.text(
        right,
        egui::Align2::RIGHT_CENTER,
        egui_phosphor::bold::CARET_RIGHT,
        font,
        accent,
    );
    let room = (after.left() - before.right() - 2.0 * ui.spacing().icon_spacing).max(0.0);
    let galley = egui::WidgetText::from(value_text(value)).into_galley(
        ui,
        Some(egui::TextWrapMode::Truncate),
        room,
        egui::TextStyle::Body,
    );
    painter.galley(rect.center() - galley.size() / 2.0, galley, accent);
    let step = resp
        .clicked()
        .then(|| resp.interact_pointer_pos())
        .flatten()
        .filter(|pos| pos.x >= rect.left())
        .map(|pos| if pos.x < rect.center().x { -1 } else { 1 });
    (resp.response, step)
}

/// `lead`'s icon, label and dot, its label in `tone`'s colour and, when
/// `shrink`, giving way to what follows; `icon_id` names the icon's box.
fn lead_atoms<'a>(lead: Lead<'a>, tone: Tone, shrink: bool, icon_id: egui::Id) -> egui::Atoms<'a> {
    let label = label_text(lead.label, tone);
    let mut atoms = egui::Atoms::default();
    let column = icon_column();
    atoms.push_right(match lead.icon {
        RowIcon::Glyph(glyph) => {
            let glyph = egui::RichText::new(glyph)
                .color(tone_color(tone))
                .size(ROW_FONT);
            centered(glyph.into(), column, icon_id)
        }
        RowIcon::Fill(glyph) => {
            let glyph = icon_fill(glyph).color(palette().accent).size(ROW_FONT);
            centered(glyph.into(), column, icon_id)
        }
        RowIcon::Image(image) => centered(image, column, icon_id),
        RowIcon::Mark(_) => egui::Atom::custom(mark_id(), column),
    });
    atoms.push_right(match shrink {
        true => label.atom_shrink(true),
        false => label.into(),
    });
    if lead.dot {
        let dot = icon_fill(egui_phosphor::fill::CIRCLE)
            .size(DOT_SIZE)
            .color(palette().accent);
        atoms.push_right(dot);
    }
    atoms
}

/// The box every row icon is centred in, so labels line up beside the widest
/// mark: that mark with a row gap's air around it.
fn icon_column() -> egui::Vec2 {
    egui::vec2(mark::column_width(ROW_FONT) + ROW_GAP, ROW_FONT)
}

/// `atom` centred in a box of `size`. The box registers as a widget, so `id`
/// must differ from its row's, which egui would otherwise hand it too.
fn centered(atom: egui::Atom<'_>, size: egui::Vec2, id: egui::Id) -> egui::Atom<'_> {
    let layout = egui::AtomLayout::new(atom)
        .id(id)
        .min_size(size)
        .align2(egui::Align2::CENTER_CENTER);
    egui::Atom::layout(layout)
}

/// The colour of a label in `tone`.
pub(super) fn tone_color(tone: Tone) -> egui::Color32 {
    match tone {
        Tone::Plain => palette().text,
        Tone::Action => palette().accent,
        Tone::Danger => palette().warn,
        Tone::Muted => palette().disabled,
    }
}

fn toned_texts(label: &str, value: &str, tone: Tone) -> (egui::RichText, egui::RichText) {
    (label_text(label, tone), value_text(value))
}

fn label_text(label: &str, tone: Tone) -> egui::RichText {
    egui::RichText::new(label)
        .color(tone_color(tone))
        .size(ROW_FONT)
}

fn value_text(value: &str) -> egui::RichText {
    egui::RichText::new(value)
        .color(palette().accent)
        .size(ROW_FONT)
}

/// How a row's label is coloured: a value to read, a verb, a verb that
/// destroys something, or a verb with nothing to act on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Tone {
    Plain,
    Action,
    Danger,
    Muted,
}

/// One entry of a [`grouped_row_list`]: a row to select, or a heading or divider
/// between them. Neither is selectable, so the index the list is given and
/// the one it reports both count rows alone.
pub(super) enum ListItem {
    Heading(String),
    Row(String, String),
    /// A row whose label is coloured by its [`Tone`].
    Toned(String, String, Tone),
    /// A hairline between the rows above and below.
    Divider,
}

/// A titled full-screen panel over one scrolling list, which headings and
/// dividers break into groups; `on_click` gets the index of a clicked row.
/// Returns whether it was closed.
pub(super) fn grouped_row_list(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    items: Vec<ListItem>,
    selected: usize,
    on_click: impl FnMut(usize),
) -> bool {
    grouped_row_list_above(ctx, id, title, items, selected, 0.0, on_click)
}

/// [`grouped_row_list`] ending `bottom` above the screen's edge, so a keyboard
/// drawn there covers none of it and the highlight scrolls into what is left.
pub(super) fn grouped_row_list_above(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    items: Vec<ListItem>,
    selected: usize,
    bottom: f32,
    mut on_click: impl FnMut(usize),
) -> bool {
    let mut screen = ctx.content_rect();
    screen.max.y -= bottom;
    // A list cut shorter may have lost its highlight below the cut.
    let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new(id));
    let inset_key = egui::Id::new(("list_bottom", layer));
    if ctx.data(|d| d.get_temp::<f32>(inset_key)) != Some(bottom) {
        ctx.data_mut(|d| {
            d.insert_temp(inset_key, bottom);
            d.remove::<egui::Id>(centered_key(layer));
        });
    }
    let width = screen.width() - SIDES;
    panel(ctx, id, screen, |ui| {
        ui.label(
            egui::RichText::new(title)
                .color(palette().accent)
                .size(ROW_FONT)
                .strong(),
        );
        ui.add_space(ROW_GAP * 3.0);
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        section_scroll(ui, screen, 0.0).show(ui, |ui| {
            let mut index = 0;
            for item in items {
                let (label, value, tone) = match item {
                    ListItem::Heading(text) => {
                        heading(ui, &text);
                        continue;
                    }
                    ListItem::Divider => {
                        divider(ui, width);
                        continue;
                    }
                    ListItem::Row(label, value) => (label, value, Tone::Plain),
                    ListItem::Toned(label, value, tone) => (label, value, tone),
                };
                let is_selected = index == selected;
                let resp = toned_row(ui, width, is_selected, &label, &value, tone);
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

/// A [`row`] whose label takes `tone`'s colour.
fn toned_row(
    ui: &mut egui::Ui,
    width: f32,
    selected: bool,
    label: &str,
    value: &str,
    tone: Tone,
) -> egui::Response {
    let (label, value) = toned_texts(label, value, tone);
    ui.add_sized(
        [width, ROW_H],
        egui::Button::selectable(selected, (label, egui::Atom::grow(), value))
            .corner_radius(ROW_RADIUS)
            .truncate(),
    )
}

/// A head over the rows that follow it, muted, cut to the list and set off
/// from the rows above.
pub(super) fn head(ui: &mut egui::Ui, head: &str) {
    ui.add_space(HEAD_LEAD);
    let text = egui::RichText::new(head.to_uppercase())
        .color(palette().dim)
        .font(HEAD_FONT);
    ui.add(egui::Label::new(text).truncate());
}

/// [`head`]s draw and measure in this font.
pub(super) const HEAD_FONT: egui::FontId = egui::FontId::proportional(BUTTON_HINT_FONT);

/// The space a [`head`] leaves above itself.
pub(super) const HEAD_LEAD: f32 = ROW_GAP;

/// A hairline across the list, with a row gap's air on either side.
pub(super) fn divider(ui: &mut egui::Ui, width: f32) {
    ui.add_space(ROW_GAP);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, DIVIDER_LINE), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, palette().hairline);
    ui.add_space(ROW_GAP);
}

/// A [`divider`]'s line.
const DIVIDER_LINE: f32 = 1.0;
/// The height a [`divider`] takes, the space around its line included.
pub(super) const DIVIDER_H: f32 = ROW_GAP + DIVIDER_LINE + ROW_GAP;

/// A heading over the rows that follow it: dim and small, and indented to the
/// text of a row rather than to the panel's edge.
fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(ROW_GAP * 2.0);
    let indent = ui.spacing().button_padding.x;
    ui.horizontal(|ui| {
        ui.add_space(indent);
        ui.label(
            egui::RichText::new(text)
                .color(palette().dim)
                .size(HEADING_FONT)
                .strong(),
        );
    });
}

/// Space between two buttons of a [`section_bar`].
const SECTION_GAP: f32 = 6.0;

/// A [`section_bar`] button down to its icon: wider than the glyph, so the icons
/// stand apart and each is a target a thumb can hit.
const ICON_SECTION_W: f32 = 40.0;

/// The top section bar: a button per section (icon and label, or just the icon
/// on an inactive one where not all fit), its icon painted where `mark` gives
/// one. Returns the clicked one.
pub(super) fn section_bar<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    screen: egui::Rect,
    sections: &[T],
    active: T,
    label: fn(T) -> &'static str,
    icon: fn(T) -> &'static str,
    mark: impl Fn(T) -> Option<Mark>,
) -> Option<T> {
    let mut clicked = None;
    // Split off its icon, so a painted one keeps the glyph's spacing.
    let spaced = |section: T| format!("  {}", label(section));
    let font = egui::FontId::proportional(ROW_FONT);
    let pad = 2.0 * ui.spacing().button_padding.x + SECTION_GAP;
    let text_w = |text: String| {
        ui.fonts_mut(|f| f.layout_no_wrap(text, font.clone(), egui::Color32::WHITE))
            .size()
            .x
    };
    let icon_w = |section: T| match mark(section) {
        Some(mark) => mark.size(ROW_FONT).x,
        None => text_w(icon(section).to_string()),
    };
    let wanted: f32 = sections
        .iter()
        .map(|&s| icon_w(s) + text_w(spaced(s)) + pad)
        .sum();
    let fits = wanted <= screen.width() - SIDES - (CLOSE_SIZE + 8.0);
    let text = |text: String| {
        egui::RichText::new(text)
            .color(palette().text)
            .size(ROW_FONT)
    };
    ui.horizontal(|ui| {
        // The gap turns the flush button row into a segmented control.
        ui.spacing_mut().item_spacing.x = SECTION_GAP;
        for &section in sections {
            let mark = mark(section);
            let mut atoms = egui::Atoms::new(match mark {
                Some(mark) => egui::Atom::custom(mark_id(), mark.size(ROW_FONT)),
                None => text(icon(section).to_string()).into(),
            });
            let min_w = match fits || section == active {
                true => {
                    atoms.push_right(text(spaced(section)));
                    0.0
                }
                false => ICON_SECTION_W,
            };
            let tab = egui::Button::selectable(section == active, atoms)
                .corner_radius(ROW_RADIUS)
                .min_size(egui::vec2(min_w, SECTION_H))
                .gap(0.0)
                .atom_ui(ui);
            if let (Some(mark), Some(rect)) = (mark, tab.rect(mark_id())) {
                mark.paint(ui.painter(), rect, ROW_FONT, palette().text);
            }
            if tab.response.clicked() {
                clicked = Some(section);
            }
        }
    });
    clicked
}

/// Bring the highlighted row into view on the frame the highlight moves: egui
/// re-applies a scroll request every frame it is asked, which pins the list so
/// no drag can leave that row. Keyed per overlay — two can be open at once.
pub(super) fn center_selected(resp: &egui::Response) {
    let key = centered_key(resp.layer_id);
    if resp.ctx.data(|d| d.get_temp::<egui::Id>(key)) == Some(resp.id) {
        return;
    }
    let pointed = pointed_key(resp.layer_id);
    let by_pointer = resp.ctx.data_mut(|d| {
        d.insert_temp(key, resp.id);
        let picked = d.get_temp::<egui::Id>(pointed);
        d.remove::<egui::Id>(pointed);
        picked == Some(resp.id)
    });
    // The row under the pointer is in view already.
    if !by_pointer {
        resp.scroll_to_me(Some(egui::Align::Center));
    }
}

/// Whether the pointer moved onto `area` this frame: a pointer resting over a
/// scrolling list picks nothing. Marks `row` for [`center_selected`] to leave
/// where it is once highlighted.
pub(super) fn pointer_moved_onto(area: &egui::Response, row: &egui::Response) -> bool {
    let moved =
        area.contains_pointer() && area.ctx.input(|i| i.pointer.delta() != egui::Vec2::ZERO);
    if moved {
        let key = pointed_key(row.layer_id);
        row.ctx.data_mut(|d| d.insert_temp(key, row.id));
    }
    moved
}

/// Where [`center_selected`] remembers the row it last brought into view.
fn centered_key(layer: egui::LayerId) -> egui::Id {
    egui::Id::new(("centered_row", layer))
}

/// Where [`pointer_moved_onto`] marks the row the pointer picked.
fn pointed_key(layer: egui::LayerId) -> egui::Id {
    egui::Id::new(("pointed_row", layer))
}

/// A section's scroll area, capped to the room down to the screen bottom less
/// `foot`, kept for a [`footer`]: the panel's `Area` auto-sizes, so an unbounded
/// `ScrollArea` would grow past the screen and clip instead of scrolling.
pub(super) fn section_scroll(ui: &egui::Ui, screen: egui::Rect, foot: f32) -> egui::ScrollArea {
    let max_h = (screen.bottom() - PAD_Y - foot - ui.cursor().top()).max(0.0);
    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .max_height(max_h)
}

/// The room a panel's [`footer`] keeps under the list above it: its height as
/// last drawn, one line of hints under its rule before then.
pub(super) fn footer_room(ui: &egui::Ui, id: &str) -> f32 {
    let line = ui.fonts_mut(|f| f.row_height(&egui::FontId::proportional(BUTTON_HINT_FONT)));
    let measured = ui.ctx().data(|d| d.get_temp(footer_key(id)));
    measured.unwrap_or(DIVIDER_H + line) + ROW_GAP
}

/// `hints` centred at the panel's foot under a rule. Laid out after the list
/// it sits under, so a change of height shows on the next frame.
pub(super) fn footer(ui: &mut egui::Ui, screen: egui::Rect, id: &str, hints: &[ButtonHint]) {
    let width = screen.width() - SIDES;
    let bottom = screen.bottom() - PAD_Y;
    let drawn = ui.ctx().data(|d| d.get_temp::<f32>(footer_key(id)));
    let room = footer_room(ui, id) - ROW_GAP;
    let rect = egui::Rect::from_min_max(
        egui::pos2(screen.left() + PAD_X, bottom - room),
        egui::pos2(screen.left() + PAD_X + width, bottom),
    );
    let h = ui
        .scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            divider(ui, width);
            centered_button_hints(ui, hints, width);
        })
        .response
        .rect
        .height();
    if drawn.is_none_or(|drawn| (drawn - h).abs() > EPSILON) {
        ui.ctx().data_mut(|d| d.insert_temp(footer_key(id), h));
        ui.ctx().request_repaint();
    }
}

/// Where a panel's [`footer`] keeps its measured height.
fn footer_key(id: &str) -> egui::Id {
    egui::Id::new(("panel_footer_h", id))
}

/// The band over a scroll viewport's edge that dissolves into the page while
/// more waits beyond it.
pub(super) const FADE: f32 = 24.0;
/// A difference in logical px under this is none: a measure that has settled,
/// a scroll at its end.
pub(super) const EPSILON: f32 = 0.5;

/// The offset the scroll area salted `salt` under `ui` was last drawn at.
pub(super) fn scroll_offset(ui: &egui::Ui, salt: &str) -> Option<egui::Vec2> {
    let id = ui.make_persistent_id(salt);
    egui::scroll_area::State::load(ui.ctx(), id).map(|state| state.offset)
}

/// Repaint when `scroll` ended at another offset than `drawn`: egui clamps
/// after shrunken content, or reaches a target, only once the frame is built.
pub(super) fn settle<R>(
    ui: &egui::Ui,
    drawn: Option<egui::Vec2>,
    scroll: &egui::scroll_area::ScrollAreaOutput<R>,
) {
    if drawn != Some(scroll.state.offset) {
        ui.ctx().request_repaint();
    }
}

/// Whether `scroll` holds content past its viewport's edge in `dir`: the far
/// edge of a top-down or left-to-right scroll, the near edge of the reverse.
pub(super) fn scrolls_on<R>(
    scroll: &egui::scroll_area::ScrollAreaOutput<R>,
    dir: egui::Direction,
) -> bool {
    let (offset, span, content) = match dir {
        egui::Direction::TopDown | egui::Direction::BottomUp => (
            scroll.state.offset.y,
            scroll.inner_rect.height(),
            scroll.content_size.y,
        ),
        egui::Direction::LeftToRight | egui::Direction::RightToLeft => (
            scroll.state.offset.x,
            scroll.inner_rect.width(),
            scroll.content_size.x,
        ),
    };
    match dir {
        egui::Direction::TopDown | egui::Direction::LeftToRight => {
            offset + span < content - EPSILON
        }
        egui::Direction::BottomUp | egui::Direction::RightToLeft => offset > EPSILON,
    }
}

/// Dissolve `band` into the page along `dir`: clear where it starts, solid
/// where it ends, so clipped content fades instead of being cut.
pub(super) fn fade(painter: &egui::Painter, band: egui::Rect, dir: egui::Direction) {
    let clear = egui::Color32::TRANSPARENT;
    // Corners in mesh order: left-top, right-top, right-bottom, left-bottom.
    let colors = match dir {
        egui::Direction::TopDown => [clear, clear, palette().bg, palette().bg],
        egui::Direction::BottomUp => [palette().bg, palette().bg, clear, clear],
        egui::Direction::LeftToRight => [clear, palette().bg, palette().bg, clear],
        egui::Direction::RightToLeft => [palette().bg, clear, clear, palette().bg],
    };
    let corners = [
        band.left_top(),
        band.right_top(),
        band.right_bottom(),
        band.left_bottom(),
    ];
    let mut mesh = egui::Mesh::default();
    for (pos, color) in corners.into_iter().zip(colors) {
        mesh.colored_vertex(pos, color);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}
