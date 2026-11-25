//! Rendering of the Hub (state lives in [`crate::overlay::hub`]):
//! the section bar with the close action, and the four section lists
//! (Tabs / Bookmarks / History / Downloads).

use super::favicon::PageIcons;
use super::keys;
use super::labels;
use super::panel::{
    self, center_selected, section_scroll, Tone, ROW_GAP, ROW_H, ROW_RADIUS, SIDES,
};
use super::tab_thumbs::TabThumbs;
use super::theme::{self, palette, ROW_FONT};
use super::toolbar::{ICON, SLOT};
use crate::browser::HOME_URL;
use crate::clock;
use crate::command::{AppCommand, HubAction};
use crate::config::{FaceLabels, TabLayout};
use crate::data::bookmarks::Launch;
use crate::data::downloads::{format_progress, format_size, State};
use crate::data::session::TabInfo;
use crate::overlay::hub::{Hub, Section};
use egui_phosphor::{bold, fill};
use egui_sdl2::egui::{self, AtomExt as _};
use std::sync::Arc;

/// The panel's id, which its footer is kept under too.
const PANEL_ID: &str = "hub";

/// A row's delete action at `slot`, accent on the selected row.
fn delete_button(
    ui: &mut egui::Ui,
    slot: egui::Rect,
    selected: bool,
    dim: egui::Color32,
) -> egui::Response {
    let color = if selected { palette().accent } else { dim };
    row_action(ui, slot, theme::icon(bold::X).color(color))
}

/// A row's bookmark toggle at `slot`.
fn bookmark_button(
    ui: &mut egui::Ui,
    slot: egui::Rect,
    bookmarked: bool,
    dim: egui::Color32,
) -> egui::Response {
    let star = if bookmarked {
        theme::icon_fill(fill::STAR).color(palette().accent)
    } else {
        theme::icon(bold::STAR).color(dim)
    };
    row_action(ui, slot, star)
}

/// A row's action at `slot` inside the row: a bare glyph, framed under the
/// pointer to show which action a click hits.
fn row_action(ui: &mut egui::Ui, slot: egui::Rect, glyph: egui::RichText) -> egui::Response {
    let button = egui::Button::new(glyph.size(ICON))
        .corner_radius(ROW_RADIUS)
        .frame_when_inactive(false);
    ui.put(slot, button)
}

/// The `index`th action slot at `row`'s trailing end, counted from the far
/// end: a toolbar-sized square.
fn action_slot(row: egui::Rect, index: usize) -> egui::Rect {
    let right = row.right() - ROW_GAP - index as f32 * SLOT;
    egui::Rect::from_center_size(
        egui::pos2(right - SLOT / 2.0, row.center().y),
        egui::Vec2::splat(SLOT),
    )
}

/// `row` short of its `count` trailing actions.
fn actions_start(row: egui::Rect, count: usize) -> egui::Rect {
    row.with_max_x(row.right() - ROW_GAP - count as f32 * SLOT)
}

/// `button` with no frame at all unless `selected`: the pointer selects the row
/// it moves onto, so a hover frame would be a second highlight.
fn unframed_unless(button: egui::Button<'_>, selected: bool) -> egui::Button<'_> {
    match selected {
        true => button,
        false => button
            .fill(egui::Color32::TRANSPARENT)
            .stroke(egui::Stroke::NONE),
    }
}

/// Ask for row `index` to be highlighted when the pointer moved onto `area`,
/// the line that holds `row`.
fn follow_pointer(
    area: &egui::Response,
    row: &egui::Response,
    selected: bool,
    index: usize,
    commands: &mut Vec<AppCommand>,
) {
    if !selected && panel::pointer_moved_onto(area, row) {
        commands.push(AppCommand::Hub(HubAction::Select(index)));
    }
}

/// A full-width row that acts rather than lists: `glyph` and `label` in
/// `tone`'s colour, with `note` at its trailing edge.
fn action_row(
    ui: &mut egui::Ui,
    width: f32,
    selected: bool,
    glyph: &'static str,
    label: &str,
    note: &str,
    tone: Tone,
) -> egui::Response {
    let color = panel::tone_color(tone);
    let text = |text: &str, color| egui::RichText::new(text).color(color).size(ROW_FONT);
    let atoms = (
        text(glyph, color),
        text(label, color).atom_shrink(true),
        egui::Atom::grow(),
        text(note, palette().accent),
    );
    row_atoms(ui, width, selected, atoms)
}

/// A selectable list row at the standard height over `atoms`, which end in an
/// [`egui::Atom::grow`] or egui centres them. Mark one atom `shrink` —
/// truncation eats that one; egui otherwise picks the leading run.
fn row_atoms<'a>(
    ui: &mut egui::Ui,
    width: f32,
    selected: bool,
    atoms: impl egui::IntoAtoms<'a>,
) -> egui::Response {
    let button = egui::Button::selectable(selected, atoms)
        .corner_radius(ROW_RADIUS)
        .truncate();
    ui.add_sized([width, ROW_H], unframed_unless(button, selected))
}

/// A two-line row's step from its title line down to its detail line.
const LINE_STEP: f32 = ROW_FONT + ROW_GAP;

/// A two-line row: a one-line row's height plus a [`LINE_STEP`].
const ROW2_H: f32 = ROW_H + LINE_STEP;

/// A two-line row's icon, as tall as its two lines of type.
const ROW_ICON: f32 = 2.0 * ROW_FONT;

/// Air between a row's icon or trailing marks and its text.
const TEXT_GAP: f32 = 2.0 * ROW_GAP;

/// A selectable two-line row; the caller paints its content over it.
fn two_line_row(ui: &mut egui::Ui, width: f32, selected: bool) -> egui::Response {
    let button = egui::Button::selectable(selected, egui::Atom::grow()).corner_radius(ROW_RADIUS);
    ui.add_sized([width, ROW2_H], unframed_unless(button, selected))
}

/// Where a two-line row's parts go: its icon at the leading edge, and beside it
/// the title line over the detail line.
struct RowSlots {
    icon: egui::Rect,
    title: egui::Rect,
    detail: egui::Rect,
}

impl RowSlots {
    /// The slots of `row`, with room for an icon when `icon`; with no `detail`
    /// the title line is centred down the row.
    fn of(ui: &egui::Ui, row: egui::Rect, icon: bool, detail: bool) -> Self {
        let inner = row.shrink2(egui::vec2(ui.spacing().button_padding.x, 0.0));
        let mid = inner.center().y;
        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(inner.left() + ROW_ICON / 2.0, mid),
            egui::Vec2::splat(ROW_ICON),
        );
        let left = match icon {
            true => icon_rect.right() + TEXT_GAP,
            false => inner.left(),
        };
        let line = |y: f32| {
            let half = LINE_STEP / 2.0;
            egui::Rect::from_x_y_ranges(left..=inner.right(), (y - half)..=(y + half))
        };
        let step = if detail { LINE_STEP / 2.0 } else { 0.0 };
        Self {
            icon: icon_rect,
            title: line(mid - step),
            detail: line(mid + step),
        }
    }
}

/// `text` in the row font and `color` on one line, cut to `max_w` with an ellipsis.
fn one_line(
    painter: &egui::Painter,
    text: &str,
    color: egui::Color32,
    max_w: f32,
) -> Arc<egui::Galley> {
    let font = egui::FontId::proportional(ROW_FONT);
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap.max_width = max_w.max(0.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    painter.layout_job(job)
}

/// Paint `text` in `color` on one line at `align`'s side of `slot`, centred down
/// it and cut to its width; returns the rect the text took.
fn paint_line(
    painter: &egui::Painter,
    slot: egui::Rect,
    text: &str,
    color: egui::Color32,
    align: egui::Align,
) -> egui::Rect {
    let galley = one_line(painter, text, color, slot.width());
    let size = galley.size();
    let x = match align {
        egui::Align::Min => slot.left(),
        egui::Align::Center => slot.center().x - size.x / 2.0,
        egui::Align::Max => slot.right() - size.x,
    };
    let at = egui::pos2(x, slot.center().y - size.y / 2.0);
    painter.galley(at, galley, color);
    egui::Rect::from_min_size(at, size)
}

/// Paint `url` past its scheme, dim in `slot`, cut in the middle to fit.
fn paint_url(ui: &egui::Ui, slot: egui::Rect, url: &str) {
    let font = egui::FontId::proportional(ROW_FONT);
    let url = url_without_scheme(url).trim_end_matches('/');
    let text = elide_middle(ui, url, &font, slot.width());
    paint_line(ui.painter(), slot, &text, palette().dim, egui::Align::Min);
}

/// Paint `icon` to fill the square `rect`, a picture keeping its shape.
fn paint_icon(painter: &egui::Painter, rect: egui::Rect, icon: ListIcon) {
    match icon {
        ListIcon::Image(texture) => {
            let size = texture.size * (rect.size() / texture.size).min_elem();
            let at = egui::Rect::from_center_size(rect.center(), size);
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(texture.id, at, uv, egui::Color32::WHITE);
        }
        ListIcon::Glyph(glyph, color) => {
            let font = egui::FontId::proportional(rect.height());
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                glyph,
                font,
                color,
            );
        }
    }
}

/// Rendered width of `text` in `font` (logical px).
fn text_width(ui: &egui::Ui, text: &str, font: &egui::FontId) -> f32 {
    ui.ctx().fonts_mut(|f| {
        f.layout_no_wrap(text.to_owned(), font.clone(), palette().dim)
            .size()
            .x
    })
}

/// Shorten `text` to `budget` px by cutting its middle. egui truncates at the
/// end only, which drops the part of a URL that identifies the page.
fn elide_middle(ui: &egui::Ui, text: &str, font: &egui::FontId, budget: f32) -> String {
    if budget <= 0.0 || text_width(ui, text, font) <= budget {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    // Binary search the number of kept chars: the width is monotonic in it.
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if text_width(ui, &join_around_ellipsis(&chars, mid), font) <= budget {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    join_around_ellipsis(&chars, lo)
}

/// `keep` chars around a central ellipsis: a third leading, the rest trailing.
fn join_around_ellipsis(chars: &[char], keep: usize) -> String {
    let head = keep / 3;
    let tail = keep - head;
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(&chars[chars.len() - tail..]);
    out
}

/// The leading clear row of History / Downloads. Two presses: the first arms it
/// and says so, the second wipes the list. `enabled` mutes it when there is
/// nothing to clear.
fn clear_row(
    ui: &mut egui::Ui,
    width: f32,
    label: &str,
    selected: bool,
    armed: bool,
    enabled: bool,
    commands: &mut Vec<AppCommand>,
) {
    let note = if armed { "press again to clear" } else { "" };
    let tone = if enabled { Tone::Danger } else { Tone::Muted };
    let resp = action_row(ui, width, selected, bold::TRASH, label, note, tone);
    if selected {
        center_selected(&resp);
    }
    follow_pointer(&resp, &resp, selected, 0, commands);
    if resp.clicked() {
        commands.push(AppCommand::Hub(HubAction::Clear));
    }
}

/// `url` past its scheme.
fn url_without_scheme(url: &str) -> &str {
    url.split_once("://").map_or(url, |(_, rest)| rest)
}

/// A Hub section's icon, the same as the Menu row that opens it.
fn section_icon(section: Section) -> &'static str {
    match section {
        Section::Tabs => bold::TABS,
        Section::Bookmarks => bold::BOOKMARKS,
        Section::History => bold::CLOCK_COUNTER_CLOCKWISE,
        Section::Downloads => bold::DOWNLOAD_SIMPLE,
    }
}

/// A section's indicator: how many tabs are open, how far the downloads in
/// flight are.
fn section_indicator(hub: &Hub, tabs: &[TabInfo], section: Section) -> Option<String> {
    match section {
        Section::Tabs => Some(tabs.len().to_string()),
        Section::Downloads => hub.downloads.progress().map(|p| p.indicator()),
        Section::Bookmarks | Section::History => None,
    }
}

/// What A, X and Y do on the highlighted row; `None` where a button does nothing.
type RowVerbs = [Option<&'static str>; 3];

/// The verbs of the highlighted row in the active section.
fn row_verbs(hub: &Hub, tabs: &[TabInfo]) -> RowVerbs {
    let bookmark = |url: &str| match hub.is_bookmarked(url) {
        true => "unbookmark",
        false => "bookmark",
    };
    let clear = || match hub.clear_armed() {
        true => "confirm",
        false => "clear",
    };
    match hub.section() {
        Section::Tabs => match hub.tab_selected().checked_sub(1).and_then(|i| tabs.get(i)) {
            Some(tab) => {
                let y = hub.can_toggle_bookmark(tab).then(|| bookmark(&tab.url));
                [Some("switch"), Some("close"), y]
            }
            None => [Some("new tab"), None, None],
        },
        Section::Bookmarks => match hub.bookmarks.selected_index() {
            Some(at) => {
                let open = match hub.bookmarks.all()[at].launch {
                    Launch::Site => "open",
                    Launch::App | Launch::Game => "launch",
                };
                [Some(open), Some("remove"), None]
            }
            None => [None; 3],
        },
        Section::History if hub.clear_selected() => [Some(clear()), None, None],
        Section::History => match hub.selected_url() {
            Some(url) => [Some("open"), Some("remove"), Some(bookmark(&url))],
            None => [None; 3],
        },
        Section::Downloads if hub.clear_selected() => {
            let a = hub.downloads.has_finished().then(clear);
            [a, None, None]
        }
        Section::Downloads => match hub.downloads.selected_entry() {
            Some(at) => {
                let a = hub.downloads.saved_path(at).map(|_| "open");
                let x = match hub.downloads.items()[at].is_active() {
                    true => "cancel",
                    false => "remove",
                };
                let y = hub.downloads.failed_url(at).map(|_| "retry");
                [a, Some(x), y]
            }
            None => [None; 3],
        },
    }
}

/// What the Hub draws besides its own state.
pub(super) struct HubView<'a> {
    pub tabs: &'a [TabInfo],
    pub icons: &'a PageIcons,
    pub thumbs: &'a TabThumbs,
    pub tab_layout: TabLayout,
    pub face: FaceLabels,
    /// Whether a footer names what the buttons do.
    pub button_hints: bool,
}

/// Draw the Hub: the section bar over the active section's list, and
/// with `button_hints` a footer naming what the buttons do on the highlighted
/// row. Left/Right switch section, Up/Down move, A open, X delete, B close.
pub(super) fn add_hub(
    ctx: &egui::Context,
    hub: &mut Hub,
    view: &HubView,
    commands: &mut Vec<AppCommand>,
) {
    let HubView {
        tabs,
        icons,
        face,
        button_hints,
        ..
    } = *view;
    let screen = ctx.content_rect();
    let closed = panel::panel(ctx, PANEL_ID, screen, |ui| {
        let clicked = panel::section_bar(
            ui,
            screen,
            &Section::ALL,
            hub.section(),
            Section::label,
            section_icon,
            |section| section_indicator(hub, tabs, section),
        );
        if let Some(section) = clicked {
            commands.push(AppCommand::Hub(HubAction::SetSection(section)));
        }
        ui.add_space(ROW_GAP * 2.0);
        let foot = match button_hints {
            true => panel::footer_room(ui, PANEL_ID),
            false => 0.0,
        };

        match (hub.section(), view.tab_layout) {
            (Section::Tabs, TabLayout::List) => {
                hub.set_tab_cols(1);
                add_tabs_section(ui, screen, foot, hub, tabs, icons, commands);
            }
            (Section::Tabs, TabLayout::Grid) => add_tab_grid(ui, screen, foot, hub, view, commands),
            (Section::Bookmarks, _) => {
                add_bookmarks_section(ui, screen, foot, hub, icons, commands)
            }
            (Section::History, _) => add_history_section(ui, screen, foot, hub, icons, commands),
            (Section::Downloads, _) => add_downloads_section(ui, screen, foot, hub, commands),
        }

        if button_hints {
            let (_, sideways) = keys::dpad_caps();
            let [a, x, y] = row_verbs(hub, tabs);
            let caps = [[face.a], [face.x], [face.y]];
            let mut hints: Vec<keys::ButtonHint> = caps
                .iter()
                .zip([a, x, y])
                .filter_map(|(cap, verb)| Some((&cap[..], verb?)))
                .collect();
            // A grid takes Left/Right across its columns.
            let sections: &[&str] = match hub.section() == Section::Tabs && hub.tab_grid() {
                true => &keys::SHOULDERS,
                false => &[sideways.as_str()],
            };
            let back = [face.b];
            hints.push((sections, "section"));
            hints.push((&back, "back"));
            panel::footer(ui, screen, PANEL_ID, &hints);
        }
    });
    if closed {
        commands.push(AppCommand::Hub(HubAction::Close));
    }
}

/// Tabs section: a leading "+ New tab" row (selection index 0) over the open tabs
/// (indices `1..=tabs.len()`), each with a bookmark toggle and a close button.
fn add_tabs_section(
    ui: &mut egui::Ui,
    screen: egui::Rect,
    foot: f32,
    hub: &Hub,
    tabs: &[TabInfo],
    icons: &PageIcons,
    commands: &mut Vec<AppCommand>,
) {
    let dim = palette().dim;
    let selected = hub.tab_selected();
    section_scroll(ui, screen, foot).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        let full_w = screen.width() - SIDES;
        let new_tab = action_row(
            ui,
            full_w,
            selected == 0,
            bold::PLUS,
            "New tab",
            "",
            Tone::Plain,
        );
        if selected == 0 {
            center_selected(&new_tab);
        }
        follow_pointer(&new_tab, &new_tab, selected == 0, 0, commands);
        if new_tab.clicked() {
            commands.push(AppCommand::Hub(HubAction::NewTab));
        }

        for (i, tab) in tabs.iter().enumerate() {
            let sel = selected == i + 1; // index 0 is the "+ New tab" button
            let resp = two_line_row(ui, full_w, sel);
            let icon = tab_icon(icons, tab);
            let home = tab.url == HOME_URL;
            let text = actions_start(resp.rect, 2);
            let slots = RowSlots::of(ui, text, icon.is_some(), !home);
            let painter = ui.painter();
            if let Some(icon) = icon {
                paint_icon(painter, slots.icon, icon);
            }
            let mut title = slots.title;
            if tab.active {
                let check = paint_line(
                    painter,
                    title,
                    bold::CHECK,
                    palette().accent,
                    egui::Align::Max,
                );
                title.max.x = check.left() - TEXT_GAP;
            }
            paint_line(painter, title, &tab.title, palette().text, egui::Align::Min);
            if !home {
                paint_url(ui, slots.detail, &tab.url);
            }
            if sel {
                center_selected(&resp);
            }
            if resp.clicked() {
                commands.push(AppCommand::Hub(HubAction::OpenTab(i)));
            }
            // Close sits at the far right, so the toggle's slot stays when it can't act.
            let marked = hub.is_bookmarked(&tab.url);
            let star_slot = action_slot(resp.rect, 1);
            if !hub.can_toggle_bookmark(tab) {
                if marked {
                    let star = theme::icon_fill(fill::STAR)
                        .size(ICON)
                        .color(palette().accent);
                    ui.put(star_slot, egui::Label::new(star).selectable(false));
                }
            } else if bookmark_button(ui, star_slot, marked, dim).clicked() {
                commands.push(AppCommand::Hub(HubAction::ToggleBookmark(tab.url.clone())));
            }
            if delete_button(ui, action_slot(resp.rect, 0), sel, dim).clicked() {
                commands.push(AppCommand::Hub(HubAction::CloseTab(i)));
            }
            follow_pointer(&resp, &resp, sel, i + 1, commands);
        }
    });
}

/// Narrowest a tab card gets before the grid drops a column.
const CARD_MIN_W: f32 = 180.0;
/// Space between cards, across and down.
const CARD_GAP: f32 = 12.0;
/// The close button in a card's header.
const CARD_CLOSE: f32 = 24.0;
/// The glyph a card without a picture shows, for Home or a new tab.
const CARD_GLYPH: f32 = 40.0;
/// A card's corner, shared by its header's top and its picture's bottom.
const CARD_CORNER: u8 = ROW_RADIUS as u8;
/// A card header's corners, over its picture.
const HEAD_CORNERS: egui::CornerRadius = egui::CornerRadius {
    nw: CARD_CORNER,
    ne: CARD_CORNER,
    sw: 0,
    se: 0,
};
/// A card picture's corners, under its header.
const PICTURE_CORNERS: egui::CornerRadius = egui::CornerRadius {
    nw: 0,
    ne: 0,
    sw: CARD_CORNER,
    se: CARD_CORNER,
};

/// Tabs section as a grid of cards: a leading "New tab" card (selection index
/// 0) before the open tabs, each with a picture of its page and a close button.
fn add_tab_grid(
    ui: &mut egui::Ui,
    screen: egui::Rect,
    foot: f32,
    hub: &mut Hub,
    view: &HubView,
    commands: &mut Vec<AppCommand>,
) {
    let full_w = screen.width() - SIDES;
    let cols = ((full_w + CARD_GAP) / (CARD_MIN_W + CARD_GAP))
        .floor()
        .max(1.0) as usize;
    hub.set_tab_cols(cols);
    let card_w = (full_w - (cols - 1) as f32 * CARD_GAP) / cols as f32;
    // The pictures' shape, else the screen's, which the page's is near.
    let aspect = view
        .tabs
        .iter()
        .find_map(|tab| tab.thumbnail.as_ref())
        .map_or(screen.height() / screen.width(), |thumb| {
            thumb.height as f32 / thumb.width as f32
        });
    let size = egui::vec2(card_w, ROW_H + (card_w * aspect).round());
    let selected = hub.tab_selected();
    let count = view.tabs.len() + 1;
    section_scroll(ui, screen, foot).show(ui, |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(CARD_GAP, CARD_GAP);
        for row in 0..count.div_ceil(cols) {
            ui.horizontal(|ui| {
                for index in row * cols..count.min((row + 1) * cols) {
                    // Index 0 is the "New tab" card.
                    let tab = index.checked_sub(1).map(|i| (i, &view.tabs[i]));
                    add_tab_card(ui, size, tab, index == selected, view, commands);
                }
            });
        }
    });
}

/// One card of the tab grid: a header with `tab`'s icon, title and close
/// button over its picture, or the "New tab" card for `None`.
fn add_tab_card(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    tab: Option<(usize, &TabInfo)>,
    selected: bool,
    view: &HubView,
    commands: &mut Vec<AppCommand>,
) {
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let (head, picture) = rect.split_top_bottom_at_y(rect.top() + ROW_H);
    let head_fill = match selected {
        true => ui.visuals().selection.bg_fill,
        false => palette().surface,
    };
    ui.painter().rect_filled(head, HEAD_CORNERS, head_fill);
    // Centred in the header, as far from its right edge as from its top.
    let inset = (ROW_H - CARD_CLOSE) / 2.0;
    let close = egui::Rect::from_min_size(
        head.right_top() + egui::vec2(-inset - CARD_CLOSE, inset),
        egui::Vec2::splat(CARD_CLOSE),
    );
    match tab {
        None => {
            paint_card_glyph(ui.painter(), picture, bold::PLUS);
            let plus = ListIcon::Glyph(bold::PLUS, palette().text);
            paint_card_title(ui.painter(), head, Some(plus), "New tab", false);
        }
        Some((_, tab)) => {
            paint_tab_picture(ui, picture, tab, view, selected);
            let icon = tab_icon(view.icons, tab);
            let title = head.with_max_x(close.left());
            paint_card_title(ui.painter(), title, icon, &tab.title, tab.active);
        }
    }
    let stroke = card_stroke(selected);
    ui.painter().hline(rect.x_range(), head.bottom(), stroke);
    ui.painter()
        .rect_stroke(rect, ROW_RADIUS, stroke, egui::StrokeKind::Inside);
    if selected {
        center_selected(&resp);
    }
    let index = tab.map_or(0, |(i, _)| i + 1);
    follow_pointer(&resp, &resp, selected, index, commands);
    if resp.clicked() {
        let action = match tab {
            Some((i, _)) => HubAction::OpenTab(i),
            None => HubAction::NewTab,
        };
        commands.push(AppCommand::Hub(action));
    }
    if let Some((i, _)) = tab {
        let id = egui::Id::new(("tab_card_close", i));
        if close_cross(ui, close, id, selected).clicked() {
            commands.push(AppCommand::Hub(HubAction::CloseTab(i)));
        }
    }
}

/// A card's outline and its header's underline, heavier and in the accent
/// on the selected card.
fn card_stroke(selected: bool) -> egui::Stroke {
    match selected {
        true => egui::Stroke::new(2.0, palette().accent),
        false => egui::Stroke::new(1.0, palette().border),
    }
}

/// A card header's close button: a cross on a disc shown under the pointer.
/// Stroked: an icon glyph sits on its font's line, off the button's centre.
fn close_cross(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    id: egui::Id,
    selected: bool,
) -> egui::Response {
    let resp = ui.interact(rect, id, egui::Sense::click());
    let color = if selected {
        palette().accent
    } else {
        palette().text
    };
    let painter = ui.painter();
    let center = rect.center();
    if resp.hovered() {
        painter.circle_filled(center, rect.width() / 2.0, palette().panel_fill);
    }
    let arm = rect.width() * CROSS_ARM;
    let stroke = egui::Stroke::new(CROSS_STROKE, color);
    for dir in [egui::vec2(arm, arm), egui::vec2(arm, -arm)] {
        painter.line_segment([center - dir, center + dir], stroke);
    }
    resp
}

/// Half a close cross's diagonal, as a share of its disc's width.
const CROSS_ARM: f32 = 0.18;
/// A close cross's line width.
const CROSS_STROKE: f32 = 2.0;

/// Paint `tab`'s picture into a card's lower `rect`: its kept frame, else a
/// stand-in for Home or its site.
fn paint_tab_picture(
    ui: &egui::Ui,
    rect: egui::Rect,
    tab: &TabInfo,
    view: &HubView,
    selected: bool,
) {
    let texture = tab
        .thumbnail
        .as_ref()
        .and_then(|thumb| view.thumbs.texture(thumb));
    match texture {
        Some(texture) => {
            egui::Image::new(texture)
                .uv(cover_uv(texture.size_vec2(), rect.size()))
                .corner_radius(PICTURE_CORNERS)
                .paint_at(ui, rect);
        }
        None if tab.url == HOME_URL => paint_card_glyph(ui.painter(), rect, bold::BROWSER),
        None => {
            let label = labels::brand_label(&tab.url);
            let icon = view.icons.icon(&tab.url);
            let painter = ui.painter();
            super::home::paint_site_card(painter, rect, PICTURE_CORNERS, &label, icon, selected);
        }
    }
}

/// The part of a `texture`-sized picture that fills a `frame`-shaped box:
/// whole across or down, cut evenly at the sides or from the bottom.
fn cover_uv(texture: egui::Vec2, frame: egui::Vec2) -> egui::Rect {
    let scale = (frame.x / texture.x).max(frame.y / texture.y);
    let seen = frame / scale / texture;
    egui::Rect::from_min_size(egui::pos2((1.0 - seen.x) / 2.0, 0.0), seen)
}

/// A card's lower `rect` with no picture: `glyph` centred on the surface.
fn paint_card_glyph(painter: &egui::Painter, rect: egui::Rect, glyph: &str) {
    painter.rect_filled(rect, PICTURE_CORNERS, palette().surface);
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(CARD_GLYPH),
        palette().dim,
    );
}

/// A card's title row: `icon`, `title` cut to the row, and a check when the
/// tab is the `shown` one.
fn paint_card_title(
    painter: &egui::Painter,
    rect: egui::Rect,
    icon: Option<ListIcon>,
    title: &str,
    shown: bool,
) {
    let mut left = rect.left() + ROW_GAP;
    if let Some(icon) = icon {
        let side = egui::Vec2::splat(ROW_FONT);
        let at =
            egui::Rect::from_center_size(egui::pos2(left + ROW_FONT / 2.0, rect.center().y), side);
        paint_icon(painter, at, icon);
        left = at.right() + TEXT_GAP;
    }
    let mut line = rect.with_min_x(left).with_max_x(rect.right() - ROW_GAP);
    if shown {
        let check = paint_line(
            painter,
            line,
            bold::CHECK,
            palette().accent,
            egui::Align::Max,
        );
        line.max.x = check.left() - TEXT_GAP;
    }
    paint_line(painter, line, title, palette().text, egui::Align::Min);
}

/// A card's or row's icon: the site's picture, or a glyph in its colour.
enum ListIcon {
    Image(egui::load::SizedTexture),
    Glyph(&'static str, egui::Color32),
}

/// `tab`'s icon on its card, as on its row: the site's, a globe without one,
/// the tab glyph for Home; `None` with icons off.
fn tab_icon(icons: &PageIcons, tab: &TabInfo) -> Option<ListIcon> {
    match tab.url == HOME_URL {
        true => icons
            .enabled()
            .then(|| ListIcon::Glyph(bold::BROWSER, palette().text)),
        false => page_icon(icons, &tab.url),
    }
}

/// A page's icon: its site's, or a globe without one; `None` with icons off.
fn page_icon(icons: &PageIcons, url: &str) -> Option<ListIcon> {
    if !icons.enabled() {
        return None;
    }
    Some(match icons.icon(url) {
        Some(icon) => ListIcon::Image(egui::load::SizedTexture::from_handle(icon.row)),
        None => ListIcon::Glyph(bold::GLOBE, palette().dim),
    })
}

/// A row's leading site icon, or a globe so titles line up; `None` with icons off.
pub(super) fn site_icon(icons: &PageIcons, url: &str) -> Option<egui::Atom<'static>> {
    if !icons.enabled() {
        return None;
    }
    let side = egui::vec2(ROW_FONT, ROW_FONT);
    Some(match icons.icon(url) {
        Some(icon) => egui::Image::new(icon.row).fit_to_exact_size(side).into(),
        None => egui::RichText::new(bold::GLOBE)
            .size(ROW_FONT)
            .color(palette().dim)
            .into(),
    })
}

/// Bookmarks section: the bookmarks in manual order, highlighted row selected;
/// a named one shows its name and address, an unnamed one its site and path.
fn add_bookmarks_section(
    ui: &mut egui::Ui,
    screen: egui::Rect,
    foot: f32,
    hub: &Hub,
    icons: &PageIcons,
    commands: &mut Vec<AppCommand>,
) {
    let dim = palette().dim;
    let bookmarks = &hub.bookmarks;
    if bookmarks.all().is_empty() {
        ui.label(
            egui::RichText::new("No bookmarks yet. Bookmark a page from Quick Access.").color(dim),
        );
        return;
    }

    // Fixed widths derived from the screen (not `ui.available_width()`, which is
    // unreliable inside a scroll area and made the list jump horizontally).
    let row_w = screen.width() - SIDES;
    section_scroll(ui, screen, foot).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        for (i, bookmark) in bookmarks.all().iter().enumerate() {
            let selected = i == bookmarks.selected();
            let url = &bookmark.url;
            let resp = two_line_row(ui, row_w, selected);
            let icon = page_icon(icons, url);
            let text = actions_start(resp.rect, 1);
            let slots = RowSlots::of(ui, text, icon.is_some(), true);
            let painter = ui.painter();
            if let Some(icon) = icon {
                paint_icon(painter, slots.icon, icon);
            }
            let mut title = slots.title;
            let tags_budget = row_w * super::home::TAGS_SHARE;
            let tags_w = super::home::tags_width(painter, &bookmark.tags, tags_budget);
            if tags_w > 0.0 {
                let y = title.center().y;
                super::home::paint_tags(painter, &bookmark.tags, title.right(), y, tags_budget);
                title.max.x -= tags_w + TEXT_GAP;
            }
            let label = labels::bookmark_label(bookmark);
            let text = palette().text;
            match bookmark.launch {
                // Sites are the default and go unmarked.
                Launch::Site => {
                    paint_line(painter, title, &label, text, egui::Align::Min);
                }
                launch => {
                    let (glyph, color) = super::home::launch_glyph(launch);
                    let gap = ui.spacing().icon_spacing;
                    let mark_w = one_line(painter, glyph, color, f32::INFINITY).size().x;
                    let room = title.with_max_x(title.right() - mark_w - gap);
                    let name = paint_line(painter, room, &label, text, egui::Align::Min);
                    let at = title.with_min_x(name.right() + gap);
                    paint_line(painter, at, glyph, color, egui::Align::Min);
                }
            }
            paint_url(ui, slots.detail, url);
            if selected {
                center_selected(&resp);
            }
            if resp.clicked() {
                commands.push(AppCommand::Hub(HubAction::OpenBookmark(i)));
            }
            if delete_button(ui, action_slot(resp.rect, 0), selected, dim).clicked() {
                commands.push(AppCommand::Hub(HubAction::RemoveAt(i)));
            }
            follow_pointer(&resp, &resp, selected, i, commands);
        }
    });
}

/// Downloads section: most-recent first, each row showing the file name and a
/// status (progress, size + date, or the error). Delete cancels or removes the
/// entry (the file on disk is kept); a finished row opens the file.
fn add_downloads_section(
    ui: &mut egui::Ui,
    screen: egui::Rect,
    foot: f32,
    hub: &Hub,
    commands: &mut Vec<AppCommand>,
) {
    let dim = palette().dim;
    let downloads = &hub.downloads;
    if downloads.items().is_empty() {
        ui.label(egui::RichText::new("No downloads yet.").color(dim));
        return;
    }

    let row_w = screen.width() - SIDES;
    let today = clock::local_day(clock::now_unix());
    section_scroll(ui, screen, foot).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        clear_row(
            ui,
            screen.width() - SIDES,
            "Clear finished",
            downloads.selected() == 0,
            hub.clear_armed(),
            downloads.has_finished(),
            commands,
        );
        let mut last_day = None;
        for (i, item) in downloads.items().iter().enumerate() {
            // A download in flight runs today; its time is set once it ends.
            let day = match (&item.state, item.time) {
                (State::Active, _) => Some(today),
                (_, 0) => None,
                (_, time) => Some(clock::local_day(time)),
            };
            if i == 0 || day != last_day {
                panel::head(ui, &labels::day_label(day, today));
            }
            last_day = day;
            let selected = downloads.selected() == i + 1; // index 0 is "Clear finished"
            let (name_color, status, status_color) = match &item.state {
                State::Active => (
                    palette().text,
                    format_progress(item.received, item.total),
                    dim,
                ),
                State::Done => {
                    let time = labels::time_label(item.time);
                    let status = format!("{} · {time}", format_size(item.received));
                    (palette().text, status, dim)
                }
                State::Failed(error) => (dim, error.clone(), palette().warn),
            };
            let resp = two_line_row(ui, row_w, selected);
            if item.is_active() {
                paint_progress(ui.painter(), resp.rect, item.received, item.total);
            }
            let slots = RowSlots::of(ui, actions_start(resp.rect, 1), true, true);
            let painter = ui.painter();
            let glyph = ListIcon::Glyph(file_glyph(&item.filename), dim);
            paint_icon(painter, slots.icon, glyph);
            paint_line(
                painter,
                slots.title,
                &item.filename,
                name_color,
                egui::Align::Min,
            );
            paint_line(
                painter,
                slots.detail,
                &status,
                status_color,
                egui::Align::Min,
            );
            if selected {
                center_selected(&resp);
            }
            if resp.clicked() && downloads.saved_path(i).is_some() {
                commands.push(AppCommand::Hub(HubAction::OpenDownload(i)));
            }
            if delete_button(ui, action_slot(resp.rect, 0), selected, dim).clicked() {
                commands.push(AppCommand::Hub(HubAction::RemoveAt(i)));
            }
            follow_pointer(&resp, &resp, selected, i + 1, commands);
        }
    });
}

/// A download row's bar along its foot: how much of `total` has arrived;
/// none while the size is unknown.
fn paint_progress(painter: &egui::Painter, row: egui::Rect, received: u64, total: u64) {
    if total == 0 {
        return;
    }
    let track = egui::Rect::from_min_max(
        egui::pos2(row.left() + ROW_RADIUS, row.bottom() - PROGRESS_H),
        egui::pos2(row.right() - ROW_RADIUS, row.bottom()),
    );
    let radius = PROGRESS_H / 2.0;
    painter.rect_filled(track, radius, palette().hairline);
    let share = received.min(total) as f32 / total as f32;
    let done = egui::Rect::from_min_size(track.min, egui::vec2(track.width() * share, PROGRESS_H));
    painter.rect_filled(done, radius, palette().accent);
}

/// A download's progress bar height.
const PROGRESS_H: f32 = 2.0;

/// A file's icon by the kind its name's extension says it is.
fn file_glyph(filename: &str) -> &'static str {
    let extension = filename
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "zip" | "7z" | "rar" | "tar" | "gz" | "tgz" | "xz" | "bz2" | "zst" => bold::FILE_ZIP,
        "pdf" => bold::FILE_PDF,
        "mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v" => bold::FILE_VIDEO,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" => bold::FILE_IMAGE,
        "mp3" | "ogg" | "opus" | "flac" | "wav" | "m4a" => bold::FILE_AUDIO,
        "txt" | "md" | "log" | "json" | "toml" | "ini" | "cfg" => bold::FILE_TEXT,
        "apk" => bold::ANDROID_LOGO,
        _ => bold::FILE,
    }
}

/// History section: visited URLs (most-recent first) with their visit date, over
/// a leading "Clear all" row.
fn add_history_section(
    ui: &mut egui::Ui,
    screen: egui::Rect,
    foot: f32,
    hub: &Hub,
    icons: &PageIcons,
    commands: &mut Vec<AppCommand>,
) {
    let dim = palette().dim;
    let hist = hub.history();
    if hist.entries().is_empty() {
        ui.label(egui::RichText::new("No history yet.").color(dim));
        return;
    }

    let row_w = screen.width() - SIDES;
    let today = clock::local_day(clock::now_unix());
    section_scroll(ui, screen, foot).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        // Top row (cursor index 0, mirroring Tabs' "+ New tab"): drops every
        // entry.
        clear_row(
            ui,
            screen.width() - SIDES,
            "Clear all",
            hist.selected() == 0,
            hub.clear_armed(),
            true,
            commands,
        );
        let mut last_day = None;
        for (i, entry) in hist.entries().iter().enumerate() {
            let day = (entry.time != 0).then(|| clock::local_day(entry.time));
            if i == 0 || day != last_day {
                panel::head(ui, &labels::day_label(day, today));
            }
            last_day = day;
            let selected = hist.selected() == i + 1; // index 0 is "Clear all"
            let resp = two_line_row(ui, row_w, selected);
            let icon = page_icon(icons, &entry.url);
            let text = actions_start(resp.rect, 2);
            let slots = RowSlots::of(ui, text, icon.is_some(), true);
            let painter = ui.painter();
            if let Some(icon) = icon {
                paint_icon(painter, slots.icon, icon);
            }
            let brand;
            let title = match entry.title.is_empty() {
                true => {
                    brand = labels::brand_label(&entry.url);
                    &brand
                }
                false => &entry.title,
            };
            paint_line(
                painter,
                slots.title,
                title,
                palette().text,
                egui::Align::Min,
            );
            let time = labels::time_label(entry.time);
            let time = paint_line(painter, slots.detail, &time, dim, egui::Align::Max);
            paint_url(
                ui,
                slots.detail.with_max_x(time.left() - TEXT_GAP),
                &entry.url,
            );
            if selected {
                center_selected(&resp);
            }
            if resp.clicked() {
                commands.push(AppCommand::Hub(HubAction::OpenUrl(entry.url.clone())));
            }
            // Trailing actions: the bookmark toggle (same as Y), then delete.
            let marked = hub.is_bookmarked(&entry.url);
            if bookmark_button(ui, action_slot(resp.rect, 1), marked, dim).clicked() {
                commands.push(AppCommand::Hub(HubAction::ToggleBookmark(
                    entry.url.clone(),
                )));
            }
            if delete_button(ui, action_slot(resp.rect, 0), selected, dim).clicked() {
                commands.push(AppCommand::Hub(HubAction::RemoveAt(i)));
            }
            follow_pointer(&resp, &resp, selected, i + 1, commands);
        }
    });
}
