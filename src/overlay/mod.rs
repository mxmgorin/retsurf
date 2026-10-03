//! State machines of the full-screen / modal overlays: the [`menu`], the
//! [`quick_access`] panel, Game Mode's own screens ([`game`]), the on-screen
//! keyboard ([`osk`]), link-hint navigation ([`hints`]), the modal page prompts
//! ([`prompt`]) and the startup [`update_notice`]. They hold state and input
//! handling only — the matching egui renderers live in [`crate::ui`]'s
//! submodules, and the central router ([`crate::app`]) decides which overlay
//! owns the input.

pub mod dial_edit;
pub mod game;
pub mod grid;
pub mod hints;
pub mod home;
pub mod menu;
pub mod osk;
pub mod prompt;
pub mod quick_access;
pub mod settings;
pub mod update_notice;
