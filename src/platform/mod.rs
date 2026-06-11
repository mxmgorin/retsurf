//! The platform layer under everything else: the SDL2 [`window`] with its GL
//! context, the surfman/Servo rendering-context glue ([`render`]), and the
//! embedded resource provider Servo loads its support files from
//! ([`resources`]).

pub mod render;
pub mod resources;
pub mod window;
