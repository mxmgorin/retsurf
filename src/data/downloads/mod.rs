//! File downloads. Servo's embedding API has no support for them (no delegate hook, no
//! `Content-Disposition` handling; still true on `main`), so retsurf does it itself: the
//! browser denies navigations to file-like URLs (see [`crate::browser`]) and the main loop
//! hands them here. Fetching runs on background threads (see [`worker`]); finished entries
//! persist to `downloads.toml` (see [`store`]), active ones don't survive a restart. Owns
//! the entry list and the menu's highlighted row; [`crate::ui`] renders it.

mod naming;
mod store;
mod worker;

use crate::config::DownloadsConfig;
pub use naming::sanitize as sanitize_filename;

/// A denied download navigation or an `a[download]` link, handed over by
/// [`crate::browser`] for the workers here to fetch.
pub struct DownloadRequest {
    pub url: String,
    /// Linking page, sent as Referer.
    pub referer: Option<String>,
    /// Name the page's `download` attribute asked for (already sanitized).
    pub suggested_name: Option<String>,
    /// The browser's cookies for `url`, sent along so a signed-in session
    /// reaches the file.
    pub cookie: Option<String>,
}

/// One file captured whole from a page (see the browser's blob shim) — no fetch
/// to run, the entry is born finished.
pub struct BlobDownload {
    pub filename: String,
    /// `Err` carries a page-side failure (over the size limit, unreadable blob).
    pub bytes: Result<Vec<u8>, String>,
}
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
    /// The user cancelled it, so its end goes unannounced.
    cancelled: bool,
}

/// A download that just ended, for the user to be told.
pub struct Finished {
    pub filename: String,
    /// Why it failed; `None` once saved.
    pub error: Option<String>,
    /// A web page the site sent in the file's place, for the tab to open.
    pub page: Option<String>,
}

/// The downloads in flight, summed with those saved while any of them still ran.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Progress {
    pub active: usize,
    pub received: u64,
    /// `None` while any size in flight is unknown.
    pub total: Option<u64>,
}

impl Progress {
    /// How far the downloads are while their size is known, else how many run.
    pub fn label(&self) -> String {
        match self.total {
            Some(total) if total > 0 => format!("{}%", self.received.min(total) * 100 / total),
            _ => format!("{} active", self.active),
        }
    }
}

impl Download {
    pub fn is_active(&self) -> bool {
        matches!(self.state, State::Active)
    }

    /// One-line status for the menu row: progress while active, size + date when
    /// done, the error otherwise.
    pub fn status_text(&self) -> String {
        match &self.state {
            State::Active => format_progress(self.received, self.total),
            State::Done => format!(
                "{} · {}",
                format_size(self.received),
                history::format_time(self.time)
            ),
            // The menu renders this in the Proportional family, which falls back
            // to the icon font (see `crate::ui::theme`), so the marker resolves.
            State::Failed(e) => format!("{} {e}", egui_phosphor::bold::X),
        }
    }
}

pub struct Downloads {
    /// Most-recent first; active entries are always from this session.
    items: Vec<Download>,
    /// Save directory, with a trailing separator (see [`DownloadsConfig`]).
    dir: String,
    /// Saved files move on into the system's shared Downloads (see [`hand_off`]).
    publish: bool,
    /// The browser's UA string, sent by the workers' fetches.
    pub user_agent: String,
    /// Highlighted row in the menu's Downloads section.
    cursor: crate::data::ListCursor,
    /// Bytes saved by downloads that ended while others still ran; [`Progress`]
    /// counts them until the last one ends.
    batch_saved: u64,
}

impl Downloads {
    /// Load the saved list (empty if the file is missing or invalid).
    pub fn load(cfg: &DownloadsConfig, user_agent: String) -> Self {
        let downloads = Self {
            items: store::load(),
            dir: cfg.resolve_dir(),
            publish: cfg.publishes(),
            user_agent,
            // Row 0 is the "Clear finished" action (like History's "Clear all"),
            // so it stays reachable without a mouse; entries follow it.
            cursor: crate::data::ListCursor::new(1),
            batch_saved: 0,
        };
        // A folder only staging files is ours alone: what a killed run left there
        // can go.
        if downloads.publish {
            naming::remove_partials(&downloads.dir);
        }
        downloads
    }

    /// Remove the partial files of the downloads still in flight, whose workers
    /// end with the process.
    pub fn discard_partials(&self) {
        for d in self
            .items
            .iter()
            .filter(|d| d.is_active() && !d.path.is_empty())
        {
            let _ = std::fs::remove_file(naming::part_of(&d.path));
        }
    }

    /// Begin fetching a denied navigation, adding an Active entry on top.
    /// Returns its end when it failed before any fetch.
    pub fn start(
        &mut self,
        request: DownloadRequest,
        sender: &UserEventSender,
    ) -> Option<Finished> {
        if let Err(e) = std::fs::create_dir_all(&self.dir) {
            log::warn!("could not create download dir `{}`: {e}", self.dir);
            let error = format!("create dir: {e}");
            let filename = naming::filename_from_url(&request.url);
            self.items.insert(
                0,
                Download {
                    filename: filename.clone(),
                    url: request.url,
                    path: String::new(),
                    received: 0,
                    total: 0,
                    time: crate::clock::now_unix(),
                    state: State::Failed(error.clone()),
                    shared: None,
                    cancelled: false,
                },
            );
            store::save(&self.items);
            return Some(Finished {
                filename,
                error: Some(error),
                page: None,
            });
        }

        let shared = worker::spawn(&request, &self.user_agent, &self.dir, self.publish, sender);
        self.items.insert(
            0,
            Download {
                filename: request
                    .suggested_name
                    .unwrap_or_else(|| naming::filename_from_url(&request.url)),
                url: request.url,
                path: String::new(),
                received: 0,
                total: 0,
                time: 0,
                state: State::Active,
                shared: Some(shared),
                cancelled: false,
            },
        );
        None
    }

    /// Record a file the page built in JavaScript and handed us whole (see
    /// [`crate::browser::BlobDownload`]). No fetch to run, so the entry is born finished.
    /// Returns that end.
    pub fn save_captured(&mut self, item: BlobDownload) -> Finished {
        let (filename, path, size, error) = match self.write_captured(&item.filename, item.bytes) {
            Ok((path, size)) => (file_name_of(&path), path, size, None),
            Err(e) => (item.filename, String::new(), 0, Some(e)),
        };
        let finished = Finished {
            filename: filename.clone(),
            error: error.clone(),
            page: None,
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
                time: crate::clock::now_unix(),
                state: error.map_or(State::Done, State::Failed),
                shared: None,
                cancelled: false,
            },
        );
        store::save(&self.items);
        finished
    }

    /// Write captured bytes into the download dir under a free name.
    fn write_captured(
        &self,
        filename: &str,
        bytes: Result<Vec<u8>, String>,
    ) -> Result<(String, u64), String> {
        let bytes = bytes?;
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("create dir: {e}"))?;
        let path = naming::unique_path(&self.dir, filename);
        std::fs::write(&path, &bytes).map_err(|e| format!("write: {e}"))?;
        let path = match self.publish {
            true => hand_off(path),
            false => path,
        };
        Ok((path, bytes.len() as u64))
    }

    /// Pull progress from the worker threads into the entries and record finishes
    /// (which also persists). Called once per frame; cheap when nothing is active.
    /// Returns the downloads that ended, less those the user cancelled.
    pub fn poll(&mut self) -> Vec<Finished> {
        let mut finished = Vec::new();
        for d in &mut self.items {
            let Some(shared) = &d.shared else { continue };
            // Result first: the worker stores its last count before it, so a
            // finished entry reads the final size.
            let result = shared.result.lock().unwrap().take();
            d.received = shared.received.load(Ordering::Relaxed);
            d.total = shared.total.load(Ordering::Relaxed);
            if let Some(path) = shared.path.lock().unwrap().take() {
                d.filename = file_name_of(&path);
                d.path = path;
            }
            if let Some(result) = result {
                if result.is_ok() {
                    self.batch_saved += d.received;
                }
                if !d.cancelled {
                    finished.push(Finished {
                        filename: d.filename.clone(),
                        error: result.clone().err(),
                        page: shared.page.lock().unwrap().take(),
                    });
                }
                d.state = match result {
                    Ok(()) => State::Done,
                    Err(e) => State::Failed(e),
                };
                d.time = crate::clock::now_unix();
                d.shared = None;
            }
        }
        if !finished.is_empty() {
            store::save(&self.items);
        }
        if !self.items.iter().any(Download::is_active) {
            self.batch_saved = 0;
        }
        finished
    }

    pub fn items(&self) -> &[Download] {
        &self.items
    }

    pub fn selected(&self) -> usize {
        self.cursor.selected()
    }

    /// The downloads in flight, or `None` without any.
    pub fn progress(&self) -> Option<Progress> {
        let mut progress = Progress {
            active: 0,
            received: self.batch_saved,
            total: Some(self.batch_saved),
        };
        for d in self.items.iter().filter(|d| d.is_active()) {
            progress.active += 1;
            progress.received += d.received;
            progress.total = progress
                .total
                .zip((d.total > 0).then_some(d.total))
                .map(|(a, b)| a + b);
        }
        (progress.active > 0).then_some(progress)
    }

    pub fn has_finished(&self) -> bool {
        self.items.iter().any(|d| !d.is_active())
    }

    /// Whether the "Clear finished" top row (cursor index 0) is highlighted. Only
    /// meaningful while there are entries (the row isn't shown otherwise).
    pub fn clear_selected(&self) -> bool {
        self.cursor.on_reserved_row(self.items.len())
    }

    /// Reset the highlight to the top (called when the menu opens).
    pub fn reset(&mut self) {
        self.cursor.reset(self.items.len());
    }

    /// Move the highlight by `dy` rows, clamped to the list.
    pub fn move_sel(&mut self, dy: i32) {
        self.cursor.move_sel(dy, self.items.len());
    }

    /// Put the highlight on row `index`, counted as [`Self::move_sel`] does.
    pub fn select(&mut self, index: usize) {
        self.cursor.select(index, self.items.len());
    }

    /// Where the entry at `index` was saved, once it finished; `None` otherwise.
    pub fn saved_path(&self, index: usize) -> Option<&str> {
        let d = self.items.get(index)?;
        matches!(d.state, State::Done).then_some(d.path.as_str())
    }

    /// The highlighted entry's index, `None` on the "Clear finished" row.
    pub fn selected_entry(&self) -> Option<usize> {
        self.cursor.entry_index()
    }

    /// Cancel the entry if still active (it stays, turning Failed once the worker
    /// stops), otherwise drop it from the list. The file on disk is kept either way.
    pub fn remove(&mut self, index: usize) {
        let Some(d) = self.items.get_mut(index) else {
            return;
        };
        if let Some(shared) = &d.shared {
            shared.cancel.store(true, Ordering::Relaxed);
            d.cancelled = true;
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

/// Move the saved file at `path` on into the system's shared Downloads; returns
/// where it is now, `path` itself when it could not move.
#[cfg(target_os = "android")]
fn hand_off(path: String) -> String {
    match crate::platform::android::publish_download(&path) {
        Ok(moved) => moved,
        Err(e) => {
            log::warn!("could not move `{path}` into Downloads: {e}");
            path
        }
    }
}

/// Only Android keeps a shared Downloads apart from the save folder.
#[cfg(not(target_os = "android"))]
fn hand_off(path: String) -> String {
    path
}

/// `path` as a `file://` URL, its name escaped so `#`, `?` and `%` stay part of
/// it; `None` for a relative path.
pub fn file_url(path: &str) -> Option<String> {
    url::Url::from_file_path(path).ok().map(String::from)
}

fn file_name_of(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

/// `NN% · a / b` while the total is known, else just what has arrived so far.
/// Shared by the menu's download rows and the About tab's update row.
pub fn format_progress(received: u64, total: u64) -> String {
    match (received * 100).checked_div(total) {
        Some(pct) => format!(
            "{pct}% · {} / {}",
            format_size(received),
            format_size(total)
        ),
        None => format_size(received),
    }
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

#[cfg(test)]
mod tests {
    use super::file_url;

    #[test]
    fn a_file_url_keeps_the_whole_name() {
        let url = file_url("/dl/Track #01?.mp3").unwrap();
        let parsed = url::Url::parse(&url).unwrap();
        assert_eq!(parsed.fragment(), None);
        assert_eq!(parsed.query(), None);
        let path = parsed.to_file_path().unwrap();
        assert_eq!(path.to_str(), Some("/dl/Track #01?.mp3"));
    }

    #[test]
    fn a_literal_percent_is_not_decoded() {
        let url = file_url("/dl/100%41.zip").unwrap();
        let path = url::Url::parse(&url).unwrap().to_file_path().unwrap();
        assert_eq!(path.to_str(), Some("/dl/100%41.zip"));
    }
}
