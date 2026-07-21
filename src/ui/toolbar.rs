//! The toolbar (top or bottom, per the display config): navigation buttons, the
//! address bar, bookmark toggle, and the chips that jump into menu sections (tab
//! position, active downloads).

use crate::app::{AppCommand, MenuAction, SettingsAction};
use crate::browser::{BrowserCommand, BrowserState};
use crate::config::ToolbarPosition;
use crate::overlay::menu::Section;
use crate::overlay::settings::SettingsSection;
use egui_sdl2::egui::{self, Vec2};

/// Create a frameless button with square sizing, as used in the toolbar.
#[inline]
fn new_toolbar_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(text)
        .frame(false)
        .min_size(Vec2 { x: 20.0, y: 20.0 })
}

#[inline]
fn new_text_edit<'a>(text: &'a mut String, id: &str) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(text).id(egui::Id::new(id))
}

/// A frameless toolbar button painting a house *silhouette* (egui's fonts have
/// no monochrome house glyph). Solid shapes — a filled roof triangle and body
/// with the door cut back out in the toolbar's background — read crisply at icon
/// size, where thin outlines look broken. Brightens on hover; returns its click
/// response.
fn add_home_button(ui: &mut egui::Ui) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2 { x: 22.0, y: 20.0 }, egui::Sense::click());
    let color = ui.style().interact(&resp).fg_stroke.color;
    let bg = ui.style().visuals.window_fill;
    let painter = ui.painter();

    // Snap the center onto a half-pixel: every offset below is a half (±5.5
    // body, ±1.5 door), so a half-pixel center lands all those edges on whole
    // pixels — crisp and symmetric. (Rounding the center to a whole pixel did
    // the opposite, feathering the door's narrow cut and reading as off-center.)
    let c = rect.center().floor() + egui::vec2(0.5, 0.5);
    let half = 5.5; // house half-width / half-height
    let (left, right) = (c.x - half, c.x + half);
    let (top, bottom) = (c.y - half, c.y + half);
    let eaves = c.y - half * 0.30; // where the roof meets the walls

    // The whole house as one filled polygon (apex → eaves → base), so the roof
    // and body share no seam between them.
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(c.x, top),
            egui::pos2(right, eaves),
            egui::pos2(right, bottom),
            egui::pos2(left, bottom),
            egui::pos2(left, eaves),
        ],
        color,
        egui::Stroke::NONE,
    ));
    // Door: cut back out in the background color.
    let dw = 3.0;
    painter.rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(c.x - dw / 2.0, bottom - 4.0),
            egui::pos2(c.x + dw / 2.0, bottom),
        ),
        0.0,
        bg,
    );
    resp
}

/// A frameless toolbar button painting a rounded square outline with the tab
/// count centered inside — like a phone browser's tab counter. Drawn (rather
/// than a bracketed label) so the square reads as an icon, not a selection.
/// Brightens on hover; returns its click response.
fn add_tabs_button(ui: &mut egui::Ui, count: usize) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2 { x: 22.0, y: 20.0 }, egui::Sense::click());
    let color = ui.style().interact(&resp).fg_stroke.color;
    let painter = ui.painter();

    // Snap the center onto a half-pixel so the 1px stroke lands on whole pixels.
    let c = rect.center().floor() + egui::vec2(0.5, 0.5);
    let half = 6.5; // square half-size
    let square = egui::Rect::from_center_size(c, Vec2::splat(half * 2.0));
    painter.rect_stroke(
        square,
        2.0,
        egui::Stroke::new(1.5, color),
        egui::StrokeKind::Inside,
    );

    // Counts past 99 won't fit — cap the label rather than overflow the square.
    let label = if count > 99 {
        "99".to_string()
    } else {
        count.to_string()
    };
    painter.text(
        c,
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(if count > 9 { 8.0 } else { 10.0 }),
        color,
    );
    resp
}

#[inline]
fn is_key_pressed(ui: &mut egui::Ui, response: egui::Response, key: egui::Key) -> bool {
    response.lost_focus() && ui.input(|i| i.key_pressed(key))
}

/// The toolbar row — nav buttons, address-bar field, and the menu/tab/download
/// chips — laid out left-to-right. Shared by the space-reserving panel
/// ([`add_toolbar`]) and the auto-hide overlay ([`add_toolbar_overlay`]).
#[allow(clippy::too_many_arguments)]
fn toolbar_contents(
    ui: &mut egui::Ui,
    state: &mut std::cell::RefMut<'_, BrowserState>,
    commands: &mut Vec<AppCommand>,
    bookmarked: bool,
    // 1-based active tab index and total tab count, e.g. `(2, 3)` → "2/3".
    tab_pos: (usize, usize),
    // Downloads still in flight; shown as a `⬇N` chip that jumps to the section.
    active_downloads: usize,
    // A newer build was found; shown as an "Update" chip that opens Settings->About.
    update_available: bool,
    // Active tab's page zoom percent when off the config default (chip hidden at it).
    zoom_pct: Option<u16>,
    // When the OSK types into the address bar, its caret position — park egui's
    // caret here so it tracks the external edit (it won't follow on its own).
    osk_caret: Option<usize>,
) {
    // Height 0 so the row sizes to its content — the panel measures it,
    // and the overlay's `Area` has no bounded height to center within.
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_size().x, 0.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            if ui.add(new_toolbar_button("←")).clicked() {
                commands.push(AppCommand::Browser(BrowserCommand::Back));
            }
            if ui.add(new_toolbar_button("→")).clicked() {
                commands.push(AppCommand::Browser(BrowserCommand::Forward));
            }

            // Reload, disabled (greyed, non-interactive) while loading —
            // servo's WebView exposes no stop()/cancel, so there's nothing
            // to click mid-load. While loading it also swaps to a ✖ (muted by
            // the disabled state) to read as "can't reload yet" rather than a
            // live reload affordance. Always the SAME Button widget, only its
            // label changes: toggling enabledness/text keeps egui's widget id
            // stable for this slot, where swapping to a different widget kind
            // churned the id between passes and tripped the red id-clash
            // outline. Static on purpose — an animated spinner would force
            // continuous repaints, which we avoid on handheld hardware.
            // The glyph renders a touch below the 13.0 default so it reads
            // lighter than the arrows; the button keeps the 20×20 footprint so
            // the slot (and its widget id) is unchanged.
            let loading = state.is_loading();
            let glyph = if loading { "✖" } else { "↻" };
            let reload = egui::Button::new(egui::RichText::new(glyph).size(11.0))
                .frame(false)
                .min_size(Vec2 { x: 20.0, y: 20.0 });
            if ui.add_enabled(!loading, reload).clicked() {
                commands.push(AppCommand::Browser(BrowserCommand::Reload));
            }

            // Navigate the active tab to the built-in start page.
            if add_home_button(ui).clicked() {
                commands.push(AppCommand::Menu(MenuAction::OpenUrl(
                    crate::browser::HOME_URL.to_string(),
                )));
            }

            ui.add_space(2.0);
            // The bookmark icons sit at the right edge; the address bar fills
            // the gap between them and the navigation buttons. ★ toggles the
            // current page (filled when saved); ☰ opens the menu.
            ui.allocate_ui_with_layout(
                ui.available_size(),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    if ui.add(new_toolbar_button("☰")).clicked() {
                        commands.push(AppCommand::Menu(MenuAction::Open));
                    }
                    // ⚙ U+2699 (in egui's emoji-icon-font, like ☰) opens
                    // the settings overlay.
                    if ui.add(new_toolbar_button("⚙")).clicked() {
                        commands.push(AppCommand::Settings(SettingsAction::Open));
                    }
                    // Update chip — shown only once a check (auto or manual) has
                    // found a newer build. Accent-coloured to draw the eye; opens
                    // Settings straight to the About tab, where the notes, link, and
                    // Install/Download action live. Plain text (no glyph) so it can't
                    // tofu on egui's bundled fonts.
                    if update_available {
                        let label = egui::RichText::new("Update")
                            .color(super::theme::ACCENT)
                            .strong();
                        let chip = egui::Button::new(label)
                            .frame(false)
                            .min_size(Vec2 { x: 20.0, y: 20.0 });
                        if ui.add(chip).clicked() {
                            commands.push(AppCommand::Settings(SettingsAction::Open));
                            commands.push(AppCommand::Settings(SettingsAction::SetSection(
                                SettingsSection::About,
                            )));
                        }
                    }
                    // ⬇ U+2B07 (not ↓ U+2193): egui's default fonts lack the
                    // plain arrow, only the emoji one renders.
                    if active_downloads > 0 {
                        let label = format!("⬇{active_downloads}");
                        if ui.add(new_toolbar_button(&label)).clicked() {
                            commands.push(AppCommand::Menu(MenuAction::Open));
                            commands
                                .push(AppCommand::Menu(MenuAction::SetSection(Section::Downloads)));
                        }
                    }
                    // Tab counter: a square icon with the total tab count
                    // inside, beside the menu button. Always shown (even at
                    // "1"); clicking it opens the menu's Tabs section (like
                    // the ⬇ chip for downloads).
                    if add_tabs_button(ui, tab_pos.1).clicked() {
                        commands.push(AppCommand::Menu(MenuAction::Open));
                        commands.push(AppCommand::Menu(MenuAction::SetSection(Section::Tabs)));
                    }
                    // Page-zoom chip (e.g. "125%"), shown only while the
                    // active tab is off the config default; clicking resets.
                    if let Some(pct) = zoom_pct {
                        let label = format!("{pct}%");
                        if ui.add(new_toolbar_button(&label)).clicked() {
                            commands.push(AppCommand::Browser(BrowserCommand::Zoom(0)));
                        }
                    }
                    if ui
                        .add(new_toolbar_button(if bookmarked { "★" } else { "☆" }))
                        .clicked()
                    {
                        commands.push(AppCommand::ToggleBookmark);
                    }
                    // The address bar fills the remaining width. We draw our
                    // own field frame (styled like egui's TextEdit) holding a
                    // frameless text edit plus the reader-mode 📖 toggle at its
                    // right edge — Firefox/Safari style. The two sit in
                    // *disjoint* rects (no overlap), so the icon click is
                    // reliable; an icon overlaid on the text edit raced it for
                    // the hit-test and clicked unreliably.
                    let avail = ui.available_size();
                    let radius = ui.visuals().widgets.inactive.corner_radius;
                    let field = egui::Frame::new()
                        .fill(ui.visuals().text_edit_bg_color())
                        .stroke(ui.visuals().widgets.inactive.bg_stroke)
                        .corner_radius(radius)
                        .inner_margin(egui::Margin::symmetric(4, 2))
                        .show(ui, |ui| {
                            // Fill the toolbar's remaining width (minus the
                            // frame's own margins) so the bar spans the gap;
                            // height stays natural (one text row).
                            ui.set_min_width(avail.x - 8.0);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                // 📖 "open book" reader toggle — its own slot
                                // at the field's right edge. Glyph lives in
                                // egui's NotoEmoji + emoji-icon-font
                                // (cmap-checked; most reader-ish glyphs tofu).
                                if ui.add(new_toolbar_button("📖")).clicked() {
                                    commands.push(AppCommand::Browser(BrowserCommand::Reader));
                                }
                                if let Some(pos) = osk_caret {
                                    super::park_caret(
                                        ui.ctx(),
                                        egui::Id::new("location"),
                                        pos,
                                        state.get_location().chars().count(),
                                    );
                                }
                                let location = ui.add_sized(
                                    ui.available_size(),
                                    new_text_edit(state.get_location_mut(), "location")
                                        .frame(egui::Frame::new()),
                                );
                                if is_key_pressed(ui, location.clone(), egui::Key::Enter) {
                                    commands.push(AppCommand::Browser(BrowserCommand::Load));
                                }
                                location.has_focus()
                            })
                            .inner
                        });
                    // Repaint the frame's ring in the accent stroke while the
                    // address bar is focused, matching egui's own TextEdit.
                    if field.inner {
                        ui.painter().rect_stroke(
                            field.response.rect,
                            radius,
                            ui.visuals().selection.stroke,
                            egui::StrokeKind::Inside,
                        );
                    }
                },
            );
        },
    );
}

/// Draw the toolbar as a space-reserving panel anchored to `position`'s edge
/// (the central panel takes whatever's left). Returns the panel's screen rect.
#[allow(clippy::too_many_arguments)]
pub(super) fn add_toolbar(
    ui: &mut egui::Ui,
    state: &mut std::cell::RefMut<'_, BrowserState>,
    commands: &mut Vec<AppCommand>,
    bookmarked: bool,
    tab_pos: (usize, usize),
    active_downloads: usize,
    update_available: bool,
    zoom_pct: Option<u16>,
    osk_caret: Option<usize>,
    position: ToolbarPosition,
) -> egui::Rect {
    let frame = egui::Frame::default()
        .fill(ui.style().visuals.window_fill)
        .inner_margin(4.0);
    let panel = match position {
        ToolbarPosition::Top => egui::Panel::top("toolbar"),
        ToolbarPosition::Bottom => egui::Panel::bottom("toolbar"),
    };
    panel
        .frame(frame)
        .show_inside(ui, |ui| {
            toolbar_contents(
                ui,
                state,
                commands,
                bookmarked,
                tab_pos,
                active_downloads,
                update_available,
                zoom_pct,
                osk_caret,
            )
        })
        .response
        .rect
}

/// Draw the toolbar as a floating overlay pinned to `position`'s edge — for
/// auto-hide, where the web view stays full-height (no reflow) and the bar is
/// drawn only while shown (the caller skips this call to hide it). `width`
/// spans the window. Returns the bar's screen rect.
#[allow(clippy::too_many_arguments)]
pub(super) fn add_toolbar_overlay(
    ctx: &egui::Context,
    width: f32,
    state: &mut std::cell::RefMut<'_, BrowserState>,
    commands: &mut Vec<AppCommand>,
    bookmarked: bool,
    tab_pos: (usize, usize),
    active_downloads: usize,
    update_available: bool,
    zoom_pct: Option<u16>,
    osk_caret: Option<usize>,
    position: ToolbarPosition,
) -> egui::Rect {
    let frame = egui::Frame::default()
        .fill(ctx.global_style().visuals.window_fill)
        .inner_margin(4.0);
    let align = match position {
        ToolbarPosition::Top => egui::Align2::CENTER_TOP,
        ToolbarPosition::Bottom => egui::Align2::CENTER_BOTTOM,
    };
    egui::Area::new(egui::Id::new("toolbar_overlay"))
        .order(egui::Order::Foreground)
        .anchor(align, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.set_width(width);
            frame
                .show(ui, |ui| {
                    toolbar_contents(
                        ui,
                        state,
                        commands,
                        bookmarked,
                        tab_pos,
                        active_downloads,
                        update_available,
                        zoom_pct,
                        osk_caret,
                    )
                })
                .response
                .rect
        })
        .inner
}
