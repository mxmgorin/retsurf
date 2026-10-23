//! Servo startup configuration: `Opts`, `Preferences` (sized to the hardware),
//! the user-agent resolver, and the experimental-feature prefs. `Opts`/`Preferences`
//! feed [`servo::ServoBuilder`] at build; the experimental prefs are set after
//! `build()`, so [`set_experimental_prefs`] can re-apply them live.

use crate::{
    browser::memory,
    config::{
        self, js_preset::JsPreset, BrowserConfig, ExperimentalConfig, PageTheme, PerformanceConfig,
    },
};

/// Servo options: with `persist_site_data` on, `config_dir` points at the
/// `servo/` subfolder of the user data dir, which Servo's net and storage
/// threads read at startup and write back on a clean shutdown.
pub(super) fn build_opts(config: &BrowserConfig) -> servo::Opts {
    let mut opts = servo::Opts::default();
    if config.persist_site_data {
        opts.config_dir = Some(std::path::PathBuf::from(crate::config::servo_data_dir()));
    }
    opts
}

/// Servo preferences sized to the hardware (see [`PerformanceConfig`]) plus the
/// configured user agent. The thread pools are created at startup, so their
/// sizes must go through `ServoBuilder`, not `set_preference`.
pub(super) fn build_preferences(
    config: &BrowserConfig,
    perf: &PerformanceConfig,
) -> servo::Preferences {
    let cores = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(4) as i64;

    // The memory profile is the holistic baseline, from GC ceilings to thread
    // counts; `auto` resolves it from the build target and detected RAM.
    let profile = memory::resolve(perf.memory_profile);
    let mut prefs = memory::preferences(profile);

    // A tier's thread counts assume its own core count, so a smaller board is
    // clamped down — never up. Desktop keeps Servo's defaults untouched.
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

    // A steady caret, as in the chrome: every blink costs a frame.
    prefs.editing_caret_blink_time = 0;

    if let Some(ua) = resolve_user_agent(&config.user_agent) {
        log::info!("user agent: {ua}");
        prefs.user_agent = ua;
    }

    apply_pref_overrides(&mut prefs);

    log::info!(
        "servo: {cores} cores, memory profile `{}` -> layout={}, webrender pool={}, js_mem_max={}",
        profile.as_str(),
        prefs.layout_threads,
        prefs.thread_pool_webrender_workers_max,
        prefs.js_mem_max,
    );
    prefs
}

/// The memory profile's JS engine values, which a preset's unset keys keep.
#[derive(Clone, Copy)]
pub(super) struct JsBase {
    ion: bool,
    wasm_ion: bool,
    baseline_jit: bool,
    offthread_compilation: bool,
    /// MB; negative is unlimited.
    mem_max: i64,
}

impl JsBase {
    pub(super) fn of(prefs: &servo::Preferences) -> Self {
        Self {
            ion: prefs.js_ion_enabled,
            wasm_ion: prefs.js_wasm_ion_enabled,
            baseline_jit: prefs.js_baseline_jit_enabled,
            offthread_compilation: prefs.js_offthread_compilation_enabled,
            mem_max: prefs.js_mem_max,
        }
    }
}

/// The engine prefs that run `preset` over `base`; SpiderMonkey reads them when
/// a page's runtime starts.
pub(super) fn js_pref_values(
    base: JsBase,
    preset: &JsPreset,
) -> [(&'static str, servo::PrefValue); 5] {
    use servo::PrefValue::{Bool, Int};
    let mem_max = match (base.mem_max, preset.heap) {
        (max, Some(scale)) if max > 0 => ((max as f32 * scale).round() as i64).max(1),
        (max, _) => max,
    };
    [
        ("js_ion_enabled", Bool(preset.ion.unwrap_or(base.ion))),
        (
            "js_wasm_ion_enabled",
            Bool(preset.wasm_ion.unwrap_or(base.wasm_ion)),
        ),
        (
            "js_baseline_jit_enabled",
            Bool(preset.baseline_jit.unwrap_or(base.baseline_jit)),
        ),
        (
            "js_offthread_compilation_enabled",
            Bool(
                preset
                    .offthread_compilation
                    .unwrap_or(base.offthread_compilation),
            ),
        ),
        ("js_mem_max", Int(mem_max)),
    ]
}

/// `RETSURF_SERVO_PREFS=name=value[,...]` sets engine prefs the config does not
/// expose, for a measurement that would otherwise need a rebuild. The name is
/// checked against `Preferences::all_fields` first: Servo's setter panics.
fn apply_pref_overrides(prefs: &mut servo::Preferences) {
    let Ok(spec) = std::env::var("RETSURF_SERVO_PREFS") else {
        return;
    };
    for entry in spec.split(',').filter(|e| !e.trim().is_empty()) {
        let Some((name, value)) = entry.split_once('=') else {
            log::warn!("pref override `{entry}` is not name=value");
            continue;
        };
        let (name, value) = (name.trim(), value.trim());
        if !servo::Preferences::all_fields().contains(&name) {
            log::warn!("pref override `{name}` is not a servo preference");
            continue;
        }
        let Some(parsed) = pref_value(name, value) else {
            log::warn!(
                "pref override `{name}` wants {}, got `{value}`",
                servo::Preferences::type_of(name)
            );
            continue;
        };
        prefs.set_value(name, parsed);
        log::info!("pref override: {name} = {value}");
    }
}

/// Parse `value` as the type `name` holds. Servo's setter unwraps the conversion,
/// so a mismatch has to be caught here.
fn pref_value(name: &str, value: &str) -> Option<servo::PrefValue> {
    match servo::Preferences::type_of(name) {
        "bool" => value.parse().ok().map(servo::PrefValue::Bool),
        "i64" => value.parse().ok().map(servo::PrefValue::Int),
        "u64" => value.parse().ok().map(servo::PrefValue::UInt),
        "f64" => value.parse().ok().map(servo::PrefValue::Float),
        "alloc::string::String" => Some(servo::PrefValue::Str(value.to_owned())),
        _ => None,
    }
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

/// Resolve the `[browser] user_agent` config value: empty (or `default`) keeps
/// Servo's platform default, a keyword picks a stock UA string, anything else is
/// sent verbatim. `mobile` is the one that fits a handheld screen.
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
fn experimental_pref_values(exp: &ExperimentalConfig) -> [(&'static str, bool); 13] {
    // Destructured without `..` so a new feature fails to compile here rather
    // than shipping unapplied.
    let ExperimentalConfig {
        webgl2,
        webgpu,
        offscreen_canvas,
        columns,
        container_queries,
        fontface,
        intersection_observer,
        resize_observer,
        indexeddb,
        storage_manager,
        notification,
        async_clipboard,
        permissions,
    } = exp;
    [
        ("dom_webgl2_enabled", *webgl2),
        ("dom_webgpu_enabled", *webgpu),
        ("dom_offscreen_canvas_enabled", *offscreen_canvas),
        ("layout_columns_enabled", *columns),
        ("layout_container_queries_enabled", *container_queries),
        ("dom_fontface_enabled", *fontface),
        ("dom_intersection_observer_enabled", *intersection_observer),
        ("dom_resize_observer_enabled", *resize_observer),
        ("dom_indexeddb_enabled", *indexeddb),
        ("dom_storage_manager_api_enabled", *storage_manager),
        ("dom_notification_enabled", *notification),
        ("dom_async_clipboard_enabled", *async_clipboard),
        ("dom_permissions_enabled", *permissions),
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

    /// A preset's unset keys keep the profile's values; the heap scales a
    /// ceiling but leaves an unlimited one so.
    #[test]
    fn a_js_preset_lays_its_keys_over_the_profile() {
        let mut prefs = servo::Preferences {
            js_ion_enabled: false,
            js_mem_max: 100,
            ..servo::Preferences::default()
        };
        let base = JsBase::of(&prefs);
        let preset = JsPreset {
            wasm_ion: Some(false),
            heap: Some(1.5),
            ..JsPreset::default()
        };
        let values = js_pref_values(base, &preset);
        let get = |name: &str| values.iter().find(|(n, _)| *n == name).unwrap().1.clone();
        assert_eq!(get("js_ion_enabled"), servo::PrefValue::Bool(false));
        assert_eq!(get("js_wasm_ion_enabled"), servo::PrefValue::Bool(false));
        assert_eq!(get("js_mem_max"), servo::PrefValue::Int(150));
        prefs.js_mem_max = -1;
        let unlimited = js_pref_values(JsBase::of(&prefs), &preset);
        assert_eq!(unlimited[4].1, servo::PrefValue::Int(-1));
    }

    /// A tier that kept its memory cache only gains the disk file and budget.
    #[test]
    fn disk_cache_sizes_and_keeps_the_memory_cache() {
        let mut prefs = servo::Preferences::default();
        let weight = prefs.network_http_cache_size;
        apply_http_disk_cache(&mut prefs, 64, "/tmp/cache/");

        assert_eq!(
            prefs.network_http_disk_cache,
            "/tmp/cache/http-cache.sqlite3"
        );
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
