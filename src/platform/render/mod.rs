//! Servo-facing rendering contexts: [`SdlRenderingContext`] over SDL2's GL/GLES
//! context, and — with the `software` feature — [`SwglRenderingContext`] over
//! swgl, for devices with no GPU at all.

mod sdl;
#[cfg(feature = "software")]
mod swgl;

pub use self::sdl::SdlRenderingContext;
#[cfg(feature = "software")]
pub use self::swgl::SwglRenderingContext;

/// Every buffer here is 32-bit colour: RGBA or BGRA, four bytes either way.
pub const BYTES_PER_PIXEL: usize = 4;
