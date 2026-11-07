//! The toolbar's downloads button: what it shows of the downloads in flight,
//! and, for a moment after the last one ends, how they ended. Progress is
//! sampled at the workers' notify cadence, so the button redraws the chrome no
//! more often than they wake the loop; the arc standing in for an unknown size
//! steps only while bytes arrive, so a stalled transfer stands still.

use super::toast::Notice;
use super::AppUi;
use crate::data::downloads::{Finished, Progress};
use crate::net::NOTIFY_EVERY;
use egui_phosphor::bold;
use std::time::{Duration, Instant};

/// How long the ending mark stays up.
const ENDED_FOR: Duration = Duration::from_millis(2500);

/// What the downloads button shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Chip {
    /// Nothing in flight, nor just ended.
    #[default]
    Hidden,
    /// Downloads in flight: how many, the share done out of [`Chip::SHARE_FULL`]
    /// (`None` while a size is unknown), and how far the unknown-size arc has stepped.
    Running {
        count: usize,
        share: Option<u8>,
        step: u32,
    },
    /// The last of a batch just ended; `failed` if any of it did.
    Ended { failed: bool },
}

impl Chip {
    /// A [`Chip::Running`] share when all is done. About the ring's circumference
    /// in pixels: a finer share would redraw the chrome for no visible change.
    pub const SHARE_FULL: u8 = 100;
}

/// [`Chip`] over time: samples the downloads and holds the ending mark.
pub(super) struct DownloadChip {
    shown: Chip,
    /// When `shown` last took a sample.
    sampled: Instant,
    /// Bytes received as of that sample, telling a moving transfer from a stalled one.
    received: u64,
    /// Time the running batch has spent receiving; steps the unknown-size arc.
    flowing: Duration,
    /// Whether any download of the running batch ended saved, or failed.
    saved: bool,
    failed: bool,
    /// When the ending mark went up.
    ended: Instant,
}

impl DownloadChip {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            shown: Chip::Hidden,
            sampled: now,
            received: 0,
            flowing: Duration::ZERO,
            saved: false,
            failed: false,
            ended: now,
        }
    }

    pub fn shown(&self) -> Chip {
        self.shown
    }

    /// Record a download that ended, for the mark shown once the last one has.
    pub fn note(&mut self, finished: &Finished) {
        match finished.error {
            None => self.saved = true,
            Some(_) => self.failed = true,
        }
    }

    /// Bring the chip up to `progress`, the downloads as of `now`.
    pub fn refresh(&mut self, progress: Option<Progress>, now: Instant) {
        // Half the notify cadence: every worker wake samples, the repaint
        // passes a redraw brings along do not.
        let sample_every = NOTIFY_EVERY / 2;
        let since = now.saturating_duration_since(self.sampled);
        self.shown = match (progress, self.shown) {
            (Some(p), Chip::Running { count, .. }) if count == p.active && since < sample_every => {
                return;
            }
            (Some(p), shown) => {
                match shown {
                    Chip::Running { .. } if p.received != self.received => {
                        self.flowing += since.min(NOTIFY_EVERY);
                    }
                    Chip::Running { .. } => {}
                    Chip::Hidden | Chip::Ended { .. } => self.flowing = Duration::ZERO,
                }
                self.received = p.received;
                self.sampled = now;
                let share = p.total.map(|total| {
                    let done = (p.received as f32 / total as f32).min(1.0);
                    (done * f32::from(Chip::SHARE_FULL)) as u8
                });
                Chip::Running {
                    count: p.active,
                    share,
                    step: (self.flowing.as_nanos() / NOTIFY_EVERY.as_nanos()) as u32,
                }
            }
            (None, _) if self.saved || self.failed => {
                self.ended = now;
                self.saved = false;
                Chip::Ended {
                    failed: std::mem::take(&mut self.failed),
                }
            }
            (None, Chip::Ended { .. }) if now.saturating_duration_since(self.ended) < ENDED_FOR => {
                return;
            }
            (None, _) => Chip::Hidden,
        };
    }

    /// Time left on the ending mark, while one is up.
    pub fn ended_for(&self, now: Instant) -> Option<Duration> {
        matches!(self.shown, Chip::Ended { .. })
            .then(|| ENDED_FOR.saturating_sub(now.saturating_duration_since(self.ended)))
    }
}

impl AppUi {
    /// Tell the user a download ended: a toast now, and the button's mark once
    /// the last one has.
    pub fn download_finished(&mut self, finished: &Finished) {
        self.download_chip.note(finished);
        let notice = match &finished.error {
            None => Notice::new(bold::CHECK, format!("Downloaded {}", finished.filename)),
            Some(e) => Notice::warn(format!("Download failed: {e}")),
        };
        self.toast(notice);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1 << 20;

    fn progress(active: usize, received: u64, total: Option<u64>) -> Option<Progress> {
        Some(Progress {
            active,
            received,
            total,
        })
    }

    fn saved() -> Finished {
        Finished {
            filename: "a.zip".into(),
            error: None,
        }
    }

    fn failed() -> Finished {
        Finished {
            filename: "a.zip".into(),
            error: Some("stalled".into()),
        }
    }

    #[test]
    fn a_running_download_shows_its_share() {
        let mut chip = DownloadChip::new();
        chip.refresh(progress(1, MB, Some(4 * MB)), Instant::now());
        assert_eq!(
            chip.shown(),
            Chip::Running {
                count: 1,
                share: Some(25),
                step: 0
            }
        );
    }

    /// Repaint passes right after a sample would otherwise each redraw a new share.
    #[test]
    fn samples_no_faster_than_the_workers_notify() {
        let mut chip = DownloadChip::new();
        let t0 = Instant::now();
        chip.refresh(progress(1, MB, Some(4 * MB)), t0);
        chip.refresh(
            progress(1, 2 * MB, Some(4 * MB)),
            t0 + Duration::from_millis(1),
        );
        assert!(matches!(
            chip.shown(),
            Chip::Running {
                share: Some(25),
                ..
            }
        ));
        chip.refresh(progress(1, 2 * MB, Some(4 * MB)), t0 + NOTIFY_EVERY);
        assert!(matches!(
            chip.shown(),
            Chip::Running {
                share: Some(50),
                ..
            }
        ));
    }

    #[test]
    fn a_new_download_shows_at_once() {
        let mut chip = DownloadChip::new();
        let t0 = Instant::now();
        chip.refresh(progress(1, MB, Some(4 * MB)), t0);
        chip.refresh(progress(2, MB, Some(8 * MB)), t0 + Duration::from_millis(1));
        assert!(matches!(chip.shown(), Chip::Running { count: 2, .. }));
    }

    #[test]
    fn the_unknown_size_arc_steps_only_while_bytes_arrive() {
        let mut chip = DownloadChip::new();
        let t0 = Instant::now();
        chip.refresh(progress(1, MB, None), t0);
        chip.refresh(progress(1, 2 * MB, None), t0 + NOTIFY_EVERY);
        assert!(matches!(
            chip.shown(),
            Chip::Running {
                step: 1,
                share: None,
                ..
            }
        ));
        chip.refresh(progress(1, 2 * MB, None), t0 + 2 * NOTIFY_EVERY);
        assert!(matches!(chip.shown(), Chip::Running { step: 1, .. }));
    }

    #[test]
    fn a_stall_does_not_count_toward_the_arc() {
        let mut chip = DownloadChip::new();
        let t0 = Instant::now();
        chip.refresh(progress(1, MB, None), t0);
        chip.refresh(progress(1, 2 * MB, None), t0 + 40 * NOTIFY_EVERY);
        assert!(matches!(chip.shown(), Chip::Running { step: 1, .. }));
    }

    #[test]
    fn the_last_end_holds_its_mark_then_hides() {
        let mut chip = DownloadChip::new();
        let t0 = Instant::now();
        chip.refresh(progress(1, MB, Some(MB)), t0);
        chip.note(&saved());
        chip.refresh(None, t0 + NOTIFY_EVERY);
        assert_eq!(chip.shown(), Chip::Ended { failed: false });
        chip.refresh(None, t0 + NOTIFY_EVERY + ENDED_FOR / 2);
        assert_eq!(chip.shown(), Chip::Ended { failed: false });
        chip.refresh(None, t0 + NOTIFY_EVERY + ENDED_FOR);
        assert_eq!(chip.shown(), Chip::Hidden);
        assert_eq!(chip.ended_for(t0 + NOTIFY_EVERY + ENDED_FOR), None);
    }

    #[test]
    fn one_failure_marks_the_whole_batch() {
        let mut chip = DownloadChip::new();
        let t0 = Instant::now();
        chip.refresh(progress(2, MB, Some(2 * MB)), t0);
        chip.note(&failed());
        chip.refresh(progress(1, MB, Some(2 * MB)), t0 + NOTIFY_EVERY);
        chip.note(&saved());
        chip.refresh(None, t0 + 2 * NOTIFY_EVERY);
        assert_eq!(chip.shown(), Chip::Ended { failed: true });
    }

    /// A cancel reports no end, so nothing is left to mark.
    #[test]
    fn a_cancelled_batch_just_hides() {
        let mut chip = DownloadChip::new();
        let t0 = Instant::now();
        chip.refresh(progress(1, MB, Some(4 * MB)), t0);
        chip.refresh(None, t0 + NOTIFY_EVERY);
        assert_eq!(chip.shown(), Chip::Hidden);
    }

    /// A page-built file is saved whole, never seen running.
    #[test]
    fn an_end_never_seen_running_still_marks() {
        let mut chip = DownloadChip::new();
        chip.note(&saved());
        chip.refresh(None, Instant::now());
        assert_eq!(chip.shown(), Chip::Ended { failed: false });
    }
}
