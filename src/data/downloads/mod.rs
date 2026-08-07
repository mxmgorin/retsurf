//! File downloads. Servo's embedding API has no support for them (no delegate hook, no
//! `Content-Disposition` handling; still true on `main`), so retsurf does it itself: the
//! browser denies navigations to file-like URLs (see [`crate::browser`]) and the main loop
//! hands them here. Fetching runs on background threads (see [`worker`]); finished entries
//! persist to `downloads.toml` (see [`store`]), active ones don't survive a restart. Owns
//! the entry list and the menu's highlighted row; [`crate::ui`] renders it.

mod store;
mod worker;

use crate::config::DownloadsConfig;
use crate::data::history;
use crate::event::user::UserEventSender;
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
    /// Name shown in the menu; URL-derived until the response picks the real one.
    pub filename: String,
    /// Destination path; empty until the response headers picked the name.
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
    /// The browser's UA string, sent by the workers' fetches.
    user_agent: String,
    /// Highlighted row in the menu's Downloads section.
    cursor: crate::data::ListCursor,
}

impl Downloads {
    /// Load the saved list (empty if the file is missing or invalid).
    pub fn load(cfg: &DownloadsConfig, user_agent: String) -> Self {
        Self {
            items: store::load(),
            dir: cfg.resolve_dir(),
            user_agent,
            cursor: crate::data::ListCursor::new(0),
        }
    }

    /// Begin fetching a denied navigation, adding an Active entry on top.
    pub fn start(&mut self, request: crate::browser::DownloadRequest, sender: &UserEventSender) {
        if let Err(e) = std::fs::create_dir_all(&self.dir) {
            log::warn!("could not create download dir `{}`: {e}", self.dir);
            self.items.insert(
                0,
                Download {
                    filename: worker::filename_from_url(&request.url),
                    url: request.url,
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

        let shared = worker::spawn(
            &request.url,
            request.referer,
            &self.user_agent,
            &self.dir,
            sender,
        );
        self.items.insert(
            0,
            Download {
                filename: worker::filename_from_url(&request.url),
                url: request.url,
                path: String::new(),
                received: 0,
                total: 0,
                time: 0,
                state: State::Active,
                shared: Some(shared),
            },
        );
    }

    /// Record a file the page built in JavaScript and handed us whole (see
    /// [`crate::browser::BlobDownload`]). No fetch to run, so the entry is born finished.
    pub fn save_captured(&mut self, item: crate::browser::BlobDownload) {
        let (filename, path, size, state) = match self.write_captured(&item.filename, item.bytes) {
            Ok((path, size)) => (file_name_of(&path), path, size, State::Done),
            Err(e) => (item.filename, String::new(), 0, State::Failed(e)),
        };
        self.items.insert(
            0,
            Download {
                // Blob URLs are per-document and revoked by now: nothing to re-open.
                url: String::new(),
                filename,
                path,
                received: size,
                total: size,
                time: history::now_unix(),
                state,
                shared: None,
            },
        );
        store::save(&self.items);
    }

    /// Write captured bytes into the download dir under a free name.
    fn write_captured(
        &self,
        filename: &str,
        bytes: Result<Vec<u8>, String>,
    ) -> Result<(String, u64), String> {
        let bytes = bytes?;
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("create dir: {e}"))?;
        let path = worker::unique_path(&self.dir, filename);
        std::fs::write(&path, &bytes).map_err(|e| format!("write: {e}"))?;
        Ok((path, bytes.len() as u64))
    }

    /// Pull progress from the worker threads into the entries and record finishes
    /// (which also persists). Called once per frame; cheap when nothing is active.
    pub fn poll(&mut self) {
        let mut finished = false;
        for d in &mut self.items {
            let Some(shared) = &d.shared else { continue };
            d.received = shared.received.load(Ordering::Relaxed);
            d.total = shared.total.load(Ordering::Relaxed);
            if let Some(path) = shared.path.lock().unwrap().take() {
                d.filename = file_name_of(&path);
                d.path = path;
            }
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
        self.cursor.selected()
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
        self.cursor.reset(self.items.len());
    }

    /// Move the highlight by `dy` rows, clamped to the list.
    pub fn move_sel(&mut self, dy: i32) {
        self.cursor.move_sel(dy, self.items.len());
    }

    /// `file://` URL of the entry at `index` if it finished; `None` otherwise.
    pub fn open_url(&self, index: usize) -> Option<String> {
        let d = self.items.get(index)?;
        matches!(d.state, State::Done).then(|| format!("file://{}", d.path))
    }

    pub fn selected_open_url(&self) -> Option<String> {
        self.cursor.entry_index().and_then(|i| self.open_url(i))
    }

    /// X/✖ on an entry: cancel it if still active (it stays, turning Failed once the
    /// worker stops), otherwise drop it from the list. The file on disk is kept either way.
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
        if let Some(i) = self.cursor.entry_index() {
            self.remove(i);
        }
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
        self.cursor.clamp(self.items.len());
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
