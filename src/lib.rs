mod app;
mod browser;
mod config;
mod data;
mod event;
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

/// Shared startup used by both the desktop `main` binary and the Android
/// `SDL_main` entry point (`android/lib`). Everything platform-specific is
/// `cfg`-gated here so the two callers stay trivial.
pub fn run_app() {
    // Capture panics before anything else can panic. On the handheld the launcher
    // usually discards stderr, so a bare panic leaves no trace beyond exit code 101;
    // mirroring it to a file is how we recover the message and location.
    install_panic_hook();

    init_logging();

    log::info!("Init main: retsurf {BUILD_ID}");
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Error initializing crypto provider");
    let mut app_config = config::AppConfig::load();
    if let Ok(v) = std::env::var("RETSURF_GLES") {
        app_config.display.use_gles = v != "0";
    }
    // Android GPUs (Mali/Adreno/PowerVR) only expose GLES; desktop GL is never an
    // option there, so the config/RETSURF_GLES toggle can't select it.
    #[cfg(target_os = "android")]
    {
        app_config.display.use_gles = true;
        // Don't let SDL synthesize mouse events from touch: we handle finger
        // events ourselves (drag→scroll, tap→click in `event::touch`), and the
        // synthesized clicks would otherwise fire at the end of every scroll.
        std::env::set_var("SDL_TOUCH_MOUSE_EVENTS", "0");
    }

    if app_config.display.use_gles {
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
    // Android has its own SDL video driver and no WAYLAND_DISPLAY, so skip it.
    #[cfg(not(target_os = "android"))]
    if std::env::var_os("SDL_VIDEODRIVER").is_none()
        && std::env::var_os("WAYLAND_DISPLAY").is_some()
    {
        std::env::set_var("SDL_VIDEODRIVER", "wayland");
    }

    // Android delivers the hardware/gesture Back button to the app as an
    // AC_BACK key event only when this hint is set; otherwise SDL lets Android
    // background the activity and the app never sees it. We map AC_BACK to the
    // Back/Cancel intent in src/event/keyboard.rs.
    #[cfg(target_os = "android")]
    std::env::set_var("SDL_ANDROID_TRAP_BACK_BUTTON", "1");

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
        let backtrace = std::backtrace::Backtrace::force_capture();
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
                writeln!(file, "retsurf {BUILD_ID} at {at}\n\n{info}\n\nbacktrace:\n{backtrace}\n")
            });
        default(info);
    }));
}
