use crate::{
    app::AppCommand,
    browser::{AppBrowser, BrowserCommand, BrowserState},
    egui_glow_sdl2::EguiGlow,
    window::AppWindow,
};
use egui::{TopBottomPanel, Vec2};
use std::{sync::Arc, time::Duration};

pub struct AppUi {
    egui: EguiGlow,
    render_browser_fn: Arc<egui_glow::CallbackFn>,
    repaint_delay: Option<Duration>,
    toolbar_size: egui::Vec2,
    repaint_pending: bool,
}

impl AppUi {
    pub fn new(window: &AppWindow) -> Self {
        let render_to_parent_fn = window
            .offscreen_rendering_ctx
            .render_to_parent_callback()
            .unwrap();
        let render_browser_fn = egui_glow::CallbackFn::new(move |info, painter| {
            let viewport = info.viewport_in_pixels();
            let rect = servo::euclid::Rect::new(
                servo::euclid::Point2D::new(viewport.left_px, viewport.from_bottom_px),
                servo::euclid::Size2D::new(viewport.width_px, viewport.height_px),
            );
            // Servo draws into egui's GL context here
            render_to_parent_fn(painter.gl(), rect);
        });
        let egui = EguiGlow::new(window.get_sdl2_window(), window.get_glow_ctx(), None, false);

        Self {
            egui,
            render_browser_fn: Arc::new(render_browser_fn),
            repaint_delay: None,
            toolbar_size: egui::Vec2::default(),
            repaint_pending: false,
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
        let consumed = resp.consumed & self.is_pointer_over_toolbar(); // don't consume when pointer over browser area

        consumed
    }

    pub fn update(&mut self, browser: &mut AppBrowser, commands: &mut Vec<AppCommand>) {
        let mut state = browser.get_state_mut();

        let repaint_delay = self.egui.run(|ctx| {
            add_toolbar(ctx, &mut state, commands, &mut self.toolbar_size);

            egui::CentralPanel::default().show(ctx, |ui| {
                let min = ui.cursor().min;
                let size = ui.available_size();
                let rect = egui::Rect::from_min_size(min, size);
                ui.allocate_space(size);

                ui.painter().add(egui::PaintCallback {
                    rect,
                    callback: self.render_browser_fn.clone(),
                });
            });
        });
        self.repaint_delay.replace(repaint_delay);
    }

    /// Paints ui and presents to the window
    pub fn draw(&mut self, window: &AppWindow, force: bool) {
        if self.repaint_pending || force {
            window.prepare_for_rendering();
            self.egui.paint();
            window.present();
            self.repaint_pending = false;
        }
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
    ctx: &egui::Context,
    state: &mut std::cell::RefMut<'_, BrowserState>,
    commands: &mut Vec<AppCommand>,
    size: &mut egui::Vec2,
) {
    let frame = egui::Frame::default()
        .fill(ctx.style().visuals.window_fill)
        .inner_margin(4.0);
    TopBottomPanel::top("toolbar").frame(frame).show(ctx, |ui| {
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
