use crate::{browser::AppBrowser, egui_glue::EguiGlue};
use servo::{OffscreenRenderingContext, RenderingContext};
use std::{rc::Rc, time::Duration};

pub struct AppUi {
    egui: EguiGlue,
    rendering_ctx: Rc<OffscreenRenderingContext>,
}

impl AppUi {
    pub fn new(rendering_ctx: Rc<OffscreenRenderingContext>) -> Self {
        let egui = EguiGlue::new(rendering_ctx.glow_gl_api(), None);

        Self {
            egui,
            rendering_ctx,
        }
    }

    pub fn update(&mut self, browser: &AppBrowser) -> Duration {
        self.egui.run(|ctx| {
            if let Some(url) = browser.get_url() {
                let frame = egui::Frame::default()
                    .fill(ctx.style().visuals.window_fill)
                    .inner_margin(4.0);
                egui::TopBottomPanel::top("browser_url").frame(frame).show(ctx, |ui| {
                    ui.label(url.to_string());
                });
            }

            egui::CentralPanel::default().show(ctx, |ui| {
                let min = ui.cursor().min;
                let size = ui.available_size();
                let rect = egui::Rect::from_min_size(min, size);
                ui.allocate_space(size);

                browser.draw();

                if let Some(render_to_parent) = self.rendering_ctx.render_to_parent_callback() {
                    ui.painter().add(egui::PaintCallback {
                        rect,
                        callback: std::sync::Arc::new(egui_glow::CallbackFn::new(
                            move |info, painter| {
                                let clip = info.viewport_in_pixels();
                                let rect_in_parent = servo::euclid::Rect::new(
                                    servo::euclid::Point2D::new(clip.left_px, clip.from_bottom_px),
                                    servo::euclid::Size2D::new(clip.width_px, clip.height_px),
                                );
                                // Servo draws into egui's GL context here
                                render_to_parent(painter.gl(), rect_in_parent);
                            },
                        )),
                    });
                }
            });
        })
    }

    pub fn draw(&mut self, size: [u32; 2]) {
        self.rendering_ctx.parent_context().prepare_for_rendering();
        self.egui.paint(size);
        self.rendering_ctx.parent_context().present();
    }

    pub fn destroy(&mut self) {
        self.egui.destroy();
    }
}
