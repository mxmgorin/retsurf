//! File downloads. Servo 0.2 has no download support in its embedding API (no
//! delegate hook, no `Content-Disposition` handling), so retsurf does it itself:
//! the browser denies navigations to file-like URLs (see [`crate::browser`]) and
//! the main loop hands them here. Fetching runs on background threads (see
//! [`worker`]); finished entries persist to `downloads.toml` (see [`store`]),
//! active ones live only in memory (they don't survive a restart). This module
//! owns the entry list and the highlighted row in the menu's Downloads section
//! (see [`crate::menu`]); [`crate::ui`] renders it.

mod store;
mod worker;

use crate::config::DownloadsConfig;
use crate::event::user::UserEventSender;
use crate::history;
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// Lifecycle of one download.
pub enum State {
    /// A worker thread is still fetching the file.
    Active,
    Done,
    /// The fetch failed or was cancelled; the partial file was removed.
    Failed(String),
}

pub struct Download {
    pub url: String,
    /// Final file name (the last component of `path`), shown in the menu.
    pub filename: String,
    /// Destination path; the worker writes to `<path>.part` until done.
    pub path: String,
    pub received: u64,
    /// Total size from Content-Length, `0` while/if unknown.
    pub total: u64,
    /// When the download finished (unix seconds), `0` while active.
    pub time: u64,
    pub state: State,
    /// Progress shared with the worker thread; dropped once it finishes.
    shared: Option<Arc<worker::Shared>>,
}

impl Download {
    pub fn is_active(&self) -> bool {
        matches!(self.state, State::Active)
    }

    /// One-line status for the menu row: progress while active, size + date when
    /// done, the error otherwise.
    pub fn status_text(&self) -> String {
        match &self.state {
            State::Active if self.total > 0 => format!(
                "{}% · {} / {}",
                self.received * 100 / self.total,
                format_size(self.received),
                format_size(self.total),
            ),
            State::Active => format_size(self.received),
            State::Done => format!(
                "{} · {}",
                format_size(self.received),
                history::format_time(self.time)
            ),
            State::Failed(e) => format!("✖ {e}"),
        }
    }
}

pub struct Downloads {
    /// Most-recent first; active entries are always from this session.
    items: Vec<Download>,
    /// Save directory, with a trailing separator (see [`DownloadsConfig`]).
    dir: String,
    /// Highlighted row in the menu's Downloads section.
    selected: usize,
}

impl Downloads {
    /// Load the saved list (missing/invalid file → empty).
    pub fn load(cfg: &DownloadsConfig) -> Self {
        Self {
            items: store::load(),
            dir: cfg.resolve_dir(),
            selected: 0,
        }
    }

    /// Begin fetching `url` on a background thread, adding an Active entry on top.
    pub fn start(&mut self, url: &str, sender: &UserEventSender) {
        if let Err(e) = std::fs::create_dir_all(&self.dir) {
            log::warn!("could not create download dir `{}`: {e}", self.dir);
            self.items.insert(
                0,
                Download {
                    url: url.to_string(),
                    filename: worker::filename_from_url(url),
                    path: String::new(),
                    received: 0,
                    total: 0,
                    time: history::now_unix(),
                    state: State::Failed(format!("create dir: {e}")),
                    shared: None,
                },
            );
            store::save(&self.items);
            return;
        }

        let (path, shared) = worker::spawn(url, &self.dir, sender);
        self.items.insert(
            0,
            Download {
                url: url.to_string(),
                filename: file_name_of(&path),
                path,
                received: 0,
                total: 0,
                time: 0,
                state: State::Active,
                shared: Some(shared),
            },
        );
    }

    /// Pull progress from the worker threads into the entries and record finishes
    /// (which also persists). Called once per frame; cheap when nothing is active.
    pub fn poll(&mut self) {
        let mut finished = false;
        for d in &mut self.items {
            let Some(shared) = &d.shared else { continue };
            d.received = shared.received.load(Ordering::Relaxed);
            d.total = shared.total.load(Ordering::Relaxed);
            let result = shared.result.lock().unwrap().take();
            if let Some(result) = result {
                d.state = match result {
                    Ok(()) => State::Done,
                    Err(e) => State::Failed(e),
                };
                d.time = history::now_unix();
                d.shared = None;
                finished = true;
            }
        }
        if finished {
            store::save(&self.items);
        }
    }

    pub fn items(&self) -> &[Download] {
        &self.items
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Number of downloads still in flight (drives the toolbar ⬇ indicator).
    pub fn active_count(&self) -> usize {
        self.items.iter().filter(|d| d.is_active()).count()
    }

    pub fn has_finished(&self) -> bool {
        self.items.iter().any(|d| !d.is_active())
    }

    /// Reset the highlight to the top (called when the menu opens).
    pub fn reset(&mut self) {
        self.selected = 0;
    }

    /// Move the highlight by `dy` rows, clamped to the list.
    pub fn move_sel(&mut self, dy: i32) {
        if self.items.is_empty() {
            return;
        }
        let last = self.items.len() as i32 - 1;
        self.selected = (self.selected as i32 + dy).clamp(0, last) as usize;
    }

    /// `file://` URL of the entry at `index` if it finished successfully (so it
    /// can be opened in the browser); `None` otherwise.
    pub fn open_url(&self, index: usize) -> Option<String> {
        let d = self.items.get(index)?;
        matches!(d.state, State::Done).then(|| format!("file://{}", d.path))
    }

    pub fn selected_open_url(&self) -> Option<String> {
        self.open_url(self.selected)
    }

    /// X/✖ on an entry: cancel it if still active (the entry stays and turns
    /// Failed once the worker stops), otherwise remove it from the list. The
    /// downloaded file on disk is kept either way.
    pub fn remove(&mut self, index: usize) {
        let Some(d) = self.items.get(index) else {
            return;
        };
        if let Some(shared) = &d.shared {
            shared.cancel.store(true, Ordering::Relaxed);
            return;
        }
        self.items.remove(index);
        self.clamp_selected();
        store::save(&self.items);
    }

    pub fn remove_selected(&mut self) {
        self.remove(self.selected);
    }

    /// Drop all finished entries (active ones stay); persists.
    pub fn clear_finished(&mut self) {
        let before = self.items.len();
        self.items.retain(|d| d.is_active());
        if self.items.len() != before {
            self.clamp_selected();
            store::save(&self.items);
        }
    }

    fn clamp_selected(&mut self) {
        self.selected = self.selected.min(self.items.len().saturating_sub(1));
    }
}

fn file_name_of(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

/// Compact human size, e.g. `831 B`, `3.4 MB`.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
