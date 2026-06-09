use crate::{
    app::{AppCommand, BookmarkAction},
    bookmarks::Bookmarks,
    browser::{AppBrowser, BrowserCommand, BrowserState},
    config::InterfaceConfig,
    osk::{Osk, OskCommand},
    window::AppWindow,
};
use egui_sdl2::egui::{self, Vec2};
use egui_sdl2::EguiGlow;
use std::time::{Duration, Instant};

/// Gamepad cursor overlay: circle radius and outline width (logical px).
const CURSOR_RADIUS: f32 = 5.0;
const CURSOR_STROKE: f32 = 1.5;
/// The cursor's full painted half-extent — how far it reaches from its center,
/// used to keep the whole glyph (not just the center) inside the web view.
const CURSOR_EXTENT: f32 = CURSOR_RADIUS + CURSOR_STROKE / 2.0;

pub struct AppUi {
    egui: EguiGlow,
    repaint_delay: Option<Duration>,
    /// Y of the web view's top edge (logical px) = the real toolbar bottom,
    /// measured from the central panel each frame. Used to map cursor↔browser
    /// coordinates and to keep the cursor out of the toolbar.
    webview_top: f32,
    repaint_pending: bool,
    /// egui handle to Servo's FBO color texture (rendered directly by WebRender).
    browser_tex_id: egui::TextureId,
    /// Last browser viewport size (physical px) we requested, to avoid churn.
    browser_viewport: (u32, u32),
    /// Gamepad cursor position (logical px). The UI owns it — it draws the
    /// overlay — and the gamepad moves it via [`AppUi::move_cursor`].
    cursor: (f32, f32),
    /// When the cursor last moved, or `None` if it has never moved. Drives the
    /// auto-hide: the overlay shows only within `cursor_linger` of this.
    cursor_last_move: Option<Instant>,
    /// How long the cursor stays visible after a move (from the interface config).
    cursor_linger: Duration,
    /// On-screen keyboard: state, rendering, and input routing all live here.
    osk: Osk,
    /// Saved bookmarks and the full-screen bookmarks overlay state.
    bookmarks: Bookmarks,
}

impl AppUi {
    pub fn new(window: &AppWindow, interface: &InterfaceConfig) -> Self {
        let mut egui =
            EguiGlow::new(window.get_sdl2_window(), window.get_glow_ctx(), None, false);
        // Register the FBO color texture once; its GL name is stable across
        // resizes, so this TextureId stays valid for the program's lifetime.
        let browser_tex_id = egui
            .painter
            .register_native_texture(window.rendering_color_texture());

        Self {
            egui,
            repaint_delay: None,
            webview_top: 0.0,
            repaint_pending: false,
            browser_tex_id,
            browser_viewport: (0, 0),
            cursor: {
                let (w, h) = window.size();
                (w as f32 / 2.0, h as f32 / 2.0)
            },
            cursor_last_move: None,
            cursor_linger: Duration::from_millis(interface.cursor_linger_ms),
            osk: Osk::new(),
            bookmarks: Bookmarks::load(),
        }
    }

    #[inline]
    pub fn take_repain_delay(&mut self) -> Option<Duration> {
        self.repaint_delay.take()
    }

    /// Move the gamepad cursor by a logical-px delta and mark it visible. Clamped
    /// to the window (inset by the cursor's painted extent so the whole circle
    /// stays on screen); it may roam over the toolbar so its buttons are clickable.
    #[inline]
    pub fn move_cursor(&mut self, dx: f32, dy: f32, window: &AppWindow) {
        let (w, h) = window.size();
        self.cursor.0 = (self.cursor.0 + dx).clamp(CURSOR_EXTENT, w as f32 - CURSOR_EXTENT);
        self.cursor.1 = (self.cursor.1 + dy).clamp(CURSOR_EXTENT, h as f32 - CURSOR_EXTENT);
        self.cursor_last_move = Some(Instant::now());
    }

    /// Whether the cursor is over the web view (below the toolbar). Clicks there
    /// go to the page; clicks above go to the egui toolbar via [`AppUi::click_ui`].
    #[inline]
    pub fn cursor_over_browser(&self) -> bool {
        self.cursor.1 >= self.webview_top
    }

    /// Click the egui UI element under the cursor by feeding the backend a
    /// synthetic mouse button event (egui never sees the gamepad otherwise).
    /// `pressed` mirrors the A button's press/release so egui registers a click.
    pub fn click_ui(&mut self, pressed: bool, window: &AppWindow) {
        let ppp = self.egui.ctx.pixels_per_point();
        let (x, y) = ((self.cursor.0 * ppp) as i32, (self.cursor.1 * ppp) as i32);
        let win = window.get_sdl2_window();
        let window_id = win.id();
        let event = if pressed {
            sdl2::event::Event::MouseButtonDown {
                timestamp: 0,
                window_id,
                which: 0,
                mouse_btn: sdl2::mouse::MouseButton::Left,
                clicks: 1,
                x,
                y,
            }
        } else {
            sdl2::event::Event::MouseButtonUp {
                timestamp: 0,
                window_id,
                which: 0,
                mouse_btn: sdl2::mouse::MouseButton::Left,
                clicks: 1,
                x,
                y,
            }
        };
        let _ = self.egui.state.on_event(win, &event);
        self.repaint_pending = true;
    }

    /// Time left before the cursor auto-hides, or `None` if it's already hidden
    /// (never moved or idle past `cursor_linger`).
    #[inline]
    fn cursor_visible_for(&self) -> Option<Duration> {
        self.cursor_last_move
            .and_then(|t| self.cursor_linger.checked_sub(t.elapsed()))
    }

    /// The gamepad cursor in browser-relative coordinates (below the toolbar),
    /// ready to feed to Servo as a mouse position.
    #[inline]
    pub fn cursor_browser_rel(&self) -> (f32, f32) {
        self.into_browser_rel_pos(self.cursor.0, self.cursor.1)
    }

    /// Whether the on-screen keyboard is currently shown.
    #[inline]
    pub fn osk_visible(&self) -> bool {
        self.osk.visible
    }

    /// Apply an [`OskCommand`] to the on-screen keyboard, routing typed input to
    /// the address bar when it holds focus, otherwise to the focused page element.
    pub fn osk(&mut self, cmd: OskCommand, browser: &AppBrowser, commands: &mut Vec<AppCommand>) {
        let to_address_bar = self.address_bar_focused();
        self.osk.handle(cmd, to_address_bar, browser, commands);
    }

    /// Whether the full-screen bookmarks overlay is shown.
    #[inline]
    pub fn bookmarks_visible(&self) -> bool {
        self.bookmarks.visible
    }

    /// Open the bookmarks overlay.
    #[inline]
    pub fn bookmarks_open(&mut self) {
        self.bookmarks.show();
    }

    #[inline]
    pub fn bookmarks_hide(&mut self) {
        self.bookmarks.hide();
    }

    /// Move the bookmarks selection by `dy` rows.
    #[inline]
    pub fn bookmarks_move(&mut self, dy: i32) {
        self.bookmarks.move_sel(dy);
    }

    /// The highlighted bookmark's URL, if any.
    #[inline]
    pub fn bookmarks_selected_url(&self) -> Option<String> {
        self.bookmarks.selected_url()
    }

    /// Remove the highlighted bookmark.
    #[inline]
    pub fn bookmarks_remove_selected(&mut self) {
        self.bookmarks.remove_selected();
    }

    /// Remove the bookmark at `index` (clicking its ✕ button).
    #[inline]
    pub fn bookmarks_remove_at(&mut self, index: usize) {
        self.bookmarks.remove(index);
    }

    /// Add or remove `url` from the saved bookmarks.
    #[inline]
    pub fn bookmark_toggle(&mut self, url: &str) {
        self.bookmarks.toggle(url);
    }

    /// Whether the address-bar text field currently holds keyboard focus.
    fn address_bar_focused(&self) -> bool {
        self.egui
            .ctx
            .memory(|m| m.has_focus(egui::Id::new("location")))
    }

    #[inline]
    pub fn into_browser_rel_pos(&self, x: f32, y: f32) -> (f32, f32) {
        (x, y - self.webview_top)
    }

    /// Handles the event and returns whether it is consumed
    pub fn handle_event(&mut self, window: &AppWindow, event: &sdl2::event::Event) -> bool {
        let resp = self.egui.state.on_event(window.get_sdl2_window(), event);
        self.repaint_pending = resp.repaint;
        // don't consume when pointer over browser area
        resp.consumed & self.is_pointer_over_toolbar()
    }

    pub fn update(&mut self, browser: &mut AppBrowser, commands: &mut Vec<AppCommand>) {
        let mut desired_px: Option<(u32, u32)> = None;

        // The cursor draws only while it lingers after a move. When it does, ask
        // the loop to wake when the linger ends so it gets erased even if no other
        // event arrives; otherwise leave the idle wait untouched.
        let cursor_visible = if self.osk.visible || self.bookmarks.visible {
            None
        } else {
            self.cursor_visible_for()
        };
        self.repaint_delay = cursor_visible;

        {
            let mut state = browser.get_state_mut();
            self.egui.run(|ctx| {
                let ppp = ctx.pixels_per_point();
                let mut root = egui::Ui::new(
                    ctx.clone(),
                    egui::Id::new("root_ui"),
                    egui::UiBuilder::new().max_rect(ctx.content_rect()),
                );
                root.set_clip_rect(ctx.content_rect());

                let bookmarked = self.bookmarks.contains(state.get_location());
                add_toolbar(&mut root, &mut state, commands, bookmarked);

                let frame = egui::Frame::default().inner_margin(0.0);
                egui::CentralPanel::default()
                    .frame(frame)
                    .show_inside(&mut root, |ui| {
                        let rect = ui.max_rect();
                        // The panel's top edge is the real toolbar bottom (incl.
                        // frame margins), so map cursor/clicks against it.
                        self.webview_top = rect.min.y;
                        ui.allocate_rect(rect, egui::Sense::hover());

                        desired_px = Some((
                            (rect.width() * ppp).round().max(1.0) as u32,
                            (rect.height() * ppp).round().max(1.0) as u32,
                        ));

                        // WebRender renders bottom-up into the FBO, so flip V.
                        let uv = egui::Rect::from_min_max(
                            egui::pos2(0.0, 1.0),
                            egui::pos2(1.0, 0.0),
                        );
                        ui.painter()
                            .image(self.browser_tex_id, rect, uv, egui::Color32::WHITE);
                    });

                if self.bookmarks.visible {
                    add_bookmarks(ctx, &self.bookmarks, commands);
                } else if self.osk.visible {
                    add_osk(ctx, self.osk.selected(), self.osk.shift(), self.osk.caps);
                } else if cursor_visible.is_some() {
                    // Gamepad cursor overlay, always on top. `cursor` is in logical
                    // px which equals egui points at the handheld's 1.0 scale factor.
                    let painter = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("gamepad_cursor"),
                    ));
                    let pos = egui::pos2(self.cursor.0, self.cursor.1);
                    painter.circle_filled(pos, CURSOR_RADIUS, egui::Color32::from_white_alpha(235));
                    painter.circle_stroke(
                        pos,
                        CURSOR_RADIUS,
                        egui::Stroke::new(CURSOR_STROKE, egui::Color32::BLACK),
                    );
                }
            });
        }

        if let Some(size) = desired_px {
            if size != self.browser_viewport {
                self.browser_viewport = size;
                browser.resize(size.0, size.1);
            }
        }
    }

    /// Paints the UI (toolbar + browser texture) and presents to the window.
    pub fn draw(&mut self, window: &AppWindow) {
        // Servo's software context made its own GL context current while rendering;
        // restore SDL2's context before egui issues any GL calls.
        window.make_current();
        window.bind_default_framebuffer();
        self.egui.paint();
        window.present();
        self.repaint_pending = false;
    }

    pub fn destroy(&mut self) {
        self.egui.destroy();
    }

    #[inline]
    fn is_pointer_over_toolbar(&self) -> bool {
        let Some(pos) = self.egui.state.get_pointer_pos_in_points() else {
            return false;
        };

        pos.y < self.webview_top
    }
}

/// Create a frameless button with square sizing, as used in the toolbar.
#[inline]
fn new_toolbar_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(text)
        .frame(false)
        .min_size(Vec2 { x: 20.0, y: 20.0 })
}

#[inline]
pub fn new_text_edit<'a>(text: &'a mut String, id: &str) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(text).id(egui::Id::new(id))
}

#[inline]
fn is_key_pressed(ui: &mut egui::Ui, response: egui::Response, key: egui::Key) -> bool {
    response.lost_focus() && ui.input(|i| i.key_pressed(key))
}

#[inline]
fn add_toolbar(
    ui: &mut egui::Ui,
    state: &mut std::cell::RefMut<'_, BrowserState>,
    commands: &mut Vec<AppCommand>,
    bookmarked: bool,
) {
    let frame = egui::Frame::default()
        .fill(ui.style().visuals.window_fill)
        .inner_margin(4.0);
    egui::Panel::top("toolbar").frame(frame).show_inside(ui, |ui| {
        ui.allocate_ui_with_layout(
            ui.available_size(),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                if ui.add(new_toolbar_button("⏴")).clicked() {
                    commands.push(AppCommand::Browser(BrowserCommand::Back));
                }
                if ui.add(new_toolbar_button("⏵")).clicked() {
                    commands.push(AppCommand::Browser(BrowserCommand::Foward));
                }

                if state.is_loading() {
                    ui.add(new_toolbar_button("X"));
                } else {
                    if ui.add(new_toolbar_button("↻")).clicked() {
                        commands.push(AppCommand::Browser(BrowserCommand::Reload));
                    }
                }

                ui.add_space(2.0);
                // The bookmark icons sit at the right edge; the address bar fills
                // the gap between them and the navigation buttons. ★ toggles the
                // current page (filled when saved); ☰ opens the bookmarks list.
                ui.allocate_ui_with_layout(
                    ui.available_size(),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if ui.add(new_toolbar_button("☰")).clicked() {
                            commands.push(AppCommand::Bookmark(BookmarkAction::Open));
                        }
                        if ui
                            .add(new_toolbar_button(if bookmarked { "★" } else { "☆" }))
                            .clicked()
                        {
                            commands.push(AppCommand::Bookmark(BookmarkAction::ToggleCurrent));
                        }
                        let location =
                            ui.add_sized(ui.available_size(), new_text_edit(state.get_location_mut(), "location"));
                        if is_key_pressed(ui, location, egui::Key::Enter) {
                            commands.push(AppCommand::Browser(BrowserCommand::Load));
                        }
                    },
                );
            },
        );
    });
}

/// Draw the on-screen keyboard, Steam-Deck style: a dark rounded overlay anchored
/// to the bottom, with the selected key (and active Shift/Caps) highlighted.
fn add_osk(ctx: &egui::Context, selected: (usize, usize), shift: bool, caps: bool) {
    use crate::osk::{key_label, Key, LAYOUT};

    let highlight = egui::Color32::from_rgb(0x2f, 0x81, 0xf7);
    let key_fill = egui::Color32::from_rgb(0x3a, 0x3a, 0x40);
    // Char keys are 36 wide with 4px gaps, so the 14-key top rows span 574px
    // (≈598 with the frame margin, inside the 640px window). Enter and Shift are
    // sized to make their (shorter) rows fill that same width.
    let key_width = |key: &Key| match key {
        Key::Space => 298.0,
        Key::Shift => 85.0,
        Key::Enter => 76.0,
        Key::Tab | Key::Caps | Key::Backspace | Key::Lang | Key::Hide => 54.0,
        _ => 36.0,
    };

    egui::Area::new(egui::Id::new("osk"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -10.0))
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(egui::Color32::from_rgba_unmultiplied(0x18, 0x18, 0x1c, 245))
                .corner_radius(12.0)
                .inner_margin(12.0)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 5.0);
                    for (r, row) in LAYOUT.iter().enumerate() {
                        ui.horizontal(|ui| {
                            for (c, key) in row.iter().enumerate() {
                                let is_sel = (r, c) == selected;
                                let active = is_sel
                                    || (*key == Key::Shift && shift)
                                    || (*key == Key::Caps && caps);
                                let size = egui::vec2(key_width(key), 38.0);
                                let fill = if active { highlight } else { key_fill };
                                let button = egui::Button::new(
                                    egui::RichText::new(key_label(*key, shift, caps))
                                        .color(egui::Color32::WHITE),
                                )
                                .fill(fill)
                                .corner_radius(6.0)
                                .min_size(size);
                                ui.add(button);
                            }
                        });
                    }
                });
        });
}

/// Draw the full-screen bookmarks overlay: a dark panel listing saved URLs with
/// the highlighted row selected, plus a one-line control hint. Navigated by the
/// gamepad (the router maps the stick/buttons to selection/open/delete/close).
fn add_bookmarks(
    ctx: &egui::Context,
    bookmarks: &Bookmarks,
    commands: &mut Vec<AppCommand>,
) {
    let screen = ctx.content_rect();
    let dim = egui::Color32::from_gray(0x99);
    // Fixed widths derived from the screen (not `ui.available_width()`, which is
    // unreliable inside a scroll area and made the list jump horizontally).
    let content_w = screen.width() - 32.0; // frame inner_margin (16) on each side
    let del_w = 26.0;
    let row_w = content_w - del_w - 6.0; // delete button + spacing
    egui::Area::new(egui::Id::new("bookmarks"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(egui::Color32::from_rgb(0x18, 0x18, 0x1c))
                .inner_margin(16.0)
                .show(ui, |ui| {
                    ui.set_min_size(screen.size());
                    // Header: title and a mouse-clickable Close.
                    ui.horizontal(|ui| {
                        ui.heading(egui::RichText::new("Bookmarks").color(egui::Color32::WHITE));
                        if ui
                            .button(egui::RichText::new("✖ Close").color(egui::Color32::WHITE))
                            .clicked()
                        {
                            commands.push(AppCommand::Bookmark(BookmarkAction::Close));
                        }
                    });
                    ui.label(
                        egui::RichText::new("A / click: open   X / ✖: delete   B / Close")
                            .color(dim),
                    );
                    ui.add_space(8.0);

                    if bookmarks.urls().is_empty() {
                        ui.label(
                            egui::RichText::new("No bookmarks yet — press ★ to add this page.")
                                .color(dim),
                        );
                        return;
                    }

                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for (i, url) in bookmarks.urls().iter().enumerate() {
                            let selected = i == bookmarks.selected();
                            // ✕ deletes, the row opens (mouse); the gamepad uses
                            // the stick + A/X instead.
                            ui.horizontal(|ui| {
                                if ui.add_sized([del_w, 26.0], egui::Button::new("✖")).clicked() {
                                    commands.push(AppCommand::Bookmark(BookmarkAction::Remove(i)));
                                }
                                let row = ui.add_sized(
                                    [row_w, 26.0],
                                    egui::Button::selectable(
                                        selected,
                                        egui::RichText::new(url).color(egui::Color32::WHITE),
                                    )
                                    .truncate(),
                                );
                                if row.clicked() {
                                    commands.push(AppCommand::Bookmark(BookmarkAction::OpenUrl(
                                        url.clone(),
                                    )));
                                }
                            });
                        }
                    });
                });
        });
}

