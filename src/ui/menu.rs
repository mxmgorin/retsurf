//! Rendering of the full-screen menu overlay (state lives in [`crate::overlay::menu`]):
//! the section bar with the close action, and the four section lists
//! (Tabs / Bookmarks / History / Downloads).

use super::favicon::PageIcons;
use super::keys;
use super::panel::{
    self, center_selected, section_scroll_over, Tone, ROW_GAP, ROW_H, ROW_RADIUS, SIDES,
};
use super::tab_thumbs::TabThumbs;
use super::theme::{self, palette, ROW_FONT};
use crate::browser::HOME_URL;
use crate::command::{AppCommand, MenuAction};
use crate::config::{FaceLabels, TabLayout};
use crate::data::bookmarks::Launch;
use crate::data::history;
use crate::data::session::TabInfo;
use crate::overlay::menu::{Menu, Section};
use egui_phosphor::{bold, fill};
use egui_sdl2::egui::{self, AtomExt as _};

/// The square delete button leading each row.
const DEL_W: f32 = 26.0;

/// The panel's id, which its footer is kept under too.
const PANEL_ID: &str = "menu";

/// A row's trailing delete button, framed and accent on the selected row.
fn delete_button(ui: &mut egui::Ui, selected: bool, dim: egui::Color32) -> egui::Response {
    let color = if selected { palette().accent } else { dim };
    row_action(ui, selected, theme::icon(bold::X).color(color))
}

/// A row's bookmark toggle (Tabs / History), sized like [`delete_button`] so the
/// trailing buttons line up across rows.
fn bookmark_button(
    ui: &mut egui::Ui,
    selected: bool,
    bookmarked: bool,
    dim: egui::Color32,
) -> egui::Response {
    let star = if bookmarked {
        theme::icon_fill(fill::STAR).color(palette().accent)
    } else {
        theme::icon(bold::STAR).color(dim)
    };
    row_action(ui, selected, star)
}

/// A row's trailing button: framed on the selected row, a bare glyph elsewhere.
fn row_action(ui: &mut egui::Ui, selected: bool, glyph: egui::RichText) -> egui::Response {
    let button = egui::Button::new(glyph)
        .corner_radius(ROW_RADIUS)
        .frame_when_inactive(selected);
    ui.add_sized([DEL_W, ROW_H], unframed_unless(button, selected))
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
        commands.push(AppCommand::Menu(MenuAction::Select(index)));
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

/// A selectable list row at the standard height, with its label left-aligned:
/// the trailing [`egui::Atom::grow`] fills the rest of the row, where egui
/// would otherwise center the text.
fn row_button(
    ui: &mut egui::Ui,
    width: f32,
    selected: bool,
    text: egui::RichText,
) -> egui::Response {
    row_atoms(
        ui,
        width,
        selected,
        (text.size(ROW_FONT), egui::Atom::grow()),
    )
}

/// [`row_button`] over pre-built atoms (a multi-color label). Mark one atom
/// `shrink` — truncation eats that one; egui otherwise picks the leading run.
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

/// A URL row's label: site name in white, rest of the URL dim and middle-elided
/// to `width`. Leading with the name makes the list scannable; keeping both ends
/// of the path keeps what differs between two rows of the same site.
fn url_atoms(
    ui: &egui::Ui,
    brand_text: String,
    tail: &str,
    icon: Option<egui::Atom<'static>>,
    width: f32,
) -> egui::Atoms<'static> {
    let font = egui::FontId::proportional(ROW_FONT);
    // Row padding, the gap after the brand, and the icon when present.
    let mut budget = width - text_width(ui, &brand_text, &font) - 24.0;
    if icon.is_some() {
        budget -= ROW_FONT + 4.0;
    }
    let brand = egui::RichText::new(brand_text)
        .size(ROW_FONT)
        .color(palette().text);
    let tail = egui::RichText::new(elide_middle(ui, tail, &font, budget))
        .size(ROW_FONT)
        .color(palette().dim);
    let mut atoms = egui::Atoms::new((brand, tail.atom_shrink(true), egui::Atom::grow()));
    if let Some(icon) = icon {
        atoms.push_left(icon);
    }
    atoms
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
        commands.push(AppCommand::Menu(MenuAction::Clear));
    }
}

/// `url` past its scheme.
fn url_without_scheme(url: &str) -> &str {
    url.split_once("://").map_or(url, |(_, rest)| rest)
}

/// Path, query and fragment of `url`; empty for a bare host.
fn url_tail(url: &str) -> &str {
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let tail = after_scheme.find('/').map_or("", |i| &after_scheme[i..]);
    if tail == "/" {
        ""
    } else {
        tail
    }
}

/// A lists-menu tab's icon, the same as the Quick Menu row that opens it.
fn section_icon(section: Section) -> &'static str {
    match section {
        Section::Tabs => bold::TABS,
        Section::Bookmarks => bold::BOOKMARKS,
        Section::History => bold::CLOCK_COUNTER_CLOCKWISE,
        Section::Downloads => bold::DOWNLOAD_SIMPLE,
    }
}

/// What a section's tab shows after its label: how many tabs are open, how
/// far the downloads in flight are.
fn section_badge(menu: &Menu, tabs: &[TabInfo], section: Section) -> Option<String> {
    match section {
        Section::Tabs => Some(tabs.len().to_string()),
        Section::Downloads => menu.downloads.progress().map(|p| p.label()),
        Section::Bookmarks | Section::History => None,
    }
}

/// What A, X and Y do on the highlighted row; `None` where a button does nothing.
type RowVerbs = [Option<&'static str>; 3];

/// The verbs of the highlighted row in the active section.
fn row_verbs(menu: &Menu, tabs: &[TabInfo]) -> RowVerbs {
    let bookmark = |url: &str| match menu.is_bookmarked(url) {
        true => "unbookmark",
        false => "bookmark",
    };
    let clear = || match menu.clear_armed() {
        true => "confirm",
        false => "clear",
    };
    match menu.section() {
        Section::Tabs => match menu.tab_selected().checked_sub(1).and_then(|i| tabs.get(i)) {
            Some(tab) => {
                let y = menu.can_toggle_bookmark(tab).then(|| bookmark(&tab.url));
                [Some("switch"), Some("close"), y]
            }
            None => [Some("new tab"), None, None],
        },
        Section::Bookmarks => match menu.bookmarks.selected_index() {
            Some(at) => {
                let open = match menu.bookmarks.all()[at].launch {
                    Launch::Browser => "open",
                    Launch::App | Launch::Game => "launch",
                };
                [Some(open), Some("remove"), None]
            }
            None => [None; 3],
        },
        Section::History if menu.clear_selected() => [Some(clear()), None, None],
        Section::History => match menu.selected_url() {
            Some(url) => [Some("open"), Some("remove"), Some(bookmark(&url))],
            None => [None; 3],
        },
        Section::Downloads if menu.clear_selected() => {
            let a = menu.downloads.has_finished().then(clear);
            [a, None, None]
        }
        Section::Downloads => match menu.downloads.selected_entry() {
            Some(at) => {
                let a = menu.downloads.saved_path(at).map(|_| "open");
                let x = match menu.downloads.items()[at].is_active() {
                    true => "cancel",
                    false => "remove",
                };
                [a, Some(x), None]
            }
            None => [None; 3],
        },
    }
}

/// What the lists draw besides their own state.
pub(super) struct ListsView<'a> {
    pub tabs: &'a [TabInfo],
    pub icons: &'a PageIcons,
    pub thumbs: &'a TabThumbs,
    pub tab_layout: TabLayout,
    pub face: FaceLabels,
    /// Whether a footer names what the buttons do.
    pub button_hints: bool,
}

/// Draw the menu overlay: the section bar over the active section's list, and
/// with `button_hints` a footer naming what the buttons do on the highlighted
/// row. Left/Right switch section, Up/Down move, A open, X delete, B close.
pub(super) fn add_menu(
    ctx: &egui::Context,
    menu: &mut Menu,
    view: &ListsView,
    commands: &mut Vec<AppCommand>,
) {
    let ListsView {
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
            menu.section(),
            Section::label,
            section_icon,
            |section| section_badge(menu, tabs, section),
        );
        if let Some(section) = clicked {
            commands.push(AppCommand::Menu(MenuAction::SetSection(section)));
        }
        ui.add_space(ROW_GAP * 2.0);
        let foot = match button_hints {
            true => panel::footer_room(ui, PANEL_ID),
            false => 0.0,
        };

        match (menu.section(), view.tab_layout) {
            (Section::Tabs, TabLayout::List) => {
                menu.set_tab_cols(1);
                add_tabs_section(ui, screen, foot, menu, tabs, icons, commands);
            }
            (Section::Tabs, TabLayout::Grid) => {
                add_tab_grid(ui, screen, foot, menu, view, commands)
            }
            (Section::Bookmarks, _) => {
                add_bookmarks_section(ui, screen, foot, menu, icons, commands)
            }
            (Section::History, _) => add_history_section(ui, screen, foot, menu, icons, commands),
            (Section::Downloads, _) => add_downloads_section(ui, screen, foot, menu, commands),
        }

        if button_hints {
            let (_, sideways) = keys::dpad_caps();
            let [a, x, y] = row_verbs(menu, tabs);
            let caps = [[face.a], [face.x], [face.y]];
            let mut hints: Vec<keys::Hint> = caps
                .iter()
                .zip([a, x, y])
                .filter_map(|(cap, verb)| Some((&cap[..], verb?)))
                .collect();
            // A grid takes Left/Right across its columns.
            let sections: &[&str] = match menu.section() == Section::Tabs && menu.tab_grid() {
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
        commands.push(AppCommand::Menu(MenuAction::Close));
    }
}

/// Tabs section: a leading "+ New tab" row (selection index 0) over the open tabs
/// (indices `1..=tabs.len()`), each with a bookmark toggle and a close button.
fn add_tabs_section(
    ui: &mut egui::Ui,
    screen: egui::Rect,
    foot: f32,
    menu: &Menu,
    tabs: &[TabInfo],
    icons: &PageIcons,
    commands: &mut Vec<AppCommand>,
) {
    let dim = palette().dim;
    let selected = menu.tab_selected();
    // Title width, less the two trailing buttons and the spacing before each.
    let row_w = screen.width() - SIDES - 2.0 * DEL_W - 12.0;
    section_scroll_over(ui, screen, foot).show(ui, |ui| {
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
            commands.push(AppCommand::Menu(MenuAction::NewTab));
        }

        for (i, tab) in tabs.iter().enumerate() {
            let sel = selected == i + 1; // index 0 is the "+ New tab" button
            let line = ui.horizontal(|ui| {
                let text = egui::RichText::new(&tab.title)
                    .size(ROW_FONT)
                    .color(palette().text);
                let mut atoms = egui::Atoms::new((text.atom_shrink(true), egui::Atom::grow()));
                if tab.active {
                    let check = egui::RichText::new(bold::CHECK)
                        .size(ROW_FONT)
                        .color(palette().accent);
                    atoms.push_right(check);
                }
                if let Some(icon) = tab_icon(icons, tab) {
                    atoms.push_left(icon);
                }
                let resp = row_atoms(ui, row_w, sel, atoms);
                if sel {
                    center_selected(&resp);
                }
                if resp.clicked() {
                    commands.push(AppCommand::Menu(MenuAction::OpenTab(i)));
                }
                // Close sits at the far right, so the toggle's slot stays when it can't act.
                let marked = menu.is_bookmarked(&tab.url);
                if !menu.can_toggle_bookmark(tab) {
                    let star = theme::icon_fill(fill::STAR).color(palette().accent);
                    let shown = egui::Label::new(star).selectable(false);
                    match marked {
                        true => ui.add_sized([DEL_W, ROW_H], shown),
                        false => {
                            ui.allocate_exact_size(egui::vec2(DEL_W, ROW_H), egui::Sense::hover())
                                .1
                        }
                    };
                } else if bookmark_button(ui, sel, marked, dim).clicked() {
                    commands.push(AppCommand::Menu(MenuAction::ToggleBookmark(
                        tab.url.clone(),
                    )));
                }
                if delete_button(ui, sel, dim).clicked() {
                    commands.push(AppCommand::Menu(MenuAction::CloseTab(i)));
                }
                resp
            });
            follow_pointer(&line.response, &line.inner, sel, i + 1, commands);
        }
    });
}

/// Narrowest a tab card gets before the grid drops a column.
const CARD_MIN_W: f32 = 180.0;
/// Space between cards, across and down.
const CARD_GAP: f32 = 12.0;
/// A card's content inset from its highlight.
const CARD_PAD: f32 = 4.0;
/// The close button over a card's picture.
const CARD_CLOSE: f32 = 24.0;
/// The glyph a card without a picture shows, for the start page or a new tab.
const CARD_GLYPH: f32 = 40.0;

/// A card's outer size and its picture's.
#[derive(Clone, Copy)]
struct CardSize {
    card: egui::Vec2,
    picture: egui::Vec2,
}

/// Tabs section as a grid of cards: a leading "New tab" card (selection index
/// 0) before the open tabs, each with a picture of its page and a close button.
fn add_tab_grid(
    ui: &mut egui::Ui,
    screen: egui::Rect,
    foot: f32,
    menu: &mut Menu,
    view: &ListsView,
    commands: &mut Vec<AppCommand>,
) {
    let full_w = screen.width() - SIDES;
    let cols = ((full_w + CARD_GAP) / (CARD_MIN_W + CARD_GAP))
        .floor()
        .max(1.0) as usize;
    menu.set_tab_cols(cols);
    let card_w = (full_w - (cols - 1) as f32 * CARD_GAP) / cols as f32;
    let picture_w = card_w - 2.0 * CARD_PAD;
    // The pictures' shape, else the screen's, which the page's is near.
    let aspect = view
        .tabs
        .iter()
        .find_map(|tab| tab.thumbnail.as_ref())
        .map_or(screen.height() / screen.width(), |thumb| {
            thumb.height as f32 / thumb.width as f32
        });
    let picture = egui::vec2(picture_w, (picture_w * aspect).round());
    let size = CardSize {
        card: egui::vec2(card_w, CARD_PAD + picture.y + ROW_H),
        picture,
    };
    let selected = menu.tab_selected();
    let count = view.tabs.len() + 1;
    section_scroll_over(ui, screen, foot).show(ui, |ui| {
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

/// One card of the tab grid: `tab`'s picture over its icon and title, or the
/// "New tab" card for `None`.
fn add_tab_card(
    ui: &mut egui::Ui,
    size: CardSize,
    tab: Option<(usize, &TabInfo)>,
    selected: bool,
    view: &ListsView,
    commands: &mut Vec<AppCommand>,
) {
    let (rect, resp) = ui.allocate_exact_size(size.card, egui::Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, ROW_RADIUS, ui.visuals().selection.bg_fill);
    }
    let picture =
        egui::Rect::from_min_size(rect.min + egui::vec2(CARD_PAD, CARD_PAD), size.picture);
    let title = egui::Rect::from_min_max(
        picture.left_bottom(),
        egui::pos2(picture.right(), rect.bottom()),
    );
    match tab {
        None => {
            paint_card_glyph(painter, picture, bold::PLUS, selected);
            paint_card_title(painter, title, None, "New tab", false);
        }
        Some((_, tab)) => {
            paint_tab_picture(ui, picture, tab, view, selected);
            let icon = card_icon(view.icons, tab);
            paint_card_title(ui.painter(), title, icon, &tab.title, tab.active);
        }
    }
    if selected {
        center_selected(&resp);
    }
    let index = tab.map_or(0, |(i, _)| i + 1);
    follow_pointer(&resp, &resp, selected, index, commands);
    if resp.clicked() {
        let action = match tab {
            Some((i, _)) => MenuAction::OpenTab(i),
            None => MenuAction::NewTab,
        };
        commands.push(AppCommand::Menu(action));
    }
    if let Some((i, _)) = tab {
        let at = picture.right_top() + egui::vec2(-CARD_PAD - CARD_CLOSE, CARD_PAD);
        let close = egui::Rect::from_min_size(at, egui::vec2(CARD_CLOSE, CARD_CLOSE));
        let id = egui::Id::new(("tab_card_close", i));
        if close_disc(ui, close, id, selected).clicked() {
            commands.push(AppCommand::Menu(MenuAction::CloseTab(i)));
        }
    }
}

/// A round close button over a card: a cross drawn from strokes, since an icon
/// glyph sits on its font's line rather than at the disc's centre.
fn close_disc(ui: &mut egui::Ui, rect: egui::Rect, id: egui::Id, selected: bool) -> egui::Response {
    let resp = ui.interact(rect, id, egui::Sense::click());
    let fill = match resp.hovered() {
        true => palette().surface,
        false => palette().panel_fill,
    };
    let color = if selected {
        palette().accent
    } else {
        palette().text
    };
    let painter = ui.painter();
    let center = rect.center();
    painter.circle_filled(center, rect.width() / 2.0, fill);
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

/// Paint `tab`'s picture into `rect`: its kept frame, else a stand-in for the
/// start page or its site.
fn paint_tab_picture(
    ui: &egui::Ui,
    rect: egui::Rect,
    tab: &TabInfo,
    view: &ListsView,
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
                .corner_radius(ROW_RADIUS)
                .paint_at(ui, rect);
            let stroke = match selected {
                true => egui::Stroke::new(2.0, palette().accent),
                false => egui::Stroke::new(1.0, palette().border),
            };
            ui.painter()
                .rect_stroke(rect, ROW_RADIUS, stroke, egui::StrokeKind::Inside);
        }
        None if tab.url == HOME_URL => {
            paint_card_glyph(ui.painter(), rect, bold::BROWSER, selected)
        }
        None => {
            let label = super::labels::brand_label(&tab.url);
            let icon = view.icons.icon(&tab.url);
            super::home::paint_site_card(ui.painter(), rect, &label, icon, selected);
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

/// A card with no picture: `glyph` centred on the surface.
fn paint_card_glyph(painter: &egui::Painter, rect: egui::Rect, glyph: &str, selected: bool) {
    painter.rect_filled(rect, ROW_RADIUS, palette().surface);
    let stroke = match selected {
        true => egui::Stroke::new(2.0, palette().accent),
        false => egui::Stroke::new(1.0, palette().border),
    };
    painter.rect_stroke(rect, ROW_RADIUS, stroke, egui::StrokeKind::Inside);
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
    icon: Option<CardIcon>,
    title: &str,
    shown: bool,
) {
    let font = egui::FontId::proportional(ROW_FONT);
    let gap = ROW_GAP * 2.0;
    let mut left = rect.left() + ROW_GAP;
    let side = egui::vec2(ROW_FONT, ROW_FONT);
    match icon {
        Some(CardIcon::Image(texture)) => {
            let at =
                egui::Rect::from_min_size(egui::pos2(left, rect.center().y - ROW_FONT / 2.0), side);
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(texture, at, uv, egui::Color32::WHITE);
            left += ROW_FONT + gap;
        }
        Some(CardIcon::Glyph(glyph, color)) => {
            let at = egui::pos2(left, rect.center().y);
            painter.text(at, egui::Align2::LEFT_CENTER, glyph, font.clone(), color);
            left += ROW_FONT + gap;
        }
        None => {}
    }
    let mut right = rect.right() - ROW_GAP;
    if shown {
        let at = egui::pos2(right, rect.center().y);
        let check = painter.text(
            at,
            egui::Align2::RIGHT_CENTER,
            bold::CHECK,
            font.clone(),
            palette().accent,
        );
        right = check.left() - gap;
    }
    let mut job = egui::text::LayoutJob::simple_singleline(title.to_string(), font, palette().text);
    job.wrap.max_width = (right - left).max(0.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = painter.layout_job(job);
    let at = egui::pos2(left, rect.center().y - galley.size().y / 2.0);
    painter.galley(at, galley, palette().text);
}

/// A card title's icon: the site's picture, or a glyph in its colour.
enum CardIcon {
    Image(egui::TextureId),
    Glyph(&'static str, egui::Color32),
}

/// `tab`'s icon on its card, as on its row: the site's, a globe without one,
/// the tab glyph for the start page; `None` with icons off.
fn card_icon(icons: &PageIcons, tab: &TabInfo) -> Option<CardIcon> {
    if !icons.enabled() {
        return None;
    }
    Some(match (tab.url == HOME_URL, icons.icon(&tab.url)) {
        (true, _) => CardIcon::Glyph(bold::BROWSER, palette().text),
        (false, Some(icon)) => CardIcon::Image(icon.row.id()),
        (false, None) => CardIcon::Glyph(bold::GLOBE, palette().dim),
    })
}

/// A tab row's leading icon: its site's, but the tab glyph for the start page,
/// which is no site; `None` with icons off.
fn tab_icon(icons: &PageIcons, tab: &TabInfo) -> Option<egui::Atom<'static>> {
    match tab.url == HOME_URL {
        true => icons.enabled().then(|| {
            egui::RichText::new(bold::BROWSER)
                .size(ROW_FONT)
                .color(palette().text)
                .into()
        }),
        false => site_icon(icons, &tab.url),
    }
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
    menu: &Menu,
    icons: &PageIcons,
    commands: &mut Vec<AppCommand>,
) {
    let dim = palette().dim;
    let bookmarks = &menu.bookmarks;
    if bookmarks.all().is_empty() {
        ui.label(
            egui::RichText::new("No bookmarks yet. Bookmark a page from Quick Access.").color(dim),
        );
        return;
    }

    // Fixed widths derived from the screen (not `ui.available_width()`, which is
    // unreliable inside a scroll area and made the list jump horizontally).
    let row_w = screen.width() - SIDES - DEL_W - 6.0; // frame margins + delete + spacing
    section_scroll_over(ui, screen, foot).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        for (i, bookmark) in bookmarks.all().iter().enumerate() {
            let selected = i == bookmarks.selected();
            let url = &bookmark.url;
            let line = ui.horizontal(|ui| {
                let icon = site_icon(icons, url);
                let tail = match bookmark.name.is_empty() {
                    true => url_tail(url),
                    false => url_without_scheme(url),
                };
                let label = super::labels::bookmark_label(bookmark);
                let atoms = url_atoms(ui, label, tail, icon, row_w);
                let resp = row_atoms(ui, row_w, selected, atoms);
                if selected {
                    center_selected(&resp);
                }
                if resp.clicked() {
                    commands.push(AppCommand::Menu(MenuAction::OpenBookmark(i)));
                }
                if delete_button(ui, selected, dim).clicked() {
                    commands.push(AppCommand::Menu(MenuAction::RemoveAt(i)));
                }
                resp
            });
            follow_pointer(&line.response, &line.inner, selected, i, commands);
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
    menu: &Menu,
    commands: &mut Vec<AppCommand>,
) {
    let dim = palette().dim;
    let downloads = &menu.downloads;
    if downloads.items().is_empty() {
        ui.label(egui::RichText::new("No downloads yet.").color(dim));
        return;
    }

    let status_w = 170.0; // fits "100% · 999.9 MB / 999.9 MB"-ish, truncated past that
    let row_w = screen.width() - SIDES - DEL_W - status_w - 12.0;
    section_scroll_over(ui, screen, foot).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        clear_row(
            ui,
            screen.width() - SIDES,
            "Clear finished",
            downloads.selected() == 0,
            menu.clear_armed(),
            downloads.has_finished(),
            commands,
        );
        for (i, item) in downloads.items().iter().enumerate() {
            let selected = downloads.selected() == i + 1; // index 0 is "Clear finished"
            let line = ui.horizontal(|ui| {
                let resp = row_button(
                    ui,
                    row_w,
                    selected,
                    egui::RichText::new(&item.filename).color(palette().text),
                );
                if selected {
                    center_selected(&resp);
                }
                if resp.clicked() && downloads.saved_path(i).is_some() {
                    commands.push(AppCommand::Menu(MenuAction::OpenDownload(i)));
                }
                ui.add_sized(
                    [status_w, ROW_H],
                    egui::Label::new(egui::RichText::new(item.status_text()).color(dim)).truncate(),
                );
                if delete_button(ui, selected, dim).clicked() {
                    commands.push(AppCommand::Menu(MenuAction::RemoveAt(i)));
                }
                resp
            });
            follow_pointer(&line.response, &line.inner, selected, i + 1, commands);
        }
    });
}

/// History section: visited URLs (most-recent first) with their visit date, over
/// a leading "Clear all" row.
fn add_history_section(
    ui: &mut egui::Ui,
    screen: egui::Rect,
    foot: f32,
    menu: &Menu,
    icons: &PageIcons,
    commands: &mut Vec<AppCommand>,
) {
    let dim = palette().dim;
    let hist = menu.history();
    if hist.entries().is_empty() {
        ui.label(egui::RichText::new("No history yet.").color(dim));
        return;
    }

    // `date_w` fits "YYYY-MM-DD HH:MM"; the URL gets what's left after the date,
    // the two trailing buttons, and their spacing.
    let date_w = 118.0;
    let row_w = screen.width() - SIDES - 2.0 * DEL_W - date_w - 18.0;
    section_scroll_over(ui, screen, foot).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        // Top row (cursor index 0, mirroring Tabs' "+ New tab"): drops every
        // entry.
        clear_row(
            ui,
            screen.width() - SIDES,
            "Clear all",
            hist.selected() == 0,
            menu.clear_armed(),
            true,
            commands,
        );
        for (i, entry) in hist.entries().iter().enumerate() {
            let selected = hist.selected() == i + 1; // index 0 is "Clear all"
            let line = ui.horizontal(|ui| {
                let icon = site_icon(icons, &entry.url);
                let brand = super::labels::brand_label(&entry.url);
                let atoms = url_atoms(ui, brand, url_tail(&entry.url), icon, row_w);
                let resp = row_atoms(ui, row_w, selected, atoms);
                if selected {
                    center_selected(&resp);
                }
                if resp.clicked() {
                    commands.push(AppCommand::Menu(MenuAction::OpenUrl(entry.url.clone())));
                }
                ui.add_sized(
                    [date_w, ROW_H],
                    egui::Label::new(
                        egui::RichText::new(history::format_time(entry.time)).color(dim),
                    )
                    .truncate(),
                );
                // Trailing actions: the bookmark toggle (same as Y), then delete.
                let marked = menu.is_bookmarked(&entry.url);
                if bookmark_button(ui, selected, marked, dim).clicked() {
                    commands.push(AppCommand::Menu(MenuAction::ToggleBookmark(
                        entry.url.clone(),
                    )));
                }
                if delete_button(ui, selected, dim).clicked() {
                    commands.push(AppCommand::Menu(MenuAction::RemoveAt(i)));
                }
                resp
            });
            follow_pointer(&line.response, &line.inner, selected, i + 1, commands);
        }
    });
}
