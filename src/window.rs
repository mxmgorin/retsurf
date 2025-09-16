use crate::config::InterfaceConfig;
use sdl2::Sdl;
use servo::RenderingContext;
use std::rc::Rc;

pub struct AppWindow {
    _video_subsystem: sdl2::VideoSubsystem,
    _window: sdl2::video::Window,
    rendering_ctx: Rc<dyn servo::RenderingContext>,
}

impl AppWindow {
    pub fn new(sdl: &Sdl, config: &InterfaceConfig) -> Result<Self, String> {
        let video_subsystem = sdl.video().unwrap();
        let gl_attr = video_subsystem.gl_attr();
        gl_attr.set_context_profile(sdl2::video::GLProfile::GLES);
        gl_attr.set_context_version(3, 0);

        let window = video_subsystem
            .window("Refsurf", config.width, config.height)
            .opengl()
            .resizable()
            .build()
            .unwrap();

        // let gl_ctx = window.gl_create_context().unwrap();
        let rending_ctx = new_servo_context(&window)?;
        rending_ctx
            .make_current()
            .map_err(|e| format!("failed rending_ctx.make_current {e:?}"))?;

        Ok(Self {
            _video_subsystem: video_subsystem,
            // _gl_ctx: gl_ctx,
            _window: window,
            rendering_ctx: Rc::new(rending_ctx),
        })
    }

    pub fn show(&self) {
        self.rendering_ctx.present();
    }

    pub fn get_rendering_ctx(&self) -> Rc<dyn servo::RenderingContext> {
        self.rendering_ctx.clone()
    }
}

fn new_servo_context(
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
