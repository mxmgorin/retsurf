use egui::{ViewportId, ViewportOutput};
use egui_glow::ShaderVersion;
use std::sync::Arc;

/// Integration between [`egui`] and [`glow`] for app based on [`sdl2`].
pub struct EguiGlue {
    pub ctx: egui::Context,
    pub painter: egui_glow::Painter,
    pub state: retsurf::egui_sdl2::State,

    // output from the last run:
    shapes: Vec<egui::epaint::ClippedShape>,
    pixels_per_point: f32,
    textures_delta: egui::TexturesDelta,
}

impl EguiGlue {
    /// For automatic shader version detection set `shader_version` to `None`.
    pub fn new(
        gl_ctx: Arc<glow::Context>,
        window: &sdl2::video::Window,
        shader_version: Option<ShaderVersion>,
    ) -> Self {
        let painter = egui_glow::Painter::new(gl_ctx, "", shader_version, false)
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

    pub fn on_event(
        &mut self,
        window: &sdl2::video::Window,
        event: &sdl2::event::Event,
    ) -> retsurf::egui_sdl2::EventResponse {
        self.state.on_event(window, event)
    }

    /// Returns the `Duration` of the timeout after which egui should be repainted even if there's no new events.
    ///
    /// Call [`Self::paint`] later to paint.
    pub fn run(
        &mut self,
        window: &sdl2::video::Window,
        run_ui: impl FnMut(&egui::Context),
    ) -> std::time::Duration {
        let raw_input = self.state.take_egui_input(window);
        let egui::FullOutput {
            platform_output,
            viewport_output,
            textures_delta,
            shapes,
            pixels_per_point,
        } = self.ctx.run(raw_input, run_ui);

        self.state.handle_platform_output(window, platform_output);

        self.shapes = shapes;
        self.textures_delta.append(textures_delta);
        self.pixels_per_point = pixels_per_point;

        match viewport_output.get(&ViewportId::ROOT) {
            Some(&ViewportOutput { repaint_delay, .. }) => repaint_delay,
            None => std::time::Duration::ZERO,
        }
    }

    /// Paint the results of the last call to [`Self::run`].
    pub fn paint(&mut self, screen_size: [u32; 2]) {
        let mut textures_delta = std::mem::take(&mut self.textures_delta);

        for (id, image_delta) in textures_delta.set {
            self.painter.set_texture(id, &image_delta);
        }

        let pixels_per_point = self.pixels_per_point;
        let shapes = std::mem::take(&mut self.shapes);
        let clipped_primitives = self.ctx.tessellate(shapes, pixels_per_point);
        self.painter
            .paint_primitives(screen_size, pixels_per_point, &clipped_primitives);

        for id in textures_delta.free.drain(..) {
            self.painter.free_texture(id);
        }
    }

    /// Call to release the allocated graphics resources.
    pub fn destroy(&mut self) {
        self.painter.destroy();
    }
}
