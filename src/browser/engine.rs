//! Servo startup configuration: `Opts`, `Preferences` (sized to the hardware),
//! the user-agent resolver, and the experimental-feature prefs. `Opts`/`Preferences`
//! feed [`servo::ServoBuilder`] at build; the experimental prefs are set after
//! `build()`, so [`set_experimental_prefs`] can re-apply them live.

use crate::{
    browser::memory,
    config::{self, BrowserConfig, ExperimentalConfig, PageTheme, PerformanceConfig},
};

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

    // Guarded so an off knob doesn't create the cache dir.
    if perf.http_disk_cache_mb != 0 {
        apply_http_disk_cache(&mut prefs, perf.http_disk_cache_mb, &config::cache_dir());
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

const BYTES_PER_MB: u64 = 1024 * 1024;

/// Memory-cache entry count for a tier that switched the cache off: the disk
/// store only ever gets what the memory cache evicts, so zero means zero.
const SPILL_MEMORY_CACHE_ENTRIES: u64 = 16;

/// Size Servo's on-disk HTTP cache and point it at a file in `cache_dir`.
fn apply_http_disk_cache(prefs: &mut servo::Preferences, budget_mb: u32, cache_dir: &str) {
    // The pref is the SQLite file itself, not a directory.
    prefs.network_http_disk_cache = format!("{cache_dir}http-cache.sqlite3");
    prefs.network_http_disk_cache_size = u64::from(budget_mb) * BYTES_PER_MB;
    if prefs.network_http_cache_disabled {
        prefs.network_http_cache_disabled = false;
        prefs.network_http_cache_size = SPILL_MEMORY_CACHE_ENTRIES;
    }
    log::info!(
        "http disk cache: {budget_mb} MB at {}, memory cache weight {}",
        prefs.network_http_disk_cache,
        prefs.network_http_cache_size,
    );
}

/// The `prefers-color-scheme` value a `[browser] page_theme` reports to pages.
pub(super) fn theme(page_theme: PageTheme) -> servo::Theme {
    if page_theme.prefers_dark() {
        servo::Theme::Dark
    } else {
        servo::Theme::Light
    }
}

/// The UA string Servo browses with; retsurf's own download fetches send the
/// same one so servers see a single client (see [`crate::data::downloads`]).
pub fn effective_user_agent(config: &BrowserConfig) -> String {
    resolve_user_agent(&config.user_agent)
        .unwrap_or_else(|| servo::Preferences::default().user_agent)
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

/// `(Servo pref, enabled)` per feature. Every name must exist in the pinned
/// `servo-config` — `set_preference` panics on an unknown pref.
fn experimental_pref_values(exp: &ExperimentalConfig) -> [(&'static str, bool); 12] {
    [
        ("dom_webgl2_enabled", exp.webgl2),
        ("dom_webgpu_enabled", exp.webgpu),
        ("dom_offscreen_canvas_enabled", exp.offscreen_canvas),
        ("layout_grid_enabled", exp.grid),
        ("layout_columns_enabled", exp.columns),
        ("layout_container_queries_enabled", exp.container_queries),
        ("dom_fontface_enabled", exp.fontface),
        ("dom_intersection_observer_enabled", exp.intersection_observer),
        ("dom_resize_observer_enabled", exp.resize_observer),
        ("dom_notification_enabled", exp.notification),
        ("dom_async_clipboard_enabled", exp.async_clipboard),
        ("dom_permissions_enabled", exp.permissions),
    ]
}

/// Apply the experimental prefs (after `build()` and on settings change).
/// Effective on the next page load, not already-loaded pages.
pub(super) fn set_experimental_prefs(servo: &servo::Servo, exp: &ExperimentalConfig) {
    for (pref, on) in experimental_pref_values(exp) {
        servo.set_preference(pref, servo::PrefValue::Bool(on));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tier that kept its memory cache only gains the disk file and budget.
    #[test]
    fn disk_cache_sizes_and_keeps_the_memory_cache() {
        let mut prefs = servo::Preferences::default();
        let weight = prefs.network_http_cache_size;
        apply_http_disk_cache(&mut prefs, 64, "/tmp/cache/");

        assert_eq!(prefs.network_http_disk_cache, "/tmp/cache/http-cache.sqlite3");
        assert_eq!(prefs.network_http_disk_cache_size, 64 * BYTES_PER_MB);
        assert!(!prefs.network_http_cache_disabled);
        assert_eq!(prefs.network_http_cache_size, weight);
    }

    /// Nothing spills without a memory cache, so the low tiers get a small one.
    #[test]
    fn disk_cache_revives_a_disabled_memory_cache() {
        let mut prefs = servo::Preferences {
            network_http_cache_disabled: true,
            ..Default::default()
        };
        apply_http_disk_cache(&mut prefs, 8, "/tmp/cache/");

        assert!(!prefs.network_http_cache_disabled);
        assert_eq!(prefs.network_http_cache_size, SPILL_MEMORY_CACHE_ENTRIES);
    }
}
