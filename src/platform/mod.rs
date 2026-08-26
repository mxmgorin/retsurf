//! The platform layer under everything else: the SDL2 [`window`] with its GL
//! context, the surfman/Servo rendering-context glue ([`render`]), the embedded
//! resource provider Servo loads its support files from ([`resources`]), and the
//! allocator's own [`heap`].

pub mod heap;
pub mod render;
pub mod resources;
pub mod window;
