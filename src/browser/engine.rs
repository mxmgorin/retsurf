//! One-shot Servo startup configuration: `Opts`, `Preferences` (sized to the
//! hardware), the user-agent resolver, and the experimental-pref toggle. None
//! of this touches a live [`AppBrowser`] — it all feeds [`servo::ServoBuilder`]
//! at construction time.

use crate::{
    browser::memory,
    config::{BrowserConfig, PerformanceConfig},
};

static EXPERIMENTAL_PREFS: &[&str] = &[
    "dom_async_clipboard_enabled",
    "dom_fontface_enabled",
    "dom_intersection_observer_enabled",
    "dom_notification_enabled",
    "dom_offscreen_canvas_enabled",
    "dom_permissions_enabled",
    "dom_resize_observer_enabled",
    "dom_webgl2_enabled",
    "dom_webgpu_enabled",
    "layout_columns_enabled",
    "layout_container_queries_enabled",
    "layout_grid_enabled",
];

/// Servo options: with `persist_site_data` on, point `config_dir` at the
/// `servo/` subfolder of the user data dir — Servo's net and storage threads
/// then load cookies / HSTS / localStorage from it at startup and write them
/// back on a clean shutdown (see [`AppBrowser::shutdown`]), so logins survive
/// restarts. The subfolder keeps Servo's files apart from retsurf's own.
///
/// [`AppBrowser::shutdown`]: super::AppBrowser::shutdown
pub(super) fn build_opts(config: &BrowserConfig) -> servo::Opts {
    let mut opts = servo::Opts::default();
    if config.persist_site_data {
        opts.config_dir = Some(std::path::PathBuf::from(crate::config::servo_data_dir()));
    }
    opts
}

/// Servo preferences sized to the hardware (see [`PerformanceConfig`]) plus
/// the configured user agent. These must go through `ServoBuilder` — the
/// thread pools are created at startup, so `set_preference` after `build()`
/// would be too late.
pub(super) fn build_preferences(
    config: &BrowserConfig,
    perf: &PerformanceConfig,
) -> servo::Preferences {
    let cores = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(4) as i64;

    // The memory profile is the holistic baseline: JS GC ceilings, back-forward
    // cache depth, HTTP/canvas caches, which DOM subsystems start, and tier-sized
    // thread counts. `auto` resolves from the build target + detected RAM.
    let profile = memory::resolve(perf.memory_profile);
    let mut prefs = memory::preferences(profile);

    // Tiers hardcode thread counts for their assumed core count (e.g. Generous
    // assumes an octa-core A527); clamp down so a quad-core board isn't
    // oversubscribed. Only ever clamps down — never raises the tier's choice.
    // Desktop is left untouched: it's Servo's own defaults, run as upstream ships.
    if profile != crate::config::MemoryProfile::Desktop {
        let cores_u = cores as u64;
        prefs.layout_threads = prefs.layout_threads.clamp(1, cores);
        let clamp = |v: &mut u64| *v = (*v).clamp(1, cores_u);
        clamp(&mut prefs.thread_pool_async_runtime_workers_max);
        clamp(&mut prefs.thread_pool_fallback_workers);
        clamp(&mut prefs.thread_pool_workers_max);
        clamp(&mut prefs.thread_pool_webrender_workers_max);
    }

    // The explicit [performance] knobs still win when set (non-zero); `0` keeps
    // the tier's choice.
    if perf.layout_threads != 0 {
        prefs.layout_threads = perf.layout_threads as i64;
    }
    if perf.worker_pool_max != 0 {
        let n = perf.worker_pool_max as u64;
        prefs.thread_pool_async_runtime_workers_max = n;
        prefs.thread_pool_fallback_workers = n;
        prefs.thread_pool_workers_max = n;
        prefs.thread_pool_webrender_workers_max = n;
    }

    if let Some(ua) = resolve_user_agent(&config.user_agent) {
        log::info!("user agent: {ua}");
        prefs.user_agent = ua;
    }

    log::info!(
        "servo: {cores} cores, memory profile `{}` -> layout={}, webrender pool={}, js_mem_max={}",
        profile.as_str(),
        prefs.layout_threads,
        prefs.thread_pool_webrender_workers_max,
        prefs.js_mem_max,
    );
    prefs
}

/// Resolve the `[browser] user_agent` config value: empty (or `default`)
/// keeps Servo's platform default, the keywords pick a stock UA string, and
/// anything else is sent verbatim. `mobile` is the interesting one on a
/// handheld — sites serve their phone layouts, which fit a small screen far
/// better than the desktop ones.
fn resolve_user_agent(value: &str) -> Option<String> {
    let value = value.trim();
    let platform = match value.to_ascii_lowercase().as_str() {
        "" | "default" => return None,
        "desktop" => servo::UserAgentPlatform::Desktop,
        "mobile" | "android" => servo::UserAgentPlatform::Android,
        "ios" => servo::UserAgentPlatform::Ios,
        _ => return Some(value.to_string()),
    };
    Some(platform.to_user_agent_string())
}

pub(super) fn set_experimental_prefs(servo: &servo::Servo, value: bool) {
    let value = servo::PrefValue::Bool(value);

    for pref in EXPERIMENTAL_PREFS {
        servo.set_preference(pref, value.clone());
    }
}
