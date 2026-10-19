//! Bookmarks in the app: opening one the way it asks, its options (the options
//! strip of [`crate::overlay::quick_access`]), and the `[game_mode]` overrides
//! of the one the active tab runs.
//!
//! A launched bookmark is tab state ([`crate::browser::AppBrowser::launched`]).
//! Its overrides live in the config while the active tab runs it, with the
//! global table kept aside: [`App::save_config`] then writes their difference
//! to the bookmark and the global table to `config.toml`.

use super::super::{App, AppCommand};
use crate::config::{overlay, HomeSort};
use crate::data::bookmarks::{self, Launch};
use crate::overlay::osk::OskCommand;
use crate::overlay::quick_access::{Entry, OptionsRow, OptionsText};
use crate::ui::{bookmark_label, Notice};
use egui_phosphor::bold;

/// The config section a bookmark's overrides are laid on.
const GAME_MODE_PATH: &str = "game_mode";

impl App {
    /// Open the bookmark at `at` as it asks: navigate, or run it as an app or
    /// a game. A game enters Game Mode before the page loads, which sizes its
    /// canvas once.
    pub(in crate::app) fn open_bookmark(&mut self, at: usize, out: &mut Vec<AppCommand>) {
        if let Some(url) = self.start_bookmark(at, out) {
            self.open_url(url);
            self.arm_bookmark_reader(at);
        }
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
            Launch::Browser => self.end_launch(out),
            Launch::App | Launch::Game => {
                self.browser.set_launched(Some(id));
                self.sync_launch(out);
            }
        }
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

    /// Hold the overrides of the bookmark the active tab runs, when that
    /// changed: the global `[game_mode]` back first, then the new overrides on
    /// it, then the result pushed live.
    pub(in crate::app) fn sync_launch(&mut self, out: &mut Vec<AppCommand>) {
        if self.browser.launched_is(self.applied_launch.as_deref()) {
            return;
        }
        let launched = self.browser.launched();
        if let Some(base) = self.game_base.take() {
            self.config.game_mode = base;
        }
        let over = launched
            .as_deref()
            .and_then(|id| self.ui.menu.bookmarks.get(id))
            .map(|b| b.game_mode.clone());
        if let Some(over) = over {
            let base = self.config.game_mode.clone();
            self.config.game_mode = overlay::apply(&base, &over, GAME_MODE_PATH);
            self.config.sanitize();
            self.game_base = Some(base);
        }
        self.applied_launch = launched;
        self.apply_live(out);
    }

    /// Persist the config. While a bookmark's overrides stand in, the
    /// `[game_mode]` keys that differ from the global table go to the bookmark
    /// and `config.toml` keeps the global table.
    pub(in crate::app) fn save_config(&mut self) {
        let (Some(base), Some(id)) = (&self.game_base, &self.applied_launch) else {
            self.config.save();
            return;
        };
        let changes = overlay::changes(&self.config.game_mode, base);
        self.ui.menu.bookmarks.set_game_mode(id, changes);
        let mut global = self.config.clone();
        global.game_mode = base.clone();
        global.save();
    }

    /// Stop running the bookmark: out of Game Mode, the global `[game_mode]`
    /// back, and the tab on the start page, which frees the page.
    pub(super) fn exit_launch(&mut self, out: &mut Vec<AppCommand>) {
        self.ui.quick_access.close();
        self.end_launch(out);
        self.open_url(crate::browser::HOME_URL.to_string());
    }

    /// Stop running the bookmark the tab runs, if any, where it is: out of
    /// Game Mode, the global `[game_mode]` back.
    pub(super) fn end_launch(&mut self, out: &mut Vec<AppCommand>) {
        if self.browser.launched().is_none() {
            return;
        }
        if self.browser.in_game_mode() {
            self.leave_game_mode();
        }
        self.browser.set_launched(None);
        self.sync_launch(out);
    }

    /// Open the options of the start-page tile at `tile`, the grid's own index.
    pub(in crate::app) fn open_options(&mut self, tile: usize) {
        let Some(&at) = self.ui.home_visible().get(tile) else {
            return;
        };
        self.ui.home.select_tile(tile);
        self.show_options(at);
    }

    /// Show the options of the bookmark at `at`: rows that fit how it opens and the
    /// order the grid is in, with its values.
    fn show_options(&mut self, at: usize) {
        let bookmark = &self.ui.menu.bookmarks.all()[at];
        let (id, title) = (bookmark.id.clone(), bookmark_label(bookmark));
        let manual = self.config.home.bookmarks.sort == HomeSort::Manual;
        let game = bookmark.launch == Launch::Game;
        self.ui.quick_access.open_options(id, title, manual, game);
        self.refresh_options();
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
            launch_label(bookmark.launch),
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
                let choices = Launch::CHOICES;
                let now = choices
                    .iter()
                    .position(|(_, token)| *token == bookmark.launch.as_str())
                    .unwrap_or(0);
                let next = Launch::from_value(choices[crate::list::wrap(now, dx, choices.len())].1);
                self.ui.menu.bookmarks.set_launch(at, next);
                // The reader row comes and goes with the game.
                self.show_options(at);
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

/// How a [`Launch`] reads in the menu.
fn launch_label(launch: Launch) -> &'static str {
    Launch::CHOICES
        .iter()
        .find(|(_, token)| *token == launch.as_str())
        .map_or("", |(label, _)| label)
}
