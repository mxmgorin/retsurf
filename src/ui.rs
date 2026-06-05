use crate::{
    app::AppCommand,
    browser::{AppBrowser, BrowserCommand, BrowserState},
    window::AppWindow,
};
use egui_sdl2::egui::{self, Vec2};
use egui_sdl2::EguiGlow;
use std::time::Duration;

pub struct AppUi {
    egui: EguiGlow,
    repaint_delay: Option<Duration>,
    toolbar_size: egui::Vec2,
    repaint_pending: bool,
    /// egui handle to Servo's FBO color texture (rendered directly by WebRender).
    browser_tex_id: egui::TextureId,
    /// Last browser viewport size (physical px) we requested, to avoid churn.
    browser_viewport: (u32, u32),
}

impl AppUi {
    pub fn new(window: &AppWindow) -> Self {
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
            toolbar_size: egui::Vec2::default(),
            repaint_pending: false,
            browser_tex_id,
            browser_viewport: (0, 0),
        }
    }

    #[inline]
    pub fn take_repain_delay(&mut self) -> Option<Duration> {
        self.repaint_delay.take()
    }

    #[inline]
    pub fn into_browser_rel_pos(&self, x: f32, y: f32) -> (f32, f32) {
        (x, y - self.toolbar_size.y)
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

                add_toolbar(&mut root, &mut state, commands, &mut self.toolbar_size);

                let frame = egui::Frame::default().inner_margin(0.0);
                egui::CentralPanel::default()
                    .frame(frame)
                    .show_inside(&mut root, |ui| {
                        let rect = ui.max_rect();
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

        pos.y < self.toolbar_size.y
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
    size: &mut egui::Vec2,
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

        *size = ui.min_size();
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
