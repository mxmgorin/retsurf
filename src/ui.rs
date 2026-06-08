use crate::{
    app::AppCommand,
    browser::{AppBrowser, BrowserCommand, BrowserState},
    config::InterfaceConfig,
    osk::Osk,
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
        }
    }

    #[inline]
    pub fn take_repain_delay(&mut self) -> Option<Duration> {
        self.repaint_delay.take()
    }

    /// Move the gamepad cursor by a logical-px delta and mark it visible. Clamped
    /// to the web view: X spans the window, Y stays below the toolbar (its buttons
    /// aren't clickable with the cursor anyway, so there's no reason to go there).
    #[inline]
    pub fn move_cursor(&mut self, dx: f32, dy: f32, window: &AppWindow) {
        let (w, h) = window.size();
        // Keep the whole circle inside the web view: inset every edge by the
        // cursor's painted extent, and the top by the toolbar height too.
        self.cursor.0 = (self.cursor.0 + dx).clamp(CURSOR_EXTENT, w as f32 - CURSOR_EXTENT);
        self.cursor.1 = (self.cursor.1 + dy)
            .clamp(self.webview_top + CURSOR_EXTENT, h as f32 - CURSOR_EXTENT);
        self.cursor_last_move = Some(Instant::now());
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

    /// Hide the on-screen keyboard.
    #[inline]
    pub fn osk_hide(&mut self) {
        self.osk.hide();
    }

    /// Move the on-screen keyboard selection by one cell.
    #[inline]
    pub fn osk_move(&mut self, dx: i32, dy: i32) {
        self.osk.move_sel(dx, dy);
    }

    /// Apply the selected on-screen-keyboard key, routing input to the address bar
    /// if it holds focus, otherwise to the focused page element.
    pub fn osk_activate(&mut self, browser: &AppBrowser, commands: &mut Vec<AppCommand>) {
        let to_address_bar = self.address_bar_focused();
        self.osk.activate(to_address_bar, browser, commands);
    }

    /// Backspace from the on-screen keyboard (the **X** button while open).
    pub fn osk_backspace(&mut self, browser: &AppBrowser) {
        let to_address_bar = self.address_bar_focused();
        self.osk.backspace(to_address_bar, browser);
    }

    /// Type a space from the on-screen keyboard (the **Y** button).
    pub fn osk_space(&mut self, browser: &AppBrowser) {
        let to_address_bar = self.address_bar_focused();
        self.osk.type_space(to_address_bar, browser);
    }

    /// Toggle Shift on the on-screen keyboard (the **L2** trigger).
    #[inline]
    pub fn osk_shift(&mut self) {
        self.osk.toggle_shift();
    }

    /// Submit from the on-screen keyboard (the **R2** trigger).
    pub fn osk_enter(&mut self, browser: &AppBrowser, commands: &mut Vec<AppCommand>) {
        let to_address_bar = self.address_bar_focused();
        self.osk.enter(to_address_bar, browser, commands);
    }

    /// Show the on-screen keyboard (the **X** button while closed).
    #[inline]
    pub fn osk_show(&mut self) {
        self.osk.visible = true;
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
        let cursor_visible = if self.osk.visible {
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

                add_toolbar(&mut root, &mut state, commands);

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

                if self.osk.visible {
                    add_osk(ctx, self.osk.selected(), self.osk.shift);
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
                add_location_text(ui, state.get_location_mut(), commands);
            },
        );
    });
}

/// Draw the on-screen keyboard, Steam-Deck style: a dark rounded overlay anchored
/// to the bottom, with the selected key (and active Shift) highlighted.
fn add_osk(ctx: &egui::Context, selected: (usize, usize), shift: bool) {
    use crate::osk::{key_label, Key, LAYOUT};

    let highlight = egui::Color32::from_rgb(0x2f, 0x81, 0xf7);
    let key_fill = egui::Color32::from_rgb(0x3a, 0x3a, 0x40);
    let key_width = |key: &Key| match key {
        Key::Space => 150.0,
        Key::Shift | Key::Backspace | Key::Go => 62.0,
        _ => 40.0,
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
                    ui.spacing_mut().item_spacing = egui::vec2(5.0, 5.0);
                    for (r, row) in LAYOUT.iter().enumerate() {
                        ui.horizontal(|ui| {
                            for (c, key) in row.iter().enumerate() {
                                let is_sel = (r, c) == selected;
                                let active = is_sel || (*key == Key::Shift && shift);
                                let size = egui::vec2(key_width(key), 38.0);
                                let fill = if active { highlight } else { key_fill };
                                let button = egui::Button::new(
                                    egui::RichText::new(key_label(*key, shift))
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

#[inline]
fn add_location_text(ui: &mut egui::Ui, text: &mut String, commands: &mut Vec<AppCommand>) {
    ui.allocate_ui_with_layout(
        ui.available_size(),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            let location = ui.add_sized(ui.available_size(), new_text_edit(text, "location"));

            if is_key_pressed(ui, location, egui::Key::Enter) {
                commands.push(AppCommand::Browser(BrowserCommand::Load));
            }
        },
    );
}
