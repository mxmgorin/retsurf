//! Handing the allocator's free memory back to the kernel.

/// glibc keeps freed chunks reserved for reuse. After a heavy page is closed
/// that is hundreds of MB the browser is not using and the device cannot have —
/// on a 128 MB handheld it means the live working set gets swapped to the card
/// instead. A no-op where the allocator is not glibc's.
pub fn trim() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        malloc_trim(0);
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
extern "C" {
    fn malloc_trim(pad: usize) -> std::os::raw::c_int;
}
