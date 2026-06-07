mod app;
mod config;
mod window;
mod event;
mod browser;
mod resources;
mod ui;

use crate::app::App;

fn main() {
    log::info!("Init main");
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Error initializing crypto provider");
    let env = env_logger::Env::default()
        .filter_or("RETSURF_LOG_LEVEL", "info")
        .write_style_or("RETSURF_LOG_STYLE", "always");
    env_logger::init_from_env(env);
    let mut app_config = config::AppConfig::default();
    if let Ok(v) = std::env::var("RETSURF_GLES") {
        app_config.interface.use_gles = v != "0";
    }

    if app_config.interface.use_gles {
        // SDL creates a GLES context (sets the thread's EGL API to ES). Servo's
        // surfman context must use the same API or context creation fails, so
        // force surfman to GLES too. Must be set before any surfman/SDL GL init.
        std::env::set_var("SURFMAN_FORCE_GLES", "1");
    }

    // surfman picks its display backend from the environment (Wayland if
    // WAYLAND_DISPLAY is set), independent of SDL. If SDL and surfman end up on
    // different display servers their GL contexts conflict and context creation
    // fails. On a Wayland desktop SDL still often defaults to x11, so align it to
    // Wayland. On the handheld (no WAYLAND_DISPLAY) this is skipped and SDL falls
    // back to its kmsdrm driver as intended. An explicit SDL_VIDEODRIVER wins.
    if std::env::var_os("SDL_VIDEODRIVER").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_some()
    {
        std::env::set_var("SDL_VIDEODRIVER", "wayland");
    }

    let mut sdl = sdl2::init().unwrap();
    let app = App::new(&mut sdl, app_config).unwrap();

    app.run();
}
