//! The Android shared library: SDL's Java `SDLActivity` dlopens it and enters
//! through the C `SDL_main` symbol below. Everything else lives in the `retsurf`
//! crate this wraps.

/// Called on SDL's own thread once the activity is up.
#[no_mangle]
pub extern "C" fn SDL_main(
    _argc: std::os::raw::c_int,
    _argv: *const *const std::os::raw::c_char,
) -> std::os::raw::c_int {
    retsurf::run_app();
    0
}
