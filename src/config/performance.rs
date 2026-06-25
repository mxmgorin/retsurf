use crate::config::token_enum::token_enum;
use serde::{Deserialize, Serialize};

/// Servo thread-count tuning (`[performance]` in the config). Servo's defaults
/// assume a desktop (3 layout threads, worker pools of 4–6); on a 4-core
/// handheld that oversubscribes the cores, with the pools competing against
/// layout, script, and WebRender itself. `0` everywhere (the default) sizes
/// them from the machine's core count instead.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PerformanceConfig {
    /// Memory/performance tier for the engine — JS heap ceilings, caches, and
    /// which DOM subsystems start (see [`MemoryProfile`]). `auto` (the default)
    /// picks one from the platform and detected RAM. The two thread knobs below
    /// override the tier's thread counts when set non-zero.
    pub memory_profile: MemoryProfile,
    /// Stylo/layout threads. `0` = keep the memory profile's choice; non-zero
    /// overrides it.
    pub layout_threads: u32,
    /// Cap on each of Servo's worker pools (image cache, async runtime,
    /// storage, WebRender workers). `0` = keep the memory profile's choice;
    /// non-zero overrides every pool with this value.
    pub worker_pool_max: u32,
}

token_enum! {
    /// Memory/performance tier for the Servo engine (`[performance] memory_profile`).
    /// Each tier bundles a coordinated set of engine prefs — JS GC ceilings,
    /// back-forward-cache depth, HTTP/canvas caches, thread counts, and which DOM
    /// subsystems are even started — tuned for a class of hardware. Lower tiers
    /// trade speed for a smaller footprint; `Auto` picks one from the build target
    /// and detected RAM. See [`crate::browser::memory`] for what each tier sets.
    /// Serializes to a lowercase token in TOML; an unknown value falls back to
    /// `Auto`, like [`CursorMode`].
    ///
    /// [`CursorMode`]: crate::config::CursorMode
    pub enum MemoryProfile {
        default Auto;
        /// Pick a tier from the build target and detected RAM (the default).
        Auto => "auto", "Auto",
        /// Tightest floor (~512 MB, sub-1 GB boards): baseline JIT only, single
        /// thread, minimal caches, foreground tab only.
        Embedded => "embedded", "Embedded",
        /// Very constrained (~1 GB boards): baseline JIT only, small caches.
        Tight => "tight", "Tight",
        /// Balanced handheld (~2 GB boards): modest parallelism, full JIT.
        Balanced => "balanced", "Balanced",
        /// Most headroom among handhelds (~4 GB): higher GC ceiling, modest threads.
        Generous => "generous", "Generous",
        /// Android phone/tablet (>3 GB): full JIT, more threads, eager memory return.
        Android => "android", "Android",
        /// Desktop / unconstrained: Servo's own defaults, untouched — no pref
        /// overrides and no thread clamp. The escape hatch when you want exactly
        /// what upstream ships.
        Desktop => "desktop", "Desktop (Servo defaults)",
    }
}
