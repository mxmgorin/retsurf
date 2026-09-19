//! Game Mode's input machinery: the [`input_map`] entity (what each source
//! sends to the page), the [`map_library`] repository of this run's maps, and
//! the [`mode`] translator that applies the active one.

pub mod input_map;
pub mod map_library;
pub mod mode;
