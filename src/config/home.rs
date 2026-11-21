use super::Brand;
use crate::config::token_enum::token_enum;
use serde::{Deserialize, Serialize};

/// Home's settings (`[home]`): the home page and the widgets Home shows.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HomeConfig {
    /// What the Home button and a new tab open: Home or any URL.
    pub page: String,
    /// Show the toolbar on Home.
    pub toolbar: bool,
    /// The brand mark at the top.
    pub brand: Brand,
    /// Where the search and address field sits, if anywhere.
    pub search: HomeSearch,
    /// The bookmark grid (`[home.bookmarks]`).
    pub bookmarks: HomeBookmarksConfig,
}

/// Which bookmarks Home's grid shows, and in what order.
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
            search: HomeSearch::default(),
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
    /// The order of Home's bookmarks.
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
pub use search::HomeSearch;
pub use size::HomeSize;

// Own modules: a file holds one `token_enum!`, whose tests take a fixed name.
mod layout {
    use crate::config::token_enum::token_enum;

    token_enum! {
        /// How Home draws its bookmarks.
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
        /// The size of Home's tiles and rows.
        pub enum HomeSize {
            default Medium;
            Small => "small", "Small",
            Medium => "medium", "Medium",
            Large => "large", "Large",
        }
    }
}

mod search {
    use crate::config::token_enum::token_enum;

    token_enum! {
        /// Where Home's search field sits.
        pub enum HomeSearch {
            default Bottom;
            /// Under the brand, scrolling with the page.
            Top => "top", "Top",
            /// Pinned above the footer, the page scrolling above it.
            Bottom => "bottom", "Bottom",
            Off => "off", "Off",
        }
    }

    impl HomeSearch {
        pub fn shown(self) -> bool {
            self != Self::Off
        }
    }
}
