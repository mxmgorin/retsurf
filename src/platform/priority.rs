//! Main-thread priority: an opt-in renice, since Servo's threads inherit
//! their creator's nice.

/// Renice the main thread to `RETSURF_MAIN_NICE` (unset or `0` = leave alone).
/// Off by default: measured on the Flip it earns nothing, because two cores at
/// 21% utilisation are not contended. A knob because it is worth -38% when they are.
pub fn prioritize_main() {
    let Some(nice) = std::env::var("RETSURF_MAIN_NICE")
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|n| *n != 0)
    else {
        return;
    };
    match imp::set_nice(nice) {
        Ok(()) => log::info!("main thread nice {nice}"),
        // Wants CAP_SYS_NICE or root; declining costs only the priority.
        Err(e) => log::info!("main thread nice {nice} declined ({e})"),
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::io;
    use std::os::raw::c_int;

    /// `PRIO_PROCESS` from `sys/resource.h`. Linux keeps nice per task, so
    /// `who = 0` moves the calling thread alone.
    const PRIO_PROCESS: c_int = 0;
    const CALLING_THREAD: u32 = 0;

    pub fn set_nice(nice: i32) -> io::Result<()> {
        // Unlike `getpriority`, -1 here is only ever the error return.
        match unsafe { setpriority(PRIO_PROCESS, CALLING_THREAD, nice) } {
            0 => Ok(()),
            _ => Err(io::Error::last_os_error()),
        }
    }

    extern "C" {
        fn setpriority(which: c_int, who: u32, prio: c_int) -> c_int;
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use std::io;

    pub fn set_nice(_: i32) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "no thread priority on this platform",
        ))
    }
}
