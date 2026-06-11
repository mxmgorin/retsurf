//! State machines of the full-screen / modal overlays: the [`menu`], the
//! on-screen keyboard ([`osk`]), link-hint navigation ([`hints`]), and the
//! modal page prompts ([`prompt`]). They hold state and input handling only —
//! the matching egui renderers live in [`crate::ui`]'s submodules, and the
//! central router ([`crate::app`]) decides which overlay owns the input.

pub mod hints;
pub mod menu;
pub mod osk;
pub mod prompt;
