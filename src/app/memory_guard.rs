//! The memory guard (`[performance] memory_guard`): when the
//! [`MemoryGuard`](crate::platform::memory_guard::MemoryGuard) trips, close the
//! tab holding the most memory. Servo cannot suspend or shrink a page, so
//! closing its webview is the only lever.

use super::App;
use crate::browser::HOME_URL;
use crate::data::session::TabInfo;
use servo::profile_traits::mem::MemoryReportResult;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How long a trip waits for the memory report before closing the active tab.
/// A debug build took 4.3 s on a heavy page.
const REPORT_WAIT: Duration = Duration::from_secs(5);

/// Outlasts a plain toast: nothing else explains the vanished tab.
const NOTICE: Duration = Duration::from_secs(6);

impl App {
    /// Act on a guard trip and on the report it asked for. Must run before the
    /// debug overlay takes that report.
    pub(super) fn poll_memory_guard(&mut self) {
        if let Some(trip) = self.memory_guard.as_ref().and_then(|w| w.take_trip()) {
            if !self.config.performance.memory_guard {
                log::warn!("memory guard: {trip}; off, nothing closed");
            } else if self.browser.tab_count() > 1 {
                log::warn!("memory guard: {trip}; finding the heaviest tab");
                // A stale report may name the wrong tab.
                let _ = self.browser.take_memory_report();
                self.browser.request_memory_report();
                self.memory_guard_asked = Some(Instant::now());
            } else {
                log::warn!("memory guard: {trip}");
                self.close_for_memory(self.browser.active_tab());
            }
        }

        let Some(asked) = self.memory_guard_asked else {
            return;
        };
        let tabs = self.browser.tabs();
        let index = match self.browser.take_memory_report() {
            Some(report) => {
                log::info!("memory guard: report in {} ms", asked.elapsed().as_millis());
                heaviest_tab(&report, &tabs)
            }
            None if asked.elapsed() >= REPORT_WAIT => {
                log::warn!("memory guard: no report in time, closing the active tab");
                None
            }
            None => return,
        };
        self.memory_guard_asked = None;
        self.close_for_memory(index.unwrap_or_else(|| self.browser.active_tab()));
    }

    /// Close the tab at `index`, except the start page, which would only reopen.
    fn close_for_memory(&mut self, index: usize) {
        let Some(url) = self.browser.tabs().get(index).map(|t| t.url.clone()) else {
            return;
        };
        if url == HOME_URL {
            log::warn!("memory guard: the start page is the tab to close; left open");
            return;
        }
        log::warn!("memory guard: closing tab {index} ({url})");
        self.close_tab_at(index);
        // Now, so a freeze or a kill cannot restore the page.
        self.save_session();
        let host = url::Url::parse(&url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or(url);
        self.ui.toast_for(
            format!("{host} needed more memory than this device has"),
            NOTICE,
        );
    }
}

/// The tab charged the most bytes. Report paths open with `url(<urls>)`, where
/// a script thread lists its documents joined by ", "; its total is split evenly.
fn heaviest_tab(report: &MemoryReportResult, tabs: &[TabInfo]) -> Option<usize> {
    let mut by_url: HashMap<&str, usize> = HashMap::new();
    for process in &report.results {
        for r in &process.reports {
            let Some(urls) = r
                .path
                .first()
                .and_then(|p| p.strip_prefix("url("))
                .and_then(|p| p.strip_suffix(')'))
            else {
                continue;
            };
            let urls: Vec<&str> = urls.split(", ").collect();
            for url in &urls {
                *by_url.entry(url).or_default() += r.size / urls.len();
            }
        }
    }
    tabs.iter()
        .enumerate()
        .filter_map(|(i, tab)| by_url.get(tab.url.as_str()).map(|&size| (i, size)))
        .max_by_key(|&(_, size)| size)
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use servo::profile_traits::mem::{MemoryReport, Report, ReportKind};

    fn report(entries: &[(&str, usize)]) -> MemoryReportResult {
        let reports = entries
            .iter()
            .map(|&(prefix, size)| Report {
                path: vec![prefix.to_string(), "js".to_string()],
                kind: ReportKind::ExplicitJemallocHeapSize,
                size,
            })
            .collect();
        MemoryReportResult {
            results: vec![MemoryReport {
                pid: 0,
                is_main_process: true,
                reports,
            }],
        }
    }

    fn tabs(urls: &[&str]) -> Vec<TabInfo> {
        urls.iter()
            .map(|url| TabInfo {
                title: String::new(),
                url: url.to_string(),
                active: false,
                favicon: None,
            })
            .collect()
    }

    #[test]
    fn picks_the_tab_with_the_most_bytes() {
        let report = report(&[
            ("url(https://a.test/)", 10),
            ("url(https://b.test/)", 300),
            ("url(https://b.test/)", 50),
            ("image-cache", 1000),
        ]);
        let tabs = tabs(&["https://a.test/", "https://b.test/"]);
        assert_eq!(heaviest_tab(&report, &tabs), Some(1));
    }

    /// A script thread's total is shared by the documents it serves.
    #[test]
    fn splits_a_shared_script_thread() {
        let report = report(&[
            ("url(https://a.test/, https://a.test/x)", 400),
            ("url(https://b.test/)", 250),
        ]);
        let tabs = tabs(&["https://a.test/", "https://b.test/"]);
        assert_eq!(heaviest_tab(&report, &tabs), Some(1));
    }

    #[test]
    fn none_when_no_tab_is_named() {
        let report = report(&[("image-cache", 1000)]);
        assert_eq!(heaviest_tab(&report, &tabs(&["https://a.test/"])), None);
    }
}
