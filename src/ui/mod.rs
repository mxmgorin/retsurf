//! The egui layer: [`AppUi`] owns the egui context, the gamepad cursor, and the
//! overlay state holders ([`crate::menu`], [`crate::osk`]), and composites
//! Servo's FBO texture under the chrome. The actual widgets are rendered by the
//! submodules: [`toolbar`], [`menu`] (the full-screen overlay), and [`osk`].

mod menu;
mod osk;
mod toolbar;

use crate::{
    app::AppCommand,
    browser::AppBrowser,
    config::{DownloadsConfig, HistoryConfig, InterfaceConfig},
    event::user::UserEventSender,
    menu::{Menu, Section},
    osk::{Osk, OskCommand},
    window::AppWindow,
};
use egui_sdl2::egui;
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
    /// The full-screen menu (Tabs / Bookmarks / History / Downloads) and its state.
    menu: Menu,
}

impl AppUi {
    pub fn new(
        window: &AppWindow,
        interface: &InterfaceConfig,
        history: &HistoryConfig,
        downloads: &DownloadsConfig,
    ) -> Self {
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
            menu: Menu::new(history, downloads),
        }
    }

    #[inline]
    pub fn take_repain_delay(&mut self) -> Option<Duration> {
        self.repaint_delay.take()
    }

    /// Ask the main loop to render one more frame immediately, without blocking on
    /// input. Commands are drained *after* this frame's [`AppUi::update`] builds the
    /// egui output, so a command that changes UI state (open a menu, switch section,
    /// type on the keyboard) wouldn't show until the next input wakes the loop. This
    /// schedules that follow-up frame so the change appears at once.
    #[inline]
    pub fn request_repaint(&mut self) {
        self.repaint_delay = Some(Duration::ZERO);
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

    /// Whether the full-screen menu is shown.
    #[inline]
    pub fn menu_visible(&self) -> bool {
        self.menu.visible
    }

    /// Open the menu.
    #[inline]
    pub fn menu_open(&mut self) {
        self.menu.open();
    }

    #[inline]
    pub fn menu_close(&mut self) {
        self.menu.close();
    }

    /// Switch the active section by `delta` (◀▶).
    #[inline]
    pub fn menu_switch(&mut self, delta: i32) {
        self.menu.switch_section(delta);
    }

    /// Jump to a specific section (clicking its tab).
    #[inline]
    pub fn menu_set_section(&mut self, section: Section) {
        self.menu.set_section(section);
    }

    /// The active menu section.
    #[inline]
    pub fn menu_section(&self) -> Section {
        self.menu.section()
    }

    /// Highlighted row in the Tabs section (== tab count means the "+ New tab" row).
    #[inline]
    pub fn menu_tab_selected(&self) -> usize {
        self.menu.tab_selected()
    }

    /// Refresh the Tabs section's known tab count (keeps its selection in range).
    #[inline]
    pub fn menu_set_tab_count(&mut self, count: usize) {
        self.menu.set_tab_count(count);
    }

    /// Move the active section's selection by `dy` rows.
    #[inline]
    pub fn menu_move(&mut self, dy: i32) {
        self.menu.move_sel(dy);
    }

    /// The highlighted entry's URL in the active section, if any.
    #[inline]
    pub fn menu_selected_url(&self) -> Option<String> {
        self.menu.selected_url()
    }

    /// Remove the highlighted entry in the active section.
    #[inline]
    pub fn menu_remove_selected(&mut self) {
        self.menu.remove_selected();
    }

    /// Remove the entry at `index` in the active section (clicking its ✖ button).
    #[inline]
    pub fn menu_remove_at(&mut self, index: usize) {
        self.menu.remove_at(index);
    }

    /// Clear all entries in the active section (History's "Clear all").
    #[inline]
    pub fn menu_clear(&mut self) {
        self.menu.clear();
    }

    /// Record a visited URL in history (no-op if history is disabled).
    #[inline]
    pub fn menu_record_history(&mut self, url: &str) {
        self.menu.record_history(url);
    }

    /// Pull progress from download worker threads into the menu's Downloads list
    /// (records finishes; cheap when nothing is downloading).
    #[inline]
    pub fn downloads_poll(&mut self) {
        self.menu.downloads.poll();
    }

    /// Start downloading `url` in the background (see [`crate::data::downloads`]).
    #[inline]
    pub fn start_download(&mut self, url: &str, sender: &UserEventSender) {
        self.menu.downloads.start(url, sender);
    }

    /// Add or remove `url` from the saved bookmarks (★ button / Start).
    #[inline]
    pub fn toggle_bookmark(&mut self, url: &str) {
        self.menu.toggle_bookmark(url);
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

    /// Resize the browser to the central web-view area (the window minus the
    /// toolbar) for the current window size. Driven by SDL window-resize events:
    /// egui's reactive sizing reads the central rect a frame later, so it can lag
    /// behind the actual window. Uses the toolbar height measured during `update`,
    /// and shares `browser_viewport` with that path so the two never double-resize.
    pub fn resize_browser(&mut self, window: &AppWindow, browser: &AppBrowser) {
        let (dw, dh) = window.drawable_size();
        if dw == 0 || dh == 0 {
            return;
        }
        let toolbar_px = (self.webview_top * self.egui.ctx.pixels_per_point()).round() as u32;
        let size = (dw, dh.saturating_sub(toolbar_px).max(1));
        if size != self.browser_viewport {
            self.browser_viewport = size;
            browser.resize(size.0, size.1);
        }
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
        let cursor_visible = if self.osk.visible || self.menu.visible {
            None
        } else {
            self.cursor_visible_for()
        };
        self.repaint_delay = cursor_visible;

        // Read tab info *before* borrowing the active tab's state below — both read
        // the browser's tab list, so they can't overlap. `tab_pos` is the 1-based
        // active index and count, shown in the toolbar; `tab_infos` feeds the menu.
        let tab_pos = (browser.active_tab() + 1, browser.tab_count());
        let tab_infos = if self.menu.visible {
            self.menu.set_tab_count(browser.tab_count());
            browser.tabs()
        } else {
            Vec::new()
        };

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

                let bookmarked = self.menu.is_bookmarked(state.get_location());
                let active_downloads = self.menu.downloads.active_count();
                toolbar::add_toolbar(
                    &mut root,
                    &mut state,
                    commands,
                    bookmarked,
                    tab_pos,
                    active_downloads,
                );

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

                if self.menu.visible {
                    menu::add_menu(ctx, &self.menu, &tab_infos, commands);
                } else if self.osk.visible {
                    osk::add_osk(ctx, self.osk.selected(), self.osk.shift(), self.osk.caps);
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
