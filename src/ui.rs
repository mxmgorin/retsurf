use crate::{
    browser::AppBrowser, egui_glue::EguiGlue, egui_sdl2::EventResponse, window::AppWindow,
};
use std::{sync::Arc, time::Duration};

pub struct AppUi {
    egui: EguiGlue,
    callback_fn: Arc<egui_glow::CallbackFn>,
    repaint_delay: Option<Duration>,
    top_bar_size: egui::Vec2,
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
        let egui = EguiGlue::new(window.get_gl_ctx(), None);

        Self {
            egui,
            callback_fn: Arc::new(callback),
            repaint_delay: None,
            top_bar_size: egui::Vec2::default(),
        }
    }

    pub fn take_repain_delay(&mut self) -> Option<Duration> {
        self.repaint_delay.take()
    }

    pub fn get_top_bar_height(&self) -> f32 {
        self.top_bar_size.y
    }

    pub fn handle_event(
        &mut self,
        window: &AppWindow,
        event: &sdl2::event::Event,
    ) -> EventResponse {
        self.egui.on_event(window.get_sdl2_window(), event)
    }

    pub fn update(&mut self, window: &AppWindow, browser: &AppBrowser) {
        let repaint_delay = self.egui.run(window.get_sdl2_window(), |ctx| {
            if let Some(url) = browser.get_url() {
                let frame = egui::Frame::default()
                    .fill(ctx.style().visuals.window_fill)
                    .inner_margin(4.0);
                egui::TopBottomPanel::top("browser_url")
                    .frame(frame)
                    .show(ctx, |ui| {
                        ui.label(url.to_string());
                        self.top_bar_size = ui.min_size();
                    });
            }

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

    pub fn paint(&mut self, size: [u32; 2]) {
        self.egui.paint(size);
    }

    pub fn destroy(&mut self) {
        self.egui.destroy();
    }
}
