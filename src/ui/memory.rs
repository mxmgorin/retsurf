//! The debug memory overlay (`[debug] memory_overlay`): a small corner panel
//! showing Servo's live memory report rolled up by subsystem, so RAM use
//! (image-cache, layout, JS, ...) can be read off on a target device. Off by
//! default. The report is requested from the main loop and aggregated here;
//! the holding state lives in [`crate::ui::AppUi`].

use super::theme::DIM;
use egui_sdl2::egui;
use servo::profile_traits::mem::{MemoryReportResult, ReportKind};
use std::cmp::Reverse;
use std::collections::HashMap;

/// A rolled-up snapshot of one memory report, ready to render.
pub struct MemorySummary {
    /// Explicit-allocation subsystem groups, largest first.
    rows: Vec<(String, usize)>,
    /// Sum of all explicit allocations.
    explicit_total: usize,
    /// Global gauges (resident, vsize, ...), kept apart from the explicit tree
    /// so the two accountings aren't mixed/double-counted.
    gauges: Vec<(String, usize)>,
    /// Whole report paths, largest first, for `RETSURF_MEMORY_DETAIL`: the rolled
    /// up rows say `js` grew and not which allocation under it did.
    detail: Vec<(String, usize)>,
}

/// How many whole paths `RETSURF_MEMORY_DETAIL` logs.
const DETAIL_ROWS: usize = 20;

/// Whether to keep whole report paths (`RETSURF_MEMORY_DETAIL`), for a session
/// spent finding out what a group is made of.
fn detail_wanted() -> bool {
    crate::config::env_flag("RETSURF_MEMORY_DETAIL").unwrap_or(false)
}

impl MemorySummary {
    /// Roll a raw report up by the first two path segments, summing bytes.
    /// Explicit allocations form the subsystem breakdown; non-explicit entries
    /// (resident, system-heap, ...) are listed as separate gauges.
    pub fn from_report(report: MemoryReportResult) -> Self {
        let mut groups: HashMap<String, usize> = HashMap::new();
        let mut gauges: Vec<(String, usize)> = Vec::new();
        let mut detail: Vec<(String, usize)> = Vec::new();
        let detail_wanted = detail_wanted();
        let mut explicit_total = 0;
        for process in report.results {
            for r in process.reports {
                if matches!(r.kind, ReportKind::NonExplicitSize) {
                    gauges.push((r.path.join("/"), r.size));
                } else {
                    explicit_total += r.size;
                    *groups.entry(group_key(&r.path)).or_default() += r.size;
                    if detail_wanted {
                        detail.push((r.path.join("/"), r.size));
                    }
                }
            }
        }
        detail.sort_by_key(|&(_, size)| Reverse(size));
        detail.truncate(DETAIL_ROWS);
        let mut rows: Vec<(String, usize)> = groups.into_iter().collect();
        rows.sort_by_key(|&(_, size)| Reverse(size));
        rows.truncate(12);
        // Gauges include one `resident-according-to-smaps/*` entry per memory
        // mapping — keep only the largest few (vsize/resident lead) so the
        // overlay stays compact.
        gauges.sort_by_key(|&(_, size)| Reverse(size));
        gauges.truncate(6);
        Self {
            rows,
            explicit_total,
            gauges,
            detail,
        }
    }
}

impl MemorySummary {
    /// The overlay's figures as one log line, for a device whose screen nobody
    /// is reading — the Miyoo answers over ssh and nothing else.
    pub fn log(&self) {
        let fmt = |pairs: &[(String, usize)], take: usize| {
            pairs
                .iter()
                .take(take)
                .map(|(name, size)| format!("{name} {}", fmt_bytes(*size)))
                .collect::<Vec<_>>()
                .join(", ")
        };
        log::info!(
            "memory: explicit {} [{}] gauges [{}]",
            fmt_bytes(self.explicit_total),
            fmt(&self.rows, 8),
            fmt(&self.gauges, 3),
        );
        if !self.detail.is_empty() {
            log::info!("memory detail: {}", fmt(&self.detail, DETAIL_ROWS));
        }
    }
}

/// The chrome's own memory, absent from Servo's report. The painter keeps its own
/// copy of every egui texture, so the process pays twice the figure logged here.
pub fn log_chrome(ctx: &egui::Context, compose: usize) {
    let [w, h] = ctx.fonts(|f| f.font_image_size());
    let textures: usize = ctx
        .tex_manager()
        .read()
        .allocated()
        .map(|(_, meta)| meta.bytes_used())
        .sum();
    log::info!(
        "memory (chrome): atlas {w}x{h}, egui textures {}, compose {}",
        fmt_bytes(textures),
        fmt_bytes(compose),
    );
}

/// Group an explicit report by its first two path segments (e.g.
/// `image-cache`, `js/main`); deeper detail folds into the group.
fn group_key(path: &[String]) -> String {
    match path.len().min(2) {
        0 => "(root)".to_string(),
        n => path[..n].join("/"),
    }
}

/// Format a byte count compactly.
fn fmt_bytes(bytes: usize) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    const KIB: f64 = 1024.0;
    let b = bytes as f64;
    if b >= MIB {
        format!("{:.1} MiB", b / MIB)
    } else if b >= KIB {
        format!("{:.0} KiB", b / KIB)
    } else {
        format!("{bytes} B")
    }
}

/// Draw the overlay anchored top-left, above the page. Non-interactive so it
/// never steals clicks from the content beneath it.
pub(super) fn add_memory(ctx: &egui::Context, summary: &MemorySummary) {
    egui::Area::new(egui::Id::new("memory_overlay"))
        .order(egui::Order::Foreground)
        .interactable(false)
        .anchor(egui::Align2::LEFT_TOP, egui::vec2(8.0, 8.0))
        .show(ctx, |ui| {
            egui::Frame::default()
                .fill(egui::Color32::from_black_alpha(220))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_gray(0x55)))
                .corner_radius(6.0)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "memory - explicit {}",
                            fmt_bytes(summary.explicit_total)
                        ))
                        .strong()
                        .color(egui::Color32::WHITE),
                    );
                    ui.add_space(4.0);
                    for (path, size) in &summary.rows {
                        row(ui, path, *size, egui::Color32::from_gray(0xdd));
                    }
                    if !summary.gauges.is_empty() {
                        ui.add_space(4.0);
                        ui.separator();
                        for (path, size) in &summary.gauges {
                            row(ui, path, *size, DIM);
                        }
                    }
                });
        });
}

/// One `<bytes>  <label>` line.
fn row(ui: &mut egui::Ui, label: &str, size: usize, color: egui::Color32) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{:>10}", fmt_bytes(size)))
                .monospace()
                .color(color),
        );
        ui.label(egui::RichText::new(label).color(color));
    });
}
