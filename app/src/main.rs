#[tokio::main]
async fn main() {
    let env = env_logger::Env::default()
        .filter_or("RETSURF_LOG_LEVEL", "info")
        .write_style_or("RETSURF_LOG_STYLE", "always");
    env_logger::init_from_env(env);
    log::info!("Starting app");
}
