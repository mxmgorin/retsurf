//! Input: the SDL event pump and its handlers (keyboard, gamepad, touch),
//! the user-event wakes, and Game Mode's input machinery under [`game`].

pub mod bindings;
pub mod game;
pub mod gamepad;
pub mod gamepad_api;
pub mod handler;
pub mod keyboard;
pub(crate) mod sdl2_servo;
pub mod touch;
pub mod user;
pub mod window;
