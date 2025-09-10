mod app;
mod config;
mod render;
mod window;
mod input;

use input::handler::InputHandler;
use crate::app::App;

#[tokio::main]
async fn main() {
    let env = env_logger::Env::default()
        .filter_or("RETSURF_LOG_LEVEL", "info")
        .write_style_or("RETSURF_LOG_STYLE", "always");
    env_logger::init_from_env(env);
    log::info!("Starting app");
    let mut sdl = sdl2::init().unwrap();
    let app = App::new(&mut sdl, config::AppConfig::default()).unwrap();
    let mut input = InputHandler::new(&sdl).unwrap();
    app.run(&mut input);
}
