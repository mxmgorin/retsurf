use crate::{
    app::AppCommand,
    browser::{AppBrowser, BrowserCommand},
    egui_glow_sdl2::EguiGlow,
    window::AppWindow,
};
use egui::{TopBottomPanel, Vec2};
use std::{sync::Arc, time::Duration};

pub struct AppUi {
    egui: EguiGlow,
    callback_fn: Arc<egui_glow::CallbackFn>,
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
        let callback = egui_glow::CallbackFn::new(move |info, painter| {
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
            callback_fn: Arc::new(callback),
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
            let frame = egui::Frame::default()
                .fill(ctx.style().visuals.window_fill)
                .inner_margin(4.0);

            TopBottomPanel::top("toolbar").frame(frame).show(ctx, |ui| {
                ui.allocate_ui_with_layout(
                    ui.available_size(),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        if ui.add(toolbar_button("⏴")).clicked() {
                            commands.push(AppCommand::Browser(BrowserCommand::Back));
                        }
                        if ui.add(toolbar_button("⏵")).clicked() {
                            commands.push(AppCommand::Browser(BrowserCommand::Foward));
                        }

                        if state.is_loading() {
                            ui.add(toolbar_button("X"));
                        } else {
                            if ui.add(toolbar_button("↻")).clicked() {
                                commands.push(AppCommand::Browser(BrowserCommand::Reload));
                            }
                        }

                        ui.add_space(2.0);

                        let location_text = state.get_location_mut();

                        ui.allocate_ui_with_layout(
                            ui.available_size(),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                let location = ui.add_sized(
                                    ui.available_size(),
                                    text_edit(location_text, "location"),
                                );

                                if key_pressed(ui, location, egui::Key::Enter) {
                                    commands.push(AppCommand::Browser(BrowserCommand::Go(
                                        location_text.to_owned(),
                                    )));
                                }
                            },
                        );
                    },
                );

                self.toolbar_size = ui.min_size();
            });

            egui::CentralPanel::default().show(ctx, |ui| {
                let min = ui.cursor().min;
                let size = ui.available_size();
                let rect = egui::Rect::from_min_size(min, size);
                ui.allocate_space(size);

                ui.painter().add(egui::PaintCallback {
                    rect,
                    callback: self.callback_fn.clone(),
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

    fn is_pointer_over_toolbar(&self) -> bool {
        let Some(pos) = self.egui.state.get_pointer_pos_in_points() else {
            return false;
        };

        pos.y < self.toolbar_size.y
    }
}

/// Create a frameless button with square sizing, as used in the toolbar.
#[inline]
fn toolbar_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(text)
        .frame(false)
        .min_size(Vec2 { x: 20.0, y: 20.0 })
}

#[inline]
pub fn text_edit<'a>(text: &'a mut String, id: &str) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(text).id(egui::Id::new(id))
}

#[inline]
fn key_pressed(ui: &mut egui::Ui, response: egui::Response, key: egui::Key) -> bool {
    response.lost_focus() && ui.input(|i| i.key_pressed(key))
}
