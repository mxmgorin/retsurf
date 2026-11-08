//! The user-origin stylesheets a `[browser] page_theme` attaches to the shared
//! `UserContentManager` in [`super`], so they land on the next load. `dark` holds
//! pages that declare no color scheme to light: only a site's own dark styles
//! change it. `forced-dark` inverts every page instead, which costs a full-page
//! compositor pass per frame — wants an on-device frame-time check before being
//! recommended on a handheld.

use crate::config::PageTheme;
use servo::user_contents::UserStyleSheet;
use servo::UserContentManager;
use std::rc::Rc;

/// The page theme's stylesheets, built once and attached or detached as the
/// theme changes.
pub(super) struct ThemeSheets {
    light_scheme: Rc<UserStyleSheet>,
    forced_dark: Rc<UserStyleSheet>,
}

impl ThemeSheets {
    pub(super) fn new() -> Self {
        Self {
            light_scheme: sheet(
                "retsurf://light-scheme.css",
                include_str!("assets/light_scheme.css"),
            ),
            forced_dark: sheet(
                "retsurf://forced-dark.css",
                include_str!("assets/forced_dark.css"),
            ),
        }
    }

    /// Each sheet with whether `theme` wants it. Exhaustive over the sheets, so
    /// a new one must state its theme here.
    fn wanted(&self, theme: PageTheme) -> [(&Rc<UserStyleSheet>, bool); 2] {
        [
            (&self.light_scheme, theme.prefers_dark()),
            (&self.forced_dark, theme.is_forced_dark()),
        ]
    }

    /// Attach the sheets `theme` wants to a manager that holds none of them.
    pub(super) fn attach(&self, user_content: &UserContentManager, theme: PageTheme) {
        for (sheet, wanted) in self.wanted(theme) {
            if wanted {
                user_content.add_stylesheet(sheet.clone());
            }
        }
    }

    /// Trade `old`'s sheets for `new`'s; returns whether any changed. A sheet
    /// both want stays put: `add_stylesheet` pushes, so attaching twice doubles it.
    pub(super) fn swap(
        &self,
        user_content: &UserContentManager,
        old: PageTheme,
        new: PageTheme,
    ) -> bool {
        let mut changed = false;
        for ((sheet, had), (_, wants)) in self.wanted(old).into_iter().zip(self.wanted(new)) {
            match (had, wants) {
                (false, true) => user_content.add_stylesheet(sheet.clone()),
                (true, false) => user_content.remove_stylesheet(sheet.clone()),
                _ => continue,
            }
            changed = true;
        }
        changed
    }
}

/// A sheet from `css`, named by `url`. Nothing is fetched; the URL only names
/// the source.
fn sheet(url: &str, css: &str) -> Rc<UserStyleSheet> {
    let url = ::url::Url::parse(url).expect("sheet URLs are valid literals");
    Rc::new(UserStyleSheet::new(css.to_string(), url))
}
