//! A bookmark's options: the controller over the bookmark strip of
//! [`crate::overlay::quick_access`] and [`crate::data::bookmarks`].

use super::super::{App, AppCommand};
use crate::config::HomeSort;
use crate::data::bookmarks;
use crate::overlay::osk::OskCommand;
use crate::overlay::quick_access::{Entry, OptionsRow, OptionsText};
use crate::ui::{bookmark_label, Notice};
use egui_phosphor::bold;

impl App {
    /// Open the options of the start-page tile at `tile`, the grid's own index.
    pub(in crate::app) fn open_options(&mut self, tile: usize) {
        let Some(&at) = self.ui.home_visible().get(tile) else {
            return;
        };
        let bookmark = &self.ui.menu.bookmarks.all()[at];
        let (id, title) = (bookmark.id.clone(), bookmark_label(bookmark));
        let manual = self.config.home.bookmarks.sort == HomeSort::Manual;
        self.ui.home.select_tile(tile);
        let mode = self.browser.mode();
        self.ui
            .quick_access
            .open_options(id, title, manual, mode, &self.config);
        self.refresh_options();
    }

    /// The index in the store of the bookmark the options are open on.
    fn options_bookmark(&self) -> Option<usize> {
        let id = self.ui.quick_access.bookmark_id()?;
        self.ui.menu.bookmarks.position(id)
    }

    /// Show the bookmark's tags beside their row.
    fn refresh_options(&mut self) {
        let Some(at) = self.options_bookmark() else {
            return;
        };
        let tags = self.ui.menu.bookmarks.all()[at].tags.join(", ");
        self.ui
            .quick_access
            .set_value(Entry::Options(OptionsRow::Tags), tags);
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
                let url = bookmark.url.clone();
                self.ui.quick_access.close();
                self.ui.menu.bookmarks.opened(&url);
                self.open_url(url);
            }
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
