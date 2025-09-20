use crate::config::InterfaceConfig;
use egui_sdl2_gl::gl;
use sdl2::Sdl;
use servo::{euclid::Point2D, webrender_api::units::DeviceIntRect, RenderingContext};
use std::{rc::Rc, time::Instant};

pub struct AppWindow {
    _video_subsystem: sdl2::VideoSubsystem,
    window: sdl2::video::Window,
    rendering_ctx: Rc<dyn servo::RenderingContext>,
    painter: egui_sdl2_gl::painter::Painter,
    egui_state: egui_sdl2_gl::EguiStateHandler,
    egui_ctx: egui::Context,
    start_time: Instant,
    egui_texture: Option<egui::TextureHandle>,
}

impl AppWindow {
    pub fn new(sdl: &Sdl, config: &InterfaceConfig) -> Result<Self, String> {
        let video_subsystem = sdl.video().unwrap();
        // let gl_attr = video_subsystem.gl_attr();
        // gl_attr.set_context_profile(sdl2::video::GLProfile::GLES);
        // gl_attr.set_context_version(3, 0);
        // gl_attr.set_double_buffer(true);
        // gl_attr.set_multisample_samples(4);
        let window = video_subsystem
            .window("Refsurf", config.width, config.height)
            .opengl()
            .resizable()
            .build()
            .unwrap();

        let rending_ctx = new_servo_window_context(&window)?;
        rending_ctx
            .make_current()
            .map_err(|e| format!("failed rending_ctx.make_current {e:?}"))?;
        let shader_ver = egui_sdl2_gl::ShaderVersion::Adaptive;
        let (painter, egui_state) =
            egui_sdl2_gl::with_sdl2(&window, shader_ver, egui_sdl2_gl::DpiScaling::Default);
        let egui_ctx = egui::Context::default();
        let image = read_image(&rending_ctx);
        let egui_texture = image.map(|i| upload_imagebuffer_to_egui(&egui_ctx, &i));

        Ok(Self {
            _video_subsystem: video_subsystem,
            window,
            rendering_ctx: Rc::new(rending_ctx),
            painter,
            egui_state,
            egui_ctx,
            start_time: Instant::now(),
            egui_texture,
        })
    }

    pub fn show(&mut self) {
        let Some(image) = read_image(self.rendering_ctx.as_ref()) else {
            return;
        };

        if let Some(texture) = &mut self.egui_texture {
            let (w, h) = image.dimensions();
            let color_image =
                egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], image.as_raw());
            texture.set(color_image, egui::TextureOptions::NEAREST);

            self.egui_state.input.time = Some(self.start_time.elapsed().as_secs_f64());
            self.egui_ctx.begin_pass(self.egui_state.input.take());

            egui::CentralPanel::default().show(&self.egui_ctx, |ui| {
                ui.image((texture.id(), texture.size_vec2()));
            });

            let egui::FullOutput {
                platform_output,
                textures_delta,
                shapes,
                pixels_per_point,
                viewport_output,
            } = self.egui_ctx.end_pass();
            self.egui_state
                .process_output(&self.window, &platform_output);
            let clipped_primitive = self.egui_ctx.tessellate(shapes, pixels_per_point);
            self.painter.paint_jobs(None, textures_delta, clipped_primitive);
            // self.window.gl_swap_window();
        }

        self.rendering_ctx.present();
    }

    pub fn get_rendering_ctx(&self) -> Rc<dyn servo::RenderingContext> {
        self.rendering_ctx.clone()
    }
}

fn new_servo_window_context(
    sdl_window: &sdl2::video::Window,
) -> Result<servo::WindowRenderingContext, String> {
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
    // TODO: getting handles will fail without windlow manager
    let display_handle = sdl_window
        .display_handle()
        .map_err(|e| format!("Failed sdl_window.display_handle: {e:?}"))?;
    let window_handle = sdl_window
        .window_handle()
        .map_err(|e| format!("Failed sdl_window.window_handle: {e:?}"))?;
    let (w, h) = sdl_window.size();
    let size = dpi::PhysicalSize::new(w, h);

    servo::WindowRenderingContext::new(display_handle, window_handle, size)
        .map_err(|e| format!("Failed to create Servo WindowRenderingContext: {e:?}"))
}

fn new_servo_software_context(
    sdl_window: &sdl2::video::Window,
) -> Result<servo::SoftwareRenderingContext, String> {
    let (w, h) = sdl_window.size();
    let size = dpi::PhysicalSize::new(w, h);

    servo::SoftwareRenderingContext::new(size)
        .map_err(|e| format!("Failed to create Servo RenderingContext: {e:?}"))
}

fn read_image(ctx: &dyn RenderingContext) -> Option<image::RgbaImage> {
    let size = ctx.size();
    let origin = Point2D::new(0, 0);
    let size = Point2D::new(size.width as i32, size.height as i32);
    let rect = DeviceIntRect::new(origin, size);

    ctx.read_to_image(rect)
}

fn upload_imagebuffer_to_egui(ctx: &egui::Context, img: &image::RgbaImage) -> egui::TextureHandle {
    let (width, height) = img.dimensions();
    let rgba = img.as_raw();
    let color_image =
        egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], rgba);

    ctx.load_texture("rendingbuffer", color_image, egui::TextureOptions::LINEAR)
}
