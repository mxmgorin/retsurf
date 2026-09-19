mod app;
mod browser;
mod clock;
mod command;
mod config;
mod data;
mod event;
mod list;
mod media;
mod net;
mod overlay;
mod platform;
mod ui;
mod update;

use crate::app::App;

/// Build identity for the startup log and the panic file: crate version, plus the
/// `HEAD` short hash and its committer date stamped in by `build.rs`. Both fall
/// back to `unknown` when built without a git checkout.
const BUILD_ID: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("RETSURF_GIT_HASH"),
    ", ",
    env!("RETSURF_BUILD_DATE"),
    ")"
);

/// Shared startup for the desktop and Android entry points. Everything
/// platform-specific is `cfg`-gated here.
pub fn run_app() {
    // Before anything else can panic: the handheld launcher discards stderr, so
    // a bare panic leaves no trace beyond exit code 101.
    install_panic_hook();

    init_logging();

    log::info!("Init main: retsurf {BUILD_ID}");
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Error initializing crypto provider");
    let mut app_config = config::AppConfig::load();
    platform::startup::prepare(&mut app_config);

    // Before SDL and Servo allocate anything: the knobs only bind what comes after.
    platform::heap::tune(browser::memory::resolve(
        app_config.performance.memory_profile,
    ));

    let mut sdl = sdl2::init().unwrap();
    let app = App::new(&mut sdl, app_config).unwrap();

    app.run();
}

#[cfg(target_os = "android")]
fn init_logging() {
    // No stderr on Android; route `log` to logcat (filter via `adb logcat -s retsurf`).
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("retsurf"),
    );
}

#[cfg(not(target_os = "android"))]
fn init_logging() {
    let env = env_logger::Env::default()
        .filter_or("RETSURF_LOG_LEVEL", "info")
        .write_style_or("RETSURF_LOG_STYLE", "always");
    let mut builder = env_logger::Builder::from_env(env);
    // The handheld launcher discards stderr too: mirror logs to a file when asked.
    if let Ok(path) = std::env::var("RETSURF_LOG_FILE") {
        match std::fs::File::create(&path) {
            Ok(file) => {
                builder.target(env_logger::Target::Pipe(Box::new(file)));
            }
            Err(e) => eprintln!("failed to open RETSURF_LOG_FILE `{path}`: {e}"),
        }
    }
    builder.init();
}

/// Mirror panics to a file in addition to stderr. The path is `RETSURF_PANIC_FILE`
/// if set, else `retsurf-panic.log` in the working directory. The default backtrace
/// hook still runs after us, so desktop behavior is unchanged.
fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let path =
            std::env::var("RETSURF_PANIC_FILE").unwrap_or_else(|_| "retsurf-panic.log".to_string());
        // The governor is machine-wide; leaving it raised costs battery until
        // the next reboot.
        platform::cpufreq::restore();
        let backtrace = std::backtrace::Backtrace::force_capture();
        // Android has no stderr, so a panic in a Servo thread would otherwise
        // vanish silently; the logger reaches logcat.
        log::error!("{info}\n\nbacktrace:\n{backtrace}");
        // Appended, not written: a device that crashes twice a session should
        // say so, and the first crash is usually the one that explains it.
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut file| {
                use std::io::Write;
                writeln!(
                    file,
                    "retsurf {BUILD_ID} at {at}\n\n{info}\n\nbacktrace:\n{backtrace}\n"
                )
            });
        default(info);
    }));
}
