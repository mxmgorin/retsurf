//! Forced dark mode: a user-origin stylesheet that inverts the page, for sites
//! that ship no dark theme (`[browser] page_theme = "forced-dark"`). Attached to
//! the shared `UserContentManager` in [`super`], so it lands on the next load.
//! Costs a full-page compositor pass per frame — wants an on-device frame-time
//! check before being recommended on a handheld.

use servo::user_contents::UserStyleSheet;
use std::rc::Rc;

/// Base URL for the sheet. Nothing is fetched; it only names the source.
const SHEET_URL: &str = "retsurf://forced-dark.css";

pub(super) fn stylesheet() -> Rc<UserStyleSheet> {
    let url = ::url::Url::parse(SHEET_URL).expect("SHEET_URL is a valid literal URL");
    Rc::new(UserStyleSheet::new(
        include_str!("forced_dark.css").to_string(),
        url,
    ))
}
