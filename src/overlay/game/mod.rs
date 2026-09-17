//! State machines of Game Mode's own overlays: its [`menu`], the input-map
//! screens ([`input_maps`]) and the map editor ([`map_edit`]). The matching
//! egui renderers live in [`crate::ui`]'s `game` submodule.

pub mod input_maps;
pub mod map_edit;
pub mod menu;
