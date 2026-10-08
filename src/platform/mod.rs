//! The platform layer under everything else: the SDL2 [`window`] with its GL
//! context, the text [`clipboard`], the surfman/Servo rendering-context glue ([`render`]), the embedded
//! resource provider Servo loads its support files from ([`resources`]), the
//! allocator's own [`heap`], the system's free memory ([`memory_guard`]), the CPU governor ([`cpufreq`]), swap tuning ([`swap_guard`]), and per-thread cost
//! accounting ([`threads`]). On Android, the glue SDL has no API for ([`android`]).

#[cfg(target_os = "android")]
pub mod android;
pub mod clipboard;
pub mod cpufreq;
pub mod heap;
pub mod memory_guard;
pub mod priority;
pub mod render;
pub mod resources;
pub mod startup;
#[cfg(all(target_os = "linux", not(target_os = "android")))]
pub mod swap_guard;
pub mod thread_cpu;
pub mod window;
