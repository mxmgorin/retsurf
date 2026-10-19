use super::Brand;
use crate::config::token_enum::token_enum;
use serde::{Deserialize, Serialize};

/// The start page (`[home]`): where Home leads and which widgets it shows.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HomeConfig {
    /// What Home and a new tab open: the built-in start page or any URL.
    pub page: String,
    /// The brand mark at the top.
    pub brand: Brand,
    /// The search and address field.
    pub search: bool,
    /// The bookmark grid (`[home.bookmarks]`).
    pub bookmarks: HomeBookmarksConfig,
}

/// Which bookmarks the start page's grid shows, and in what order.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HomeBookmarksConfig {
    /// `all`, `sites`, `apps`, `games` or a tag (see
    /// [`crate::data::bookmarks::Filter`]).
    pub filter: String,
    pub sort: HomeSort,
}

impl Default for HomeConfig {
    fn default() -> Self {
        Self {
            page: crate::browser::HOME_URL.to_string(),
            brand: Brand::default(),
            search: true,
            bookmarks: HomeBookmarksConfig::default(),
        }
    }
}

impl Default for HomeBookmarksConfig {
    fn default() -> Self {
        Self {
            filter: crate::data::bookmarks::Filter::All.token(),
            sort: HomeSort::default(),
        }
    }
}

token_enum! {
    /// The order of the start page's bookmarks.
    pub enum HomeSort {
        default Manual;
        /// The bookmarks' own order.
        Manual => "manual", "Manual",
        /// Last opened first.
        Recent => "recent", "Recent",
        Name => "name", "Name",
    }
}
