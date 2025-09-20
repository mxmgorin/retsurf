use crate::config::InterfaceConfig;
use sdl2::Sdl;
use servo::RenderingContext;
use std::rc::Rc;

pub struct AppWindow {
    _video_subsystem: sdl2::VideoSubsystem,
    _rendering_ctx: Rc<dyn servo::RenderingContext>,
    window: sdl2::video::Window,
    pub offscreen_rendering_ctx: Rc<servo::OffscreenRenderingContext>,
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

        let rendering_ctx = new_servo_window_context(&window)?;
        rendering_ctx
            .make_current()
            .map_err(|e| format!("failed rending_ctx.make_current {e:?}"))?;
        let rendering_ctx = Rc::new(rendering_ctx);
        let offscreen_rendering_ctx =
            Rc::new(rendering_ctx.offscreen_context(get_physizcal_size(&window)));

        Ok(Self {
            _video_subsystem: video_subsystem,
            window,
            _rendering_ctx: rendering_ctx,
            offscreen_rendering_ctx,
        })
    }

    pub fn get_rendering_ctx(&self) -> Rc<dyn servo::RenderingContext> {
        self.offscreen_rendering_ctx.clone()
    }

    pub fn get_size(&self) -> dpi::PhysicalSize<u32> {
        get_physizcal_size(&self.window)
    }
}

fn get_physizcal_size(window: &sdl2::video::Window) -> dpi::PhysicalSize<u32> {
    let (w, h) = window.size();

    dpi::PhysicalSize::new(w, h)
}

fn new_servo_window_context(
    window: &sdl2::video::Window,
) -> Result<servo::WindowRenderingContext, String> {
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
    // TODO: getting handles will fail without windlow manager
    let display_handle = window
        .display_handle()
        .map_err(|e| format!("Failed sdl_window.display_handle: {e:?}"))?;
    let window_handle = window
        .window_handle()
        .map_err(|e| format!("Failed sdl_window.window_handle: {e:?}"))?;
    let size = get_physizcal_size(window);

    servo::WindowRenderingContext::new(display_handle, window_handle, size)
        .map_err(|e| format!("Failed to create Servo WindowRenderingContext: {e:?}"))
}
