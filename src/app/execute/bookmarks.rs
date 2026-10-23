//! Bookmarks in the app: opening one the way it asks, its options (the rows
//! Quick Access leads with over the start page, see
//! [`crate::overlay::quick_access`]), and the setting overrides of the one
//! whose site the active tab is on.
//!
//! A launched bookmark is tab state ([`crate::browser::AppBrowser::launched`])
//! and is that tab's site wherever it navigates; any other tab takes the
//! bookmark its URL matches ([`bookmarks::Bookmarks::for_url`]). The site's
//! overrides live in the config while it applies, with the global values of
//! [`overlay::SITE_KEYS`] kept aside: [`App::save_config`] then writes their
//! difference to the bookmark and the global values to `config.toml`.
//!
//! A load of ours switches the site before it starts; a page's own navigation
//! switches it once its URL commits, reloading the tab when the user agent or
//! forced dark changed under it; a tab switch switches it without a reload.

use super::super::{App, AppCommand};
use crate::config::{js_preset, overlay, HomeSort};
use crate::data::bookmarks::{self, Launch};
use crate::overlay::osk::OskCommand;
use crate::overlay::quick_access::{Entry, HomeTile, OptionsRow, OptionsText};
use crate::ui::{bookmark_label, Notice};
use egui_phosphor::bold;

/// What names a bookmark's overrides in a log line.
const SITE_PATH: &str = "site";

/// What the site sync last looked at: the shown tab, the bookmark it runs, the
/// URL it is on and how many bookmarks there were.
#[derive(PartialEq)]
pub(in crate::app) struct SiteSeen {
    tab: usize,
    launched: Option<String>,
    url: String,
    bookmarks: usize,
}

impl App {
    /// Open the bookmark at `at` as it asks: navigate, or run it as an app or
    /// a game. A game enters Game Mode before the page loads, which sizes its
    /// canvas once.
    pub(in crate::app) fn open_bookmark(&mut self, at: usize, out: &mut Vec<AppCommand>) {
        let Some(url) = self.start_bookmark(at, out) else {
            return;
        };
        // A runtime of an earlier visit would keep its old preset.
        let fresh = self.browser.launched().is_some()
            && self.config.performance.js_preset != js_preset::AUTO;
        match fresh && self.browser.reopen_active(&url) {
            true => {
                self.browser.set_active_zoom(self.config.browser.page_zoom);
                self.ui.menu.close();
            }
            false => self.open_url(url, out),
        }
        self.arm_bookmark_reader(at);
    }

    /// Reopen the shown page in a fresh webview, so it runs the JS preset of
    /// now.
    pub(super) fn reopen_page(&mut self) {
        let url = self.browser.state().site_url().to_string();
        if !self.browser.reopen_active(&url) {
            return;
        }
        self.browser.set_active_zoom(self.config.browser.page_zoom);
        let label = js_preset::label(&self.config.performance.js_preset);
        self.ui.toast(Notice::new(
            bold::LIGHTNING,
            format!("Reopened with {label} JS"),
        ));
    }

    /// Set the active tab up to run the bookmark at `at`, short of loading it;
    /// returns its URL to load.
    pub(in crate::app) fn start_bookmark(
        &mut self,
        at: usize,
        out: &mut Vec<AppCommand>,
    ) -> Option<String> {
        let bookmark = self.ui.menu.bookmarks.all().get(at)?;
        let (id, url, launch) = (bookmark.id.clone(), bookmark.url.clone(), bookmark.launch);
        self.ui.menu.bookmarks.opened(&url);
        match launch {
            // A site takes the tab back, chrome and all.
            Launch::Browser => self.end_launch(),
            Launch::App | Launch::Game => self.browser.set_launched(Some(id)),
        }
        self.switch_site_for(&url, out);
        if launch == Launch::Game && !self.browser.in_game_mode() {
            self.enter_game_mode(out);
        }
        Some(url)
    }

    /// Arm reader view on the active tab's load, when the bookmark at `at`
    /// opens in it.
    pub(in crate::app) fn arm_bookmark_reader(&mut self, at: usize) {
        let Some(bookmark) = self.ui.menu.bookmarks.all().get(at) else {
            return;
        };
        if bookmark.reader && bookmark.launch != Launch::Game {
            self.browser.arm_reader_on_load();
        }
    }

    /// The bookmark whose settings apply to the active tab at `url`: the one
    /// it runs, else the one `url`'s site matches.
    fn site_for(&self, url: &str) -> Option<String> {
        self.browser
            .launched()
            .or_else(|| self.ui.menu.bookmarks.for_url(url).map(|b| b.id.clone()))
    }

    /// Switch to the site of `url` ahead of loading it in the active tab.
    pub(in crate::app) fn switch_site_for(&mut self, url: &str, out: &mut Vec<AppCommand>) {
        let site = self.site_for(url);
        self.switch_site(site, false, out);
    }

    /// Follow the active tab's site once per frame: a new tab switches without
    /// a reload, a navigation of the same tab with one where needed.
    pub(in crate::app) fn sync_site(&mut self, out: &mut Vec<AppCommand>) {
        let seen = SiteSeen {
            tab: self.browser.active_tab(),
            launched: self.browser.launched(),
            url: self.browser.state().site_url().to_string(),
            bookmarks: self.ui.menu.bookmarks.all().len(),
        };
        if self.site_seen.as_ref() == Some(&seen) {
            return;
        }
        let same_tab = self.site_seen.as_ref().is_some_and(|s| s.tab == seen.tab);
        let site = self.site_for(&seen.url);
        self.site_seen = Some(seen);
        self.switch_site(site, same_tab, out);
    }

    /// Hold `site`'s overrides, when that changed: the global values back
    /// first, then the new overrides on them, then the result pushed live.
    /// `reload`: reload the shown tab if its page no longer matches.
    pub(in crate::app) fn switch_site(
        &mut self,
        site: Option<String>,
        reload: bool,
        out: &mut Vec<AppCommand>,
    ) {
        if site == self.applied_site {
            return;
        }
        // Unwritten changes belong to the site being left.
        if self.config_dirty {
            self.save_config();
        }
        let zoom = self.config.browser.page_zoom;
        if let Some(base) = self.site_base.take() {
            self.config = overlay::apply(&self.config, &base, SITE_PATH);
        }
        let over = site
            .as_deref()
            .and_then(|id| self.ui.menu.bookmarks.get(id))
            .map(|b| b.settings.clone());
        if let Some(over) = over {
            self.site_base = Some(overlay::site_part(&self.config));
            self.config = overlay::apply(&self.config, &over, SITE_PATH);
            self.config.sanitize();
        }
        self.applied_site = site;
        self.browser.adopt_site(&self.config, reload);
        if self.config.browser.page_zoom != zoom {
            self.browser.set_active_zoom(self.config.browser.page_zoom);
        }
        self.apply_live(out);
    }

    /// Drop the overrides of the site shown, back to the global values.
    pub(in crate::app) fn reset_site(&mut self, out: &mut Vec<AppCommand>) {
        let Some(id) = self.applied_site.clone() else {
            return;
        };
        self.ui.menu.bookmarks.set_settings(&id, toml::Table::new());
        self.applied_site = None;
        self.switch_site(Some(id), true, out);
        self.ui.toast(Notice::new(
            bold::ARROW_COUNTER_CLOCKWISE,
            "Site settings reset",
        ));
    }

    /// Persist the config. While a site's overrides stand in, the
    /// [`overlay::SITE_KEYS`] values that differ from the global ones go to its
    /// bookmark and `config.toml` keeps the global ones.
    pub(in crate::app) fn save_config(&mut self) {
        self.config_dirty = false;
        let (Some(base), Some(id)) = (&self.site_base, &self.applied_site) else {
            self.config.save();
            return;
        };
        let changes = overlay::changes(&overlay::site_part(&self.config), base);
        self.ui.menu.bookmarks.set_settings(id, changes);
        overlay::apply(&self.config, base, SITE_PATH).save();
    }

    /// Stop running the bookmark: out of Game Mode, the global `[game_mode]`
    /// back, and the tab on the start page, which frees the page.
    pub(super) fn exit_launch(&mut self, out: &mut Vec<AppCommand>) {
        self.ui.quick_access.close();
        self.end_launch();
        self.open_url(crate::browser::HOME_URL.to_string(), out);
    }

    /// Stop running the bookmark the tab runs, if any, where it is: out of
    /// Game Mode, its overrides kept until the tab's next site applies.
    pub(super) fn end_launch(&mut self) {
        if self.browser.launched().is_none() {
            return;
        }
        if self.browser.in_game_mode() {
            self.leave_game_mode();
        }
        self.browser.set_launched(None);
    }

    /// Open Quick Access on the start-page tile at `tile`, the grid's own
    /// index; `false` when the grid has no such tile.
    pub(in crate::app) fn open_options(&mut self, tile: usize) -> bool {
        let Some(at) = self.ui.home_bookmark_at(tile) else {
            return false;
        };
        self.ui.home.select_tile(tile);
        self.open_home_access(Some(at));
        true
    }

    /// Open Quick Access over the start page: the rows of the bookmark at
    /// `at`, when one is given, then the page's own, with their values.
    pub(in crate::app) fn open_home_access(&mut self, at: Option<usize>) {
        let tile = at.map(|at| self.home_tile(at));
        let launched = self.browser.launched().and(self.site_name());
        self.ui
            .quick_access
            .open_home(tile, launched.as_deref(), &self.config);
        self.refresh_options();
    }

    /// The shown name of the bookmark whose settings apply; empty for one
    /// since removed.
    pub(super) fn site_name(&self) -> Option<String> {
        let id = self.applied_site.as_deref()?;
        Some(
            self.ui
                .menu
                .bookmarks
                .get(id)
                .map_or_else(String::new, bookmark_label),
        )
    }

    /// The bookmark at `at` as the tile Quick Access leads with.
    fn home_tile(&self, at: usize) -> HomeTile {
        let bookmark = &self.ui.menu.bookmarks.all()[at];
        HomeTile {
            id: bookmark.id.clone(),
            title: bookmark_label(bookmark),
            site: site_of(&bookmark.url),
            manual: self.config.home.bookmarks.sort == HomeSort::Manual,
            game: bookmark.launch == Launch::Game,
        }
    }

    /// The index in the store of the bookmark the options are open on.
    fn options_bookmark(&self) -> Option<usize> {
        let id = self.ui.quick_access.bookmark_id()?;
        self.ui.menu.bookmarks.position(id)
    }

    /// Show the bookmark's values beside their rows.
    fn refresh_options(&mut self) {
        let Some(at) = self.options_bookmark() else {
            return;
        };
        let bookmark = &self.ui.menu.bookmarks.all()[at];
        let (tags, launch, reader) = (
            bookmark.tags.join(", "),
            bookmark.launch.label(),
            bookmark.reader,
        );
        let menu = &mut self.ui.quick_access;
        menu.set_value(Entry::Options(OptionsRow::Tags), tags);
        menu.set_value(Entry::Options(OptionsRow::LaunchAs), launch.to_string());
        menu.set_flag(Entry::Options(OptionsRow::Reader), reader);
    }

    /// Left/Right on a row of the bookmark's options that holds a value.
    pub(super) fn step_options_row(&mut self, row: OptionsRow, dx: i32) {
        let Some(at) = self.options_bookmark() else {
            return;
        };
        let bookmark = &self.ui.menu.bookmarks.all()[at];
        match row {
            OptionsRow::LaunchAs => {
                let next = bookmark.launch.step(dx);
                self.ui.menu.bookmarks.set_launch(at, next);
                // The reader row comes and goes with the game; the panel stays
                // on this bookmark even once the filter no longer shows it.
                self.focus_home_tile(at);
                self.open_home_access(Some(at));
                self.ui.quick_access.select_entry(Entry::Options(row));
            }
            OptionsRow::Reader => {
                let reader = !bookmark.reader;
                self.ui.menu.bookmarks.set_reader(at, reader);
                self.refresh_options();
            }
            OptionsRow::Open
            | OptionsRow::Rename
            | OptionsRow::Tags
            | OptionsRow::MoveLeft
            | OptionsRow::MoveRight
            | OptionsRow::Remove => {}
        }
    }

    /// A / Enter on a row of the bookmark's options.
    pub(super) fn options_row(&mut self, row: OptionsRow, out: &mut Vec<AppCommand>) {
        let Some(at) = self.options_bookmark() else {
            self.ui.quick_access.close();
            return;
        };
        let bookmark = &self.ui.menu.bookmarks.all()[at];
        match row {
            OptionsRow::Open => {
                self.ui.quick_access.close();
                self.open_bookmark(at, out);
            }
            OptionsRow::LaunchAs | OptionsRow::Reader => self.step_options_row(row, 1),
            OptionsRow::Rename => {
                let name = bookmark_label(bookmark);
                self.ask_options_text(OptionsText::Name, name, out);
            }
            OptionsRow::Tags => {
                let tags = bookmark.tags.join(", ");
                self.ask_options_text(OptionsText::Tags, tags, out);
            }
            OptionsRow::MoveLeft => self.move_bookmark(at, -1),
            OptionsRow::MoveRight => self.move_bookmark(at, 1),
            OptionsRow::Remove => {
                if bookmark.has_settings() && !self.ui.quick_access.confirm(Entry::Options(row)) {
                    return;
                }
                self.ui.quick_access.close();
                self.ui.menu.bookmarks.remove(at);
                self.ui
                    .toast(Notice::new(bold::BOOKMARK_SIMPLE, "Bookmark removed"));
            }
        }
    }

    /// Hand the keyboard `text` to edit for the bookmark's `what`.
    fn ask_options_text(&mut self, what: OptionsText, text: String, out: &mut Vec<AppCommand>) {
        self.ui.quick_access.start_naming(what, text);
        self.ui.osk(OskCommand::Show, &self.browser, out);
    }

    /// The keyboard submitted a name or the tags for the bookmark's options.
    pub(super) fn options_text(&mut self, text: &str) {
        let (Some(what), Some(at)) = (self.ui.quick_access.take_naming(), self.options_bookmark())
        else {
            return;
        };
        let store = &mut self.ui.menu.bookmarks;
        match what {
            OptionsText::Name => {
                store.rename(at, text.to_string());
                if let Some(at) = self.options_bookmark() {
                    let title = bookmark_label(&self.ui.menu.bookmarks.all()[at]);
                    self.ui.quick_access.set_title(title);
                }
            }
            OptionsText::Tags => store.set_tags(at, bookmarks::split_tags(text)),
        }
        self.refresh_options();
    }

    /// Put the grid's focus on the bookmark at `at`, where the view shows it.
    fn focus_home_tile(&mut self, at: usize) {
        if let Some(tile) = self.ui.home_visible().iter().position(|&i| i == at) {
            self.ui.home.select_tile(tile);
        }
    }

    /// Swap the bookmark at `at` with its neighbour `delta` tiles along the
    /// grid, the selection going with it.
    fn move_bookmark(&mut self, at: usize, delta: i32) {
        let visible = self.ui.home_visible();
        let Some(tile) = visible.iter().position(|&i| i == at) else {
            return;
        };
        let Some(target) = tile.checked_add_signed(delta as isize) else {
            return;
        };
        let Some(&other) = visible.get(target) else {
            return;
        };
        self.ui.menu.bookmarks.swap(at, other);
        self.ui.home.select_tile(target);
    }
}

/// Where `url` leads, in a word: its host, else the whole of it.
fn site_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_string))
        .unwrap_or_else(|| url.to_string())
}
