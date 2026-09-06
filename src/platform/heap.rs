//! Keeping the allocator's idle memory out of the process.

use crate::config::MemoryProfile;

/// Trade allocator throughput for a smaller process on every tier but desktop
/// (measured: glibc holds ~150 MB on an image-heavy run regardless of tier).
/// Must run before the first large allocation; `RETSURF_HEAP_TUNE=0|1` overrides.
pub fn tune(profile: MemoryProfile) {
    let tier_wants = !matches!(profile, MemoryProfile::Desktop);
    let on = crate::config::env_flag("RETSURF_HEAP_TUNE").unwrap_or(tier_wants);
    if on {
        glibc::tune();
    }
}

/// Return freed chunks glibc keeps reserved: hundreds of MB after a heavy page
/// closes, which on a 128 MB handheld swaps the live working set to the card.
pub fn trim() {
    glibc::trim();
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod glibc {
    use std::os::raw::c_int;

    /// `mallopt` parameters, from `malloc.h`.
    const M_TRIM_THRESHOLD: c_int = -1;
    const M_MMAP_THRESHOLD: c_int = -3;
    const M_ARENA_MAX: c_int = -8;

    /// Blocks this size and up come from `mmap`, which frees straight back to the
    /// kernel. Pinning it also stops glibc raising it to 32 MB on its own, after
    /// which every multi-MB buffer would be heap the process keeps for good.
    const MMAP_THRESHOLD: c_int = 256 * 1024;

    /// Free space at the top of the heap over this is returned on `free`.
    const TRIM_THRESHOLD: c_int = 256 * 1024;

    /// Servo runs 27 threads, and glibc would give each its own arena to fragment.
    const ARENA_MAX: c_int = 2;

    pub fn tune() {
        unsafe {
            mallopt(M_MMAP_THRESHOLD, MMAP_THRESHOLD);
            mallopt(M_TRIM_THRESHOLD, TRIM_THRESHOLD);
            mallopt(M_ARENA_MAX, ARENA_MAX);
        }
        log::info!("heap: mmap threshold {MMAP_THRESHOLD} B, {ARENA_MAX} arenas max");
    }

    pub fn trim() {
        unsafe {
            malloc_trim(0);
        }
    }

    extern "C" {
        fn malloc_trim(pad: usize) -> c_int;
        fn mallopt(param: c_int, value: c_int) -> c_int;
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
mod glibc {
    pub fn tune() {}

    pub fn trim() {}
}
