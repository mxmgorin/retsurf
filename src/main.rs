fn main() {
    #[cfg(all(
        feature = "llsc-atomics",
        target_arch = "aarch64",
        target_os = "linux",
        target_env = "gnu"
    ))]
    prefer_exclusive_atomics();
    retsurf::run_app();
}

/// Routes the outline-atomics helpers, which prebuilt std and C++ call, to LL/SC even
/// where the CPU has LSE. Must run before any other thread starts.
#[cfg(all(
    feature = "llsc-atomics",
    target_arch = "aarch64",
    target_os = "linux",
    target_env = "gnu"
))]
fn prefer_exclusive_atomics() {
    unsafe extern "C" {
        static mut __aarch64_have_lse_atomics: u8;
    }
    // SAFETY: single-threaded here, and both paths are valid atomics on the same memory.
    unsafe { std::ptr::write_volatile(&raw mut __aarch64_have_lse_atomics, 0) };
}
