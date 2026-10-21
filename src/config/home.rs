use super::Brand;
use crate::config::token_enum::token_enum;
use serde::{Deserialize, Serialize};

/// The start page (`[home]`): where Home leads and which widgets it shows.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HomeConfig {
    /// What Home and a new tab open: the built-in start page or any URL.
    pub page: String,
    /// Show the toolbar on the start page.
    pub toolbar: bool,
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
    pub layout: HomeLayout,
    pub size: HomeSize,
}

impl Default for HomeConfig {
    fn default() -> Self {
        Self {
            page: crate::browser::HOME_URL.to_string(),
            toolbar: false,
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
            layout: HomeLayout::default(),
            size: HomeSize::default(),
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

pub use layout::HomeLayout;
pub use size::HomeSize;

// Own modules: a file holds one `token_enum!`, whose tests take a fixed name.
mod layout {
    use crate::config::token_enum::token_enum;

    token_enum! {
        /// How the start page draws its bookmarks.
        pub enum HomeLayout {
            default Grid;
            /// Icon tiles with a label.
            Grid => "grid", "Grid",
            /// One row per bookmark: its name, site and tags.
            List => "list", "List",
        }
    }
}

mod size {
    use crate::config::token_enum::token_enum;

    token_enum! {
        /// The size of the start page's tiles and rows.
        pub enum HomeSize {
            default Medium;
            Small => "small", "Small",
            Medium => "medium", "Medium",
            Large => "large", "Large",
        }
    }
}
