//! The composite path imports an `EGLImageKHR`, so it needs EGL. Where there is
//! none — or where the `webgl` feature is off — every front buffer goes straight
//! back, and Servo sees a rendering context with no surfman connection.

use euclid::default::Size2D;
use std::ffi::c_void;
use surfman::{Connection, Surface, SurfaceTexture};

/// Uninhabited: there is nothing here to construct.
pub enum FrontBuffers {}

impl FrontBuffers {
    pub fn new(_get_proc: impl Fn(&str) -> *const c_void) -> Option<Self> {
        log::info!("gl: no EGL composite path in this build; WebGL disabled");
        None
    }

    pub fn connection(&self) -> Connection {
        match *self {}
    }

    pub fn create_texture(
        &self,
        _surface: Surface,
    ) -> Result<(SurfaceTexture, u32, Size2D<i32>), Surface> {
        match *self {}
    }

    pub fn destroy_texture(&self, _surface_texture: SurfaceTexture) -> Option<Surface> {
        match *self {}
    }
}
