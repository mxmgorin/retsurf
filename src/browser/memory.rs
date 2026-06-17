//! Memory/performance profiles for the embedded Servo engine. Each tier of
//! [`MemoryProfile`] bundles memory-oriented Servo prefs tuned for a class of
//! hardware; lower tiers use less memory at some cost to performance.
//!
//! Field names match `servo_config` 0.3.0's `Preferences` (re-exported from the
//! `servo` crate). All fields are public, so each tier starts from `Default` and
//! overrides only what matters. The thread counts here are *baselines* —
//! [`crate::browser::build_preferences`] clamps them down to the machine's core
//! count and lets the `[performance]` config knobs override them.
//!
//! Unit notes:
//!   * `js_mem_max`, `*_limit_mb`: megabytes (`js_mem_max = -1` => unlimited).
//!   * `*_heap_growth*`: percent (150 == heap may reach 150% before next GC).
//!   * `network_http_cache_size`: an opaque relative *weight*, not bytes/count.
//!
//! TARGET HARDWARE (RK3566, Allwinner H700, A527, A133 Plus, RK3326):
//!   * All are aarch64 with weak *in-order* little cores — Cortex-A35 (RK3326),
//!     A53 (H700, A133 Plus), A55 (RK3566, A527). JS is CPU-bound here, so NEVER
//!     drop the baseline JIT (only the optimizing Ion compiler) — interpreter-
//!     only would be unusable.
//!   * UNIFIED MEMORY: the GPU shares system RAM (Mali G31/G52/G57 or PowerVR
//!     GE8300), so decoded images, WebRender tiles, and GL surfaces all draw
//!     from the same pool — the per-renderer ceiling must budget for GPU memory
//!     too. There is no image-cache byte-size pref; eviction +
//!     `session_history_max_length` are the only levers for image/tile memory.
//!   * Memory bandwidth is limited (single/dual-channel LPDDR3/4), so piling on
//!     layout/WebRender threads yields little — keep thread counts modest.
//!
//! Rough target -> tier mapping (refined by the board's actual RAM):
//!   * RK3326 / H700 (4xA35/A53, ~1 GB) -> Tight (keep baseline JIT)
//!   * A133 Plus (4xA53, PowerVR, 1-2 GB) -> Tight (1 GB) / Balanced (2 GB)
//!   * RK3566 (4xA55, 1-8 GB) -> Tight (1 GB) / Balanced (2-4 GB)
//!   * A527 (8xA55, 2-4 GB) -> Balanced (2 GB) / Generous (4 GB)
//!   * Android phone/tablet (>3 GB, big cores) -> Android
//!   * Desktop (8 GB+) -> Desktop (Servo's untouched defaults)

// Each tier builds its `Preferences` by starting from `Default` and mutating the
// handful of fields that matter (see the module docs), with a comment per field.
// That's far clearer here than a struct-update literal burying the overrides in a
// 30-field initializer — and it keeps each field independent across servo bumps.
#![allow(clippy::field_reassign_with_default)]

use crate::config::MemoryProfile;
use servo::Preferences;

/// Resolve [`MemoryProfile::Auto`] to a concrete tier from the build target and
/// detected RAM; every other value passes through unchanged.
pub fn resolve(profile: MemoryProfile) -> MemoryProfile {
    if profile != MemoryProfile::Auto {
        return profile;
    }
    for_target(detect_ram_mb())
}

/// The Servo preferences for a concrete tier. `Auto` is resolved by [`resolve`]
/// before reaching here; it falls back to [`balanced`] defensively.
pub fn preferences(profile: MemoryProfile) -> Preferences {
    match profile {
        MemoryProfile::Embedded => embedded(),
        MemoryProfile::Tight => tight(),
        MemoryProfile::Auto | MemoryProfile::Balanced => balanced(),
        MemoryProfile::Generous => generous(),
        MemoryProfile::Android => android(),
        MemoryProfile::Desktop => desktop(),
    }
}

/// Platform-aware selector for `Auto`. Android and desktop are decided by the
/// compile target (they also report `target_os = "linux"` is false / true
/// respectively); the handheld Linux boards fall back to RAM. The >6 GB guard
/// avoids mistaking a high-RAM handheld for a desktop.
fn for_target(ram_mb: u64) -> MemoryProfile {
    if cfg!(target_os = "android") {
        MemoryProfile::Android
    } else if cfg!(any(target_os = "windows", target_os = "macos"))
        // Desktop OSes always; high-RAM Linux too (>6 GB rules out a handheld).
        || (cfg!(target_os = "linux") && ram_mb > 6144)
    {
        MemoryProfile::Desktop
    } else {
        suggest(ram_mb)
    }
}

/// Pick a handheld tier from total RAM. Thresholds are deliberately conservative
/// because GPU memory shares this same pool (unified memory).
fn suggest(ram_mb: u64) -> MemoryProfile {
    match ram_mb {
        0..=768 => MemoryProfile::Embedded,
        769..=1536 => MemoryProfile::Tight, // ~1 GB boards: RK3326, H700, 1 GB RK3566
        1537..=3072 => MemoryProfile::Balanced, // ~2 GB: RK3566, A527
        _ => MemoryProfile::Generous,       // 4 GB+ handheld: A527, high-RAM RK3566
    }
}

/// Total system RAM in MB, read from `/proc/meminfo` (Linux/Android). Desktop
/// Windows/macOS never reach the size decision (their tier is chosen by
/// `target_os` in [`for_target`]), so a parse miss just yields a mid-range guess.
fn detect_ram_mb() -> u64 {
    const FALLBACK_MB: u64 = 2048;
    let Ok(text) = std::fs::read_to_string("/proc/meminfo") else {
        return FALLBACK_MB;
    };
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            if let Some(kb) = rest
                .split_whitespace()
                .next()
                .and_then(|n| n.parse::<u64>().ok())
            {
                return kb / 1024;
            }
        }
    }
    FALLBACK_MB
}

/// ~512 MB (sub-1 GB boards): the floor. Baseline JIT kept (Ion off) because the
/// target cores are weak and in-order; single-threaded layout and pools; tiniest
/// GC ceiling; no caches. Slowest tier, but JS remains usable.
fn embedded() -> Preferences {
    let mut p = Preferences::default();

    // Single worker everywhere — minimize per-thread stack/arena overhead.
    p.thread_pool_workers_max = 1;
    p.thread_pool_webrender_workers_max = 1;
    p.thread_pool_async_runtime_workers_max = 1;
    p.thread_pool_fallback_workers = 1;
    p.layout_threads = 1;

    // Keep the BASELINE JIT (weak in-order cores need it); drop only the
    // optimizing Ion compiler and its wasm counterpart to shed code memory.
    // Off-thread compilation would spawn helper threads we don't want here.
    p.js_ion_enabled = false;
    p.js_wasm_ion_enabled = false;
    p.js_offthread_compilation_enabled = false;
    // p.js_disable_jit = true; // last resort only (interpreter only)

    // Smallest practical GC ceiling; collect as eagerly as possible.
    p.js_mem_max = 64; // MB
    p.js_mem_gc_high_frequency_high_limit_mb = 48;
    p.js_mem_gc_high_frequency_low_limit_mb = 16;
    p.js_mem_gc_high_frequency_heap_growth_max = 130; // %
    p.js_mem_gc_high_frequency_heap_growth_min = 105; // %
    p.js_mem_gc_low_frequency_heap_growth = 105; // %
    p.js_mem_gc_empty_chunk_count_min = 0; // hand empty chunks straight back to the OS
    p.js_mem_gc_compacting_enabled = true;
    p.js_mem_gc_incremental_enabled = true;
    p.js_mem_gc_incremental_slice_ms = 3; // ms — keep a GC step well under a frame
    // Defensive: pin current Servo defaults so a crate bump can't silently flip them.
    p.js_mem_gc_per_zone_enabled = false; // collect all zones together
    p.js_baseline_interpreter_enabled = true; // never drop JS to the slow path

    // Keep only the current page; no back-forward cache to speak of.
    p.session_history_max_length = 1;

    // No in-memory HTTP cache.
    p.network_http_cache_disabled = true;

    // Drop the glyph-cache cost of subpixel AA (grayscale AA still on).
    p.gfx_subpixel_text_antialiasing_enabled = false;

    // Everything optional off.
    p.dom_webgpu_enabled = false;
    p.dom_webgl2_enabled = false;
    p.dom_webxr_enabled = false;
    p.dom_bluetooth_enabled = false;
    p.dom_serviceworker_enabled = false;
    p.dom_sharedworker_enabled = false;
    p.dom_worklet_enabled = false;
    p.accessibility_enabled = false;

    p
}

/// ~1 GB: aggressive. Minimal parallelism, low GC ceiling, most optional
/// subsystems off, optimizing JIT (Ion) off to shed generated-code memory while
/// keeping the baseline JIT for usable speed.
fn tight() -> Preferences {
    let mut p = Preferences::default();

    p.thread_pool_workers_max = 2;
    p.thread_pool_webrender_workers_max = 1;
    p.thread_pool_async_runtime_workers_max = 1;
    p.thread_pool_fallback_workers = 1;
    p.layout_threads = 1;

    // SpiderMonkey GC: collect early and often, keep the ceiling low.
    p.js_mem_max = 128; // MB
    p.js_mem_gc_high_frequency_high_limit_mb = 80;
    p.js_mem_gc_high_frequency_low_limit_mb = 30;
    p.js_mem_gc_high_frequency_heap_growth_max = 150; // %
    p.js_mem_gc_high_frequency_heap_growth_min = 110; // %
    p.js_mem_gc_low_frequency_heap_growth = 110; // %
    p.js_mem_gc_empty_chunk_count_min = 0; // return empty chunks to the OS
    p.js_mem_gc_compacting_enabled = true;
    p.js_mem_gc_incremental_enabled = true;
    p.js_mem_gc_incremental_slice_ms = 4; // ms — keep a GC step well under a frame
    // Defensive: pin current Servo defaults so a crate bump can't silently flip them.
    p.js_mem_gc_per_zone_enabled = false; // collect all zones together
    p.js_baseline_interpreter_enabled = true; // never drop JS to the slow path

    // Drop the heavy optimizing JIT; keep baseline JIT. Also drop wasm Ion to
    // cut compile-time CPU/memory.
    p.js_ion_enabled = false;
    p.js_wasm_ion_enabled = false;

    // Back-forward cache: keep almost nothing.
    p.session_history_max_length = 3;

    // No in-memory HTTP cache.
    p.network_http_cache_disabled = true;

    // Small LCD panels + weak GPU (Mali or PowerVR): skip subpixel AA (it
    // ~3x's the glyph cache and often mis-renders on these panels). Grayscale
    // AA stays on.
    p.gfx_subpixel_text_antialiasing_enabled = false;

    // Skip standing up optional subsystems.
    p.dom_webgpu_enabled = false;
    p.dom_webgl2_enabled = false;
    p.dom_webxr_enabled = false;
    p.dom_bluetooth_enabled = false;
    p.dom_serviceworker_enabled = false;
    p.dom_sharedworker_enabled = false;
    p.dom_worklet_enabled = false;
    p.accessibility_enabled = false;

    p
}

/// ~2 GB: balanced. Modest parallelism, moderate GC ceiling, keep WebGL2 and
/// service workers; full JIT on.
fn balanced() -> Preferences {
    let mut p = Preferences::default();

    p.thread_pool_workers_max = 4;
    p.thread_pool_webrender_workers_max = 2;
    p.thread_pool_async_runtime_workers_max = 2;
    p.thread_pool_fallback_workers = 2;
    p.layout_threads = 2;

    p.js_mem_max = 256; // MB
    p.js_mem_gc_high_frequency_high_limit_mb = 150;
    p.js_mem_gc_high_frequency_low_limit_mb = 50;
    p.js_mem_gc_high_frequency_heap_growth_max = 200; // %
    p.js_mem_gc_high_frequency_heap_growth_min = 120; // %
    p.js_mem_gc_low_frequency_heap_growth = 120; // %
    p.js_mem_gc_empty_chunk_count_min = 0;
    p.js_mem_gc_compacting_enabled = true;
    p.js_mem_gc_incremental_enabled = true;
    p.js_mem_gc_incremental_slice_ms = 6; // ms — desktop default is 10
    // Defensive: pin current Servo defaults so a crate bump can't silently flip them.
    p.js_mem_gc_per_zone_enabled = false; // collect all zones together
    p.js_baseline_interpreter_enabled = true; // never drop JS to the slow path

    p.session_history_max_length = 6;

    // Small HTTP cache (weight is a relative dial — tune to your profiler).
    p.network_http_cache_size = 32;

    // Trim the rarely-needed subsystems; keep WebGL2 + service workers.
    p.dom_webgpu_enabled = false;
    p.dom_webxr_enabled = false;
    p.dom_bluetooth_enabled = false;
    p.dom_sharedworker_enabled = false;
    p.dom_worklet_enabled = false;

    p
}

/// 4 GB+: the relaxed handheld tier (realistically an octa-core A527 or a 4 GB
/// RK3566). Higher GC ceiling and more capability, but thread counts stay modest
/// — even 8 cores here are weak A55s sharing limited memory bandwidth.
fn generous() -> Preferences {
    let mut p = Preferences::default();

    p.thread_pool_workers_max = 6;
    p.thread_pool_webrender_workers_max = 2; // bandwidth-bound; 2 is plenty
    p.thread_pool_async_runtime_workers_max = 3;
    p.thread_pool_fallback_workers = 2;
    p.layout_threads = 3;

    p.js_mem_max = -1; // unlimited
    p.js_mem_gc_high_frequency_high_limit_mb = 500;
    p.js_mem_gc_high_frequency_low_limit_mb = 100;
    p.js_mem_gc_high_frequency_heap_growth_max = 300; // %
    p.js_mem_gc_high_frequency_heap_growth_min = 150; // %
    p.js_mem_gc_low_frequency_heap_growth = 150; // %
    p.js_mem_gc_empty_chunk_count_min = 1;
    p.js_mem_gc_compacting_enabled = true;
    p.js_mem_gc_incremental_enabled = true;
    p.js_mem_gc_incremental_slice_ms = 8; // ms — desktop default is 10
    // Defensive: pin current Servo defaults so a crate bump can't silently flip them.
    p.js_mem_gc_per_zone_enabled = false; // collect all zones together
    p.js_baseline_interpreter_enabled = true; // never drop JS to the slow path

    p.session_history_max_length = 20;

    // Only keep the exotic device APIs off; everything else default-on.
    p.dom_webxr_enabled = false;
    p.dom_bluetooth_enabled = false;

    p
}

/// Android phone/tablet (>3 GB, capable OoO cores, unified memory). Full JIT and
/// decent parallelism, but bounded for thermal/battery, and tuned to RETURN
/// memory promptly — Android's low-memory killer reclaims from apps that don't
/// shrink under `onTrimMemory`/`onLowMemory`.
fn android() -> Preferences {
    let mut p = Preferences::default();

    p.thread_pool_workers_max = 6;
    p.thread_pool_webrender_workers_max = 3;
    p.thread_pool_async_runtime_workers_max = 3;
    p.thread_pool_fallback_workers = 2;
    p.layout_threads = 4;

    // Higher ceiling than handheld, but keep empty chunks at 0 so freed memory
    // goes back to the OS quickly (critical for the Android memory model).
    p.js_mem_max = 512; // MB
    p.js_mem_gc_high_frequency_high_limit_mb = 300;
    p.js_mem_gc_high_frequency_low_limit_mb = 100;
    p.js_mem_gc_high_frequency_heap_growth_max = 250; // %
    p.js_mem_gc_high_frequency_heap_growth_min = 130; // %
    p.js_mem_gc_low_frequency_heap_growth = 130; // %
    p.js_mem_gc_empty_chunk_count_min = 0; // hand memory back for onTrimMemory
    p.js_mem_gc_compacting_enabled = true;
    p.js_mem_gc_incremental_enabled = true;
    p.js_mem_gc_incremental_slice_ms = 5; // ms — OoO cores, but stay battery-friendly
    // Defensive: pin current Servo defaults so a crate bump can't silently flip them.
    p.js_mem_gc_per_zone_enabled = false; // collect all zones together
    p.js_baseline_interpreter_enabled = true; // never drop JS to the slow path

    p.session_history_max_length = 10;

    // High-DPI panels: grayscale AA is indistinguishable from subpixel and
    // cheaper on the glyph cache.
    p.gfx_subpixel_text_antialiasing_enabled = false;

    // Keep WebGL2 + service workers (PWAs are common on Android). Leave the
    // experimental/heavy and exotic-device APIs off.
    p.dom_webgpu_enabled = false;
    p.dom_webxr_enabled = false;
    p.dom_bluetooth_enabled = false;

    p
}

/// Desktop / unconstrained: Servo's own defaults, untouched. No pref overrides,
/// and [`crate::browser::build_preferences`] skips the thread clamp for this
/// tier, so the engine runs exactly as upstream ships it (unlimited JS heap,
/// auto-scaled thread pools, subpixel AA on, WebGL2/service workers default-on).
fn desktop() -> Preferences {
    Preferences::default()
}
