//! Keeping the allocator's idle memory out of the process.

use crate::config::MemoryProfile;

/// Trade allocator throughput for a smaller process, on every tier but the
/// desktop one — what glibc holds is a function of the workload, not of how much
/// RAM the device has (measured: ~150 MB on an image-heavy run at both `tight`
/// and `balanced`). Must run before the first large allocation.
/// `RETSURF_HEAP_TUNE=0|1` overrides the tier, which is how the two are compared
/// in one sitting.
pub fn tune(profile: MemoryProfile) {
    let tier_wants = !matches!(profile, MemoryProfile::Desktop);
    let on = match std::env::var("RETSURF_HEAP_TUNE") {
        Ok(v) => v != "0",
        Err(_) => tier_wants,
    };
    if on {
        glibc::tune();
    }
}

/// glibc keeps freed chunks reserved for reuse. After a heavy page is closed
/// that is hundreds of MB the browser is not using and the device cannot have —
/// on a 128 MB handheld it means the live working set gets swapped to the card
/// instead.
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
