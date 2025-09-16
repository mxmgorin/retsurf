mod app;
mod config;
mod window;
mod event;
mod browser;
mod resources;

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
    let mut sdl = sdl2::init().unwrap();
    let app = App::new(&mut sdl, config::AppConfig::default()).unwrap();

    app.run();
}
