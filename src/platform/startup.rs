//! Pre-SDL startup wiring: the env overrides and SDL/surfman hints that must be
//! in place before any video or GL init, gathered here so [`crate::run_app`]
//! reads as hooks, logging, config, prepare, run.

use crate::config::{self, AppConfig};

/// Fold the environment into `config` and set the process hints its choices
/// require. Must run before `sdl2::init`.
pub fn prepare(config: &mut AppConfig) {
    if let Some(gles) = config::env_flag("RETSURF_GLES") {
        config.display.use_gles = gles;
    }
    if let Some(software) = config::env_flag("RETSURF_SOFTWARE") {
        config.display.software_render = software;
    }
    // A launcher's way to try a frame cap without editing the config — and the
    // way to compare two of them in one sitting.
    if let Some(fps) = std::env::var("RETSURF_MAX_FPS")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
    {
        // Lands after `load`'s sanitize pass, so it clamps here.
        config.display.max_fps = fps.min(config::bounds::MAX_FPS.max as u32);
    }
    if config.display.software_render && !cfg!(feature = "software") {
        log::warn!(
            "software rendering asked for, but this build has no `software` feature; using GL"
        );
        config.display.software_render = false;
    }
    // Android GPUs (Mali/Adreno/PowerVR) only expose GLES; desktop GL is never an
    // option there, so the config/RETSURF_GLES toggle can't select it.
    #[cfg(target_os = "android")]
    {
        config.display.use_gles = true;
        // We handle the finger events ourselves, and SDL's synthesized clicks
        // would fire at the end of every scroll.
        std::env::set_var("SDL_TOUCH_MOUSE_EVENTS", "0");
    }

    if config.display.use_gles && !config.display.software_render {
        // SDL sets the thread's EGL API to ES, and surfman's context must match
        // or creation fails. Before any surfman or SDL GL init.
        std::env::set_var("SURFMAN_FORCE_GLES", "1");
    }

    // SDL defaults to x11 on a Wayland desktop while surfman reads WAYLAND_DISPLAY,
    // and two different display servers fail GL context creation.
    #[cfg(not(target_os = "android"))]
    if std::env::var_os("SDL_VIDEODRIVER").is_none()
        && std::env::var_os("WAYLAND_DISPLAY").is_some()
    {
        std::env::set_var("SDL_VIDEODRIVER", "wayland");
    }

    // Without this hint SDL lets Android background the activity on Back; with
    // it the button arrives as an AC_BACK key (mapped in crate::event::keyboard).
    #[cfg(target_os = "android")]
    std::env::set_var("SDL_ANDROID_TRAP_BACK_BUTTON", "1");
}
