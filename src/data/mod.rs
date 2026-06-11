//! User data stores, all shaped alike: an in-memory list with a highlighted row
//! for the menu, persisted as TOML in the user data dir (see
//! [`crate::config::data_dir`]). [`crate::overlay::menu`] owns one of each; [`crate::ui`]
//! renders them.

pub mod bookmarks;
pub mod downloads;
pub mod history;
