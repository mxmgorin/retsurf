use egui::ViewportId;
use egui_glow::ShaderVersion;
use std::{sync::Arc, time::Duration};

/// Integration between [`egui`] and [`glow`] for app based on [`sdl2`].
pub struct EguiGlow {
    pub ctx: egui::Context,
    pub painter: egui_glow::Painter,
    pub state: retsurf::egui_sdl2::State,

    // output from the last run:
    shapes: Vec<egui::epaint::ClippedShape>,
    pixels_per_point: f32,
    textures_delta: egui::TexturesDelta,
}

impl EguiGlow {
    /// For automatic shader version detection set `shader_version` to `None`.
    pub fn new(
        window: &sdl2::video::Window,
        glow_ctx: Arc<glow::Context>,
        shader_version: Option<ShaderVersion>,
        dithering: bool,
    ) -> Self {
        let painter = egui_glow::Painter::new(glow_ctx, "", shader_version, dithering)
            .map_err(|err| {
                log::error!("error occurred in initializing painter:\n{err}");
            })
            .unwrap();
        let ctx = egui::Context::default();
        let state = retsurf::egui_sdl2::State::new(window, ctx.clone(), ViewportId::ROOT);

        Self {
            ctx,
            painter,
            state,
            shapes: Default::default(),
            pixels_per_point: 1.0,
            textures_delta: Default::default(),
        }
    }

    /// Returns the `Duration` of the timeout after which egui should be repainted even if there's no new events.
    ///
    /// Call [`Self::paint`] later to paint.
    pub fn run(&mut self, run_ui: impl FnMut(&egui::Context)) -> Duration {
        let raw_input = self.state.take_egui_input();
        let egui::FullOutput {
            platform_output,
            viewport_output,
            textures_delta,
            shapes,
            pixels_per_point,
        } = self.ctx.run(raw_input, run_ui);
        self.state.handle_platform_output(platform_output);

        self.shapes = shapes;
        self.textures_delta.append(textures_delta);
        self.pixels_per_point = pixels_per_point;

        viewport_output
            .get(&ViewportId::ROOT)
            .map(|x| x.repaint_delay)
            .unwrap_or_else(|| Duration::ZERO)
    }

    /// Paint the results of the last call to [`Self::run`].
    pub fn paint(&mut self) {
        let mut textures_delta = std::mem::take(&mut self.textures_delta);

        for (id, image_delta) in textures_delta.set {
            self.painter.set_texture(id, &image_delta);
        }

        let pixels_per_point = self.pixels_per_point;
        let shapes = std::mem::take(&mut self.shapes);
        let clipped_primitives = self.ctx.tessellate(shapes, pixels_per_point);
        let size = self.state.get_window_size();
        self.painter
            .paint_primitives(size.into(), pixels_per_point, &clipped_primitives);

        for id in textures_delta.free.drain(..) {
            self.painter.free_texture(id);
        }
    }

    /// Call to release the allocated graphics resources.
    pub fn destroy(&mut self) {
        self.painter.destroy();
    }
}
