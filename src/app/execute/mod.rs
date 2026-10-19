//! Command dispatch: turning an [`AppCommand`] into effects on the browser, UI,
//! and config. The main loop ([`super::App::run`]) drains its command queue
//! through [`App::execute_command`]; the per-overlay action helpers it fans out
//! to (menu / settings / start page / bookmarks) live here too. Input intents are
//! mapped earlier, in [`super::router`].

mod game;

use super::{
    App, AppCommand, HomeAction, InputCommand, MenuAction, PromptAction, SettingsAction,
    UpdateNoticeAction,
};
use crate::browser::BrowserCommand;
use crate::config::{AppConfig, HomeSort};
use crate::data::bookmarks::Filter;
use crate::overlay::menu::Section;
use crate::overlay::osk::OskCommand;
use crate::overlay::settings::{Door, SettingsSection, Task};
use crate::overlay::update_notice::Choice;
use crate::ui::{Focus, Notice};
use egui_phosphor::bold;
use std::time::Instant;

impl App {
    pub(super) fn execute_command(&mut self, command: &AppCommand, out: &mut Vec<AppCommand>) {
        // Game Mode shrinks the vocabulary to its own, so a shortcut resolved
        // under one of its overlays cannot act on the browser behind it.
        let settings_up = self.ui.settings.visible() && command.keeps_the_page();
        if (self.browser.in_game_mode() || self.ui.game_screen())
            && !command.in_game_mode()
            && !settings_up
        {
            return;
        }
        match command {
            AppCommand::Shutdown => self.shutdown(),
            // On a window resize, size the browser to the new central area straight
            // away from the actual window (egui's reactive sizing can lag a frame).
            AppCommand::Resize => {
                self.sync_screen_geometry();
                self.ui.resize_browser(&self.window, &self.browser);
            }
            AppCommand::Browser(command) => self.browser.execute_command(command, &self.config),
            AppCommand::Input(command) => self.route_input(command, out),
            AppCommand::Menu(action) => self.menu_action(action),
            AppCommand::ToggleBookmark => self.toggle_current_bookmark(),
            AppCommand::FocusAddressBar => self.focus_address_bar(out),
            AppCommand::CloseTab => {
                self.close_tab_at(self.browser.active_tab());
                self.ui.toast(Notice::new(bold::X, "Tab closed"));
            }
            AppCommand::QuickAccess(action) => self.quick_access_action(action, out),
            AppCommand::GameInputMaps(action) => self.input_maps_action(action, out),
            AppCommand::GameMapEdit(action) => self.map_edit_action(action, out),
            AppCommand::Prompt(action) => match action {
                PromptAction::Activate => self.ui.prompt.activate(),
                PromptAction::Cancel => self.ui.prompt.cancel(),
                PromptAction::ClickSlot(index) => {
                    self.ui.prompt.set_selected(*index);
                    self.ui.prompt.activate();
                }
            },
            AppCommand::Settings(action) => self.settings_action(action, out),
            AppCommand::UpdateNotice(action) => self.update_notice_action(action, out),
            AppCommand::Home(action) => self.home_action(action),
        };

        // Commands drain after `ui.update` built the frame, so a UI change needs
        // a follow-up one. The analog tick's would spin the idle loop.
        if !matches!(
            command,
            AppCommand::Input(InputCommand::Analog { .. })
                | AppCommand::Resize
                | AppCommand::Shutdown
        ) {
            self.ui.request_repaint();
        }
    }

    /// Apply a menu action (Tabs / Bookmarks / History / Downloads overlay).
    fn menu_action(&mut self, action: &MenuAction) {
        match action {
            // The gesture toggles the menu; the menu button only ever opens it (it's
            // hidden behind the menu once shown).
            MenuAction::Open => {
                if self.ui.menu.visible {
                    self.ui.menu.close();
                } else {
                    self.ui.menu_open();
                }
            }
            MenuAction::Close => self.ui.menu.close(),
            MenuAction::SetSection(section) => self.ui.menu.set_section(*section),
            MenuAction::OpenSelected => self.menu_open_selected(),
            MenuAction::RemoveSelected => self.delete_menu_selection(),
            MenuAction::Clear => self.ui.menu.clear_or_arm(),
            MenuAction::OpenUrl(url) => self.open_url(url.clone()),
            MenuAction::ToggleBookmark(url) => self.toggle_bookmark(url),
            MenuAction::RemoveAt(index) => match self.ui.menu.section() {
                Section::Bookmarks => self.remove_bookmark(*index),
                _ => self.ui.menu.remove_at(*index),
            },
            MenuAction::OpenTab(index) => self.open_tab_at(*index),
            MenuAction::CloseTab(index) => self.close_tab_at(*index),
            MenuAction::NewTab => self.new_tab(),
        }
    }

    /// The browser tab the Tabs section's highlight points at, or `None` on the
    /// "+ New tab" row, which occupies index 0 ahead of the tabs.
    fn selected_tab_index(&self) -> Option<usize> {
        self.ui.menu.tab_selected().checked_sub(1)
    }

    /// Focus the tab at `index` and close the menu.
    fn open_tab_at(&mut self, index: usize) {
        self.browser.switch_to(index);
        self.ui.menu.close();
    }

    /// Close the tab at `index`, resync the menu's row count, and reclaim what
    /// the torn-down document frees.
    pub(super) fn close_tab_at(&mut self, index: usize) {
        self.browser.close_tab(index, &self.config.home.page);
        self.ui.menu.set_tab_count(self.browser.tab_count());
        self.schedule_heap_trim();
    }

    /// Open a new tab at the home page and close the menu.
    fn new_tab(&mut self) {
        let home = self.config.home.page.clone();
        self.browser.open_tab(&home);
        self.ui.menu.close();
    }

    /// Bookmark the current page, or remove its bookmark. The start page holds
    /// the bookmarks rather than being one.
    fn toggle_current_bookmark(&mut self) {
        let url = self.browser.state().page_url().to_string();
        if !url.is_empty() && url != crate::browser::HOME_URL {
            self.toggle_bookmark(&url);
        }
    }

    /// Bookmark `url` under its open tab's title, or remove its bookmark.
    fn toggle_bookmark(&mut self, url: &str) {
        let bookmarks = &self.ui.menu.bookmarks;
        if let Some(index) = bookmarks.all().iter().position(|b| b.url == url) {
            self.remove_bookmark(index);
            return;
        }
        let title = self
            .browser
            .tabs()
            .into_iter()
            .find(|tab| tab.url == url)
            .map(|tab| tab.title)
            .unwrap_or_default();
        let name = crate::data::bookmarks::name_from_title(&title, url);
        self.ui.menu.bookmarks.add(url, name);
        self.ui
            .toast(Notice::new(bold::BOOKMARK_SIMPLE, "Bookmarked"));
    }

    /// Remove the bookmark at `index`. One with settings or tags takes a second
    /// ask while the first one's notice shows.
    fn remove_bookmark(&mut self, index: usize) {
        let Some(bookmark) = self.ui.menu.bookmarks.all().get(index) else {
            return;
        };
        let confirmed = self
            .bookmark_remove_armed
            .take()
            .is_some_and(|(id, at)| id == bookmark.id && at.elapsed() < crate::ui::TOAST);
        if bookmark.has_settings() && !confirmed {
            self.bookmark_remove_armed = Some((bookmark.id.clone(), Instant::now()));
            self.ui.toast(Notice::new(
                bold::BOOKMARK_SIMPLE,
                "Press again to remove it and its settings",
            ));
            return;
        }
        self.ui.menu.bookmarks.remove(index);
        self.ui
            .toast(Notice::new(bold::BOOKMARK_SIMPLE, "Bookmark removed"));
    }

    /// Open the highlighted menu entry (the **A** button / Enter). In Tabs this
    /// switches to the tab (or opens a new one on the "+ New tab" row); in the URL
    /// lists it loads the entry. Closes the menu either way.
    pub(super) fn menu_open_selected(&mut self) {
        if self.ui.menu.section() == Section::Tabs {
            match self.selected_tab_index() {
                Some(index) => self.open_tab_at(index),
                None => self.new_tab(),
            }
        } else if self.ui.menu.clear_selected() {
            // The section's clear row (index 0): arms, then wipes; stays open.
            self.ui.menu.clear_or_arm();
        } else if let Some(url) = self.ui.menu.selected_url() {
            if self.ui.menu.section() == Section::Bookmarks {
                self.ui.menu.bookmarks.opened(&url);
            }
            self.open_url(url);
        } else if self.ui.menu.section() != Section::Downloads {
            // A on an active/failed download has nothing to open — keep the menu
            // up so the user can watch the progress; other sections close.
            self.ui.menu.close();
        }
    }

    /// Delete the highlighted menu entry (the **X** button / Delete). In Tabs this
    /// closes the tab; in the URL lists it removes the bookmark / history entry.
    pub(super) fn delete_menu_selection(&mut self) {
        match self.ui.menu.section() {
            Section::Tabs => {
                if let Some(index) = self.selected_tab_index() {
                    self.close_tab_at(index);
                }
            }
            Section::Bookmarks => {
                if let Some(index) = self.ui.menu.bookmarks.selected_index() {
                    self.remove_bookmark(index);
                }
            }
            Section::History | Section::Downloads => self.ui.menu.remove_selected(),
        }
    }

    /// Bookmark the entry selected in History or Tabs; the other sections have
    /// nothing to bookmark.
    pub(super) fn menu_y_action(&mut self) {
        match self.ui.menu.section() {
            Section::History => {
                if let Some(url) = self.ui.menu.selected_url() {
                    self.toggle_bookmark(&url);
                }
            }
            Section::Tabs => {
                if let Some(index) = self.selected_tab_index() {
                    if let Some(info) = self.browser.tabs().get(index) {
                        if !info.url.is_empty() {
                            self.toggle_bookmark(&info.url);
                        }
                    }
                }
            }
            Section::Bookmarks | Section::Downloads => {}
        }
    }

    /// Apply a settings-overlay action (see [`crate::overlay::settings`]).
    fn settings_action(&mut self, action: &SettingsAction, out: &mut Vec<AppCommand>) {
        match action {
            // Re-triggering the gesture while it is open closes it, like the menu.
            SettingsAction::Open => {
                if self.ui.settings.visible() {
                    self.settings_close(out);
                } else {
                    self.open_settings(None);
                }
            }
            SettingsAction::Close => self.settings_close(out),
            SettingsAction::SetSection(section) => self.ui.settings.set_section(*section),
            SettingsAction::Select(index) => self.ui.settings.set_selected(*index),
            SettingsAction::Activate => self.settings_confirm(out),
            SettingsAction::Adjust(dx) => self.ui.settings.adjust(*dx),
            // A link on the About tab: save & close like a normal exit, then load
            // it in the focused tab (open_url also tidies the menu, harmless here).
            SettingsAction::OpenLink(url) => {
                self.settings_close(out);
                self.open_url(url.clone());
            }
            // Binding capture (the binding list): the gesture performed, bound
            // to the listening action. The raw input comes from the event loop.
            SettingsAction::CaptureBinding { gesture, keyboard } => {
                self.ui.settings.apply_capture(gesture.clone(), *keyboard);
            }
            SettingsAction::CaptureCancel => self.ui.settings.cancel_capture(),
            // Self-update: a graceful shutdown, so the launcher's pm_finish runs
            // and re-execs the freshly swapped binary.
            SettingsAction::CheckUpdate => {
                // A channel edited in this visit is still only in the overlay draft
                // (`apply_config` runs on close), so adopt it before checking.
                if let Some(update) = self.ui.settings.pending_update().cloned() {
                    self.ui.update.set_config(&update);
                }
                self.ui.update.check(&self.event_sender);
            }
            SettingsAction::InstallUpdate => self.ui.update.install(&self.event_sender),
            SettingsAction::QuitForUpdate => {
                self.settings_close(out);
                self.shutdown();
            }
        }
    }

    /// A / Enter on the focused settings row, by its kind; a value steps forward
    /// (Left/Right does the rest).
    pub(super) fn settings_confirm(&mut self, out: &mut Vec<AppCommand>) {
        if self.ui.settings.is_info_section() {
            // About tab: A activates the focused row (update action or a link);
            // the resulting SettingsAction routes back through settings_action.
            if let Some(action) = self.ui.about_activate() {
                out.push(AppCommand::Settings(action));
            }
        } else if self.ui.settings.bindings_open() {
            self.ui.settings.controls_activate();
        } else if let Some(door) = self.ui.settings.open_door() {
            if door == Door::InputMaps {
                self.open_input_maps();
            }
        } else if let Some(task) = self.ui.settings.confirm_action() {
            match task {
                Task::ClearData => self.clear_browsing_data(),
                Task::RestoreDefaults => self.restore_defaults(),
                #[cfg(target_os = "android")]
                Task::ShareLogs => {
                    if !crate::platform::android::share_logs() {
                        self.ui
                            .toast(Notice::warn("Could not open the share sheet"));
                    }
                }
            }
        } else if self.ui.settings.selected_is_text() {
            self.ui.osk(OskCommand::Show, &self.browser, out);
        } else {
            self.ui.settings.adjust(1);
        }
    }

    /// Close the notice; Update goes on to the About tab, which holds the notes
    /// and the install row.
    fn update_notice_action(&mut self, action: &UpdateNoticeAction, out: &mut Vec<AppCommand>) {
        let choice = match action {
            UpdateNoticeAction::Activate => self.ui.update_notice.selected(),
            UpdateNoticeAction::Choose(choice) => *choice,
            UpdateNoticeAction::Cancel => Choice::Later,
        };
        self.ui.update_notice.close();
        if choice == Choice::Update {
            out.push(AppCommand::Settings(SettingsAction::Open));
            out.push(AppCommand::Settings(SettingsAction::SetSection(
                SettingsSection::About,
            )));
        }
    }

    /// Wipe the browsing data: history, the icons only it referenced, the
    /// finished downloads, the saved session and the open tabs, plus Servo's
    /// cookies, web storage and HTTP cache. Bookmarks and the settings stay.
    fn clear_browsing_data(&mut self) {
        self.ui.menu.history_mut().clear();
        crate::data::page_icons::prune(&self.ui.menu.icon_hosts(false));
        self.ui.menu.downloads.clear_finished();
        self.session.discard();
        self.browser.clear_site_data();
        self.browser.reset_tabs(&self.config.home.page);
        self.ui
            .toast(Notice::new(bold::CHECK, "Browsing data cleared"));
        log::info!("cleared browsing data");
    }

    /// Settings and bindings (overlay drafts, saved on close) back to how they
    /// ship; bookmarks, history and tabs stay.
    fn restore_defaults(&mut self) {
        self.ui.settings.restore_defaults();
        self.ui.toast(Notice::new(bold::CHECK, "Defaults restored"));
        log::info!("restored default settings and bindings");
    }

    /// Close the settings overlay: adopt its edited drafts
    /// — the config and the gamepad bindings, each saved and re-applied live.
    pub(super) fn settings_close(&mut self, out: &mut Vec<AppCommand>) {
        let Some((config, bindings)) = self.ui.settings_close() else {
            return;
        };
        self.apply_config(config, out);
        if let Some(store) = bindings {
            self.apply_bindings(store, out);
        }
    }

    /// Adopt edited bindings from the settings overlay: persist them, then
    /// rebuild both devices' tables in the running handler. Only called when the
    /// controls changed, so a config-only edit leaves `bindings.toml` alone.
    fn apply_bindings(&mut self, store: inputbind::Store, out: &mut Vec<AppCommand>) {
        crate::event::bindings::save(&store);
        // The pad drops what it holds, so a click still open closes here.
        self.event_handler.set_bindings(&store, out);
    }

    /// Adopt an edited config from the settings overlay: persist it, then
    /// re-apply the parts the running app can change without a restart. The rest
    /// take effect on the next launch, and their rows are flagged with `*`.
    fn apply_config(&mut self, config: AppConfig, out: &mut Vec<AppCommand>) {
        self.config = config;
        self.config.save();
        // Restoring the defaults can move the pad map under a live Game
        // Mode, so push it the same way the menu does.
        self.adopt_input_map(out);
        let view = &self.config.game_mode.view;
        let (scaling, texture_limit) = match self.browser.in_game_mode() {
            true => (view.scaling, view.texture_limit),
            false => (
                crate::config::Scaling::None,
                crate::config::TextureLimit::Full,
            ),
        };
        self.browser.set_game_scaling(scaling);
        self.browser
            .set_game_textures(texture_limit, view.texture_filter);
        self.browser.set_game_patches(self.config.game_mode.patches);
        // The router reads cursor/scroll speeds from the config each frame, but
        // the gamepad state machine and the UI cache a few values to push in.
        self.event_handler
            .set_gamepad_config(self.config.controls.clone());
        self.ui
            .set_cursor_linger(self.config.interface.cursor_linger_ms);
        self.ui.set_ui_scale(self.config.interface.scale);
        self.ui
            .set_toolbar_position(self.config.interface.toolbar_position);
        self.ui.set_shader(&self.config.game_mode.view.shader);
        self.ui
            .set_shader_resolution(self.config.game_mode.view.shader_resolution);
        self.ui.set_game_cursor(self.config.game_mode.view.cursor);
        self.ui
            .set_toolbar_autohide(self.config.interface.toolbar_autohide);
        self.ui.set_hint_badges(self.config.controls.hint_badges);
        self.ui.set_page_icons(self.config.interface.page_icons);
        self.ui.set_home(&self.config.home);
        self.ui.set_osk_style(self.config.osk.style);
        self.ui.set_pad_layout(self.config.controls.pad_layout);
        self.ui.menu.history_mut().set_config(&self.config.history);
        self.ui.set_memory_debug(
            self.config.debug.memory_overlay,
            self.config.debug.memory_log,
        );
        self.ui.update.set_config(&self.config.update);
        self.browser.apply_config(&self.config);
        self.ui.menu.downloads.user_agent =
            crate::browser::effective_user_agent(&self.config.browser);
        // Off drops the stored session now, not on the next launch.
        if !self.config.browser.restore_tabs {
            self.session.discard();
        }
        self.cpu_boost
            .set_enabled(self.config.performance.cpu_boost_on_load);
        // The frame cap takes effect on the very next frame, which is what makes
        // it worth tuning by hand on a device.
        self.window.set_max_fps(self.config.performance.max_fps);
    }

    /// Put the caret where an address is typed, over the page or the start page
    /// only; the start page's search field stands in for the address bar there.
    fn focus_address_bar(&mut self, out: &mut Vec<AppCommand>) {
        match self.ui.focus() {
            Focus::Home if self.config.home.search => self.ui.home.focus_search(),
            Focus::Home | Focus::Page | Focus::Hints => {
                self.ui.hints.hide();
                self.ui.focus_address_bar();
            }
            _ => return,
        }
        if !self.ui.last_input_keyboard() {
            self.ui.osk(OskCommand::Show, &self.browser, out);
        }
    }

    /// Open the start page's focused bookmark, or with its search field
    /// focused, the OSK to type into it.
    pub(super) fn home_confirm(&mut self, out: &mut Vec<AppCommand>) {
        match self.ui.home_selected_url() {
            Some(url) => {
                self.ui.menu.bookmarks.opened(&url);
                self.open_url(url);
            }
            None if self.ui.home.search_focused() => {
                self.ui.osk(OskCommand::Show, &self.browser, out)
            }
            None => {}
        }
    }

    /// Change the start page's filter or sort, and save it.
    pub(super) fn home_action(&mut self, action: &HomeAction) {
        let view = &mut self.config.home.bookmarks;
        match action {
            HomeAction::StepFilter(delta) => {
                let offered = self.ui.home_filters();
                let current = Filter::parse(&view.filter);
                let at = offered.iter().position(|f| *f == current).unwrap_or(0);
                view.filter = offered[crate::list::wrap(at, *delta, offered.len())].token();
            }
            HomeAction::SetFilter(token) => view.filter = token.clone(),
            HomeAction::StepSort(delta) => {
                let sorts = HomeSort::CHOICES;
                let at = sorts
                    .iter()
                    .position(|(_, token)| *token == view.sort.as_str())
                    .unwrap_or(0);
                view.sort =
                    HomeSort::from_value(sorts[crate::list::wrap(at, *delta, sorts.len())].1);
            }
        }
        self.config.save();
        self.ui.set_home(&self.config.home);
    }

    /// Load `url` in the focused tab and close the menu.
    fn open_url(&mut self, url: String) {
        self.browser.get_state_mut().location = url;
        self.browser
            .execute_command(&BrowserCommand::Load, &self.config);
        self.ui.menu.close();
    }
}
