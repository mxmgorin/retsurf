//! The platform layer under everything else: the SDL2 [`window`] with its GL
//! context, the surfman/Servo rendering-context glue ([`render`]), the embedded
//! resource provider Servo loads its support files from ([`resources`]), the
//! allocator's own [`heap`], and per-thread CPU accounting ([`threads`]).

pub mod heap;
pub mod render;
pub mod resources;
pub mod threads;
pub mod window;
