//! Link-hint navigation state — Vimium adapted for a gamepad. L3 asks the page
//! for its visible clickable elements (collected via JavaScript, see
//! [`crate::browser`]); badges are drawn over them ([`crate::ui`]) and the stick
//! hops the selection spatially (nearest element in the pressed direction)
//! instead of typing hint letters. A clicks the selected element, B exits.
//! Scrolling keeps working and schedules a re-collect once it settles, since
//! the rects are viewport-relative and go stale as the page moves. Pushing the
//! selection past the last hint at a vertical edge scrolls a chunk and
//! re-collects, so one stick reaches the whole document (see the router).

use std::time::{Duration, Instant};

/// How long after the last scroll input before the hints are re-collected.
const REFRESH_DEBOUNCE: Duration = Duration::from_millis(150);

/// One clickable element, viewport-relative (browser-area logical px).
pub struct Hint {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Absolute http(s) URL when the element is a link — lets the "open in new
    /// tab" gesture skip the click and load it directly. `None` for buttons and
    /// other non-link clickables, which fall back to a normal click.
    pub url: Option<String>,
}

impl Hint {
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

pub struct Hints {
    pub visible: bool,
    hints: Vec<Hint>,
    selected: usize,
    /// Whether a collection round is expected (L3 pressed, or a post-scroll
    /// refresh): results arriving while neither expected nor visible are stale
    /// (e.g. from a round whose mode was exited meanwhile) and get dropped.
    collecting: bool,
    /// When the post-scroll re-collect comes due.
    refresh_at: Option<Instant>,
    /// Where the selection should land after the next re-collect, overriding the
    /// caller's default. Set by an edge auto-scroll so the selection snaps to a
    /// freshly-revealed hint at the leading edge, not the now-stale old center.
    next_near: Option<(f32, f32)>,
}

impl Hints {
    pub fn new() -> Self {
        Self {
            visible: false,
            hints: vec![],
            selected: 0,
            collecting: false,
            refresh_at: None,
            next_near: None,
        }
    }

    /// A collection round was started; the results arrive asynchronously.
    pub fn begin_collect(&mut self) {
        self.collecting = true;
    }

    /// Fresh rects from the page. Keeps the selection near `near` (the previous
    /// selection, or the gamepad cursor on entry); an empty result exits the mode
    /// (nothing to select on this page/viewport).
    pub fn show(&mut self, hints: Vec<Hint>, near: (f32, f32)) {
        if !self.collecting && !self.visible {
            return; // stale result of an exited round
        }
        self.collecting = false;
        self.refresh_at = None;
        if hints.is_empty() {
            self.hide();
            return;
        }
        let near = self.next_near.take().unwrap_or(near);
        self.selected = nearest_to(&hints, near);
        self.hints = hints;
        self.visible = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.collecting = false;
        self.refresh_at = None;
        self.next_near = None;
        self.hints.clear();
        self.selected = 0;
    }

    pub fn hints(&self) -> &[Hint] {
        &self.hints
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn selected_center(&self) -> Option<(f32, f32)> {
        self.hints.get(self.selected).map(Hint::center)
    }

    /// The selected hint's link URL, if it is a link (`None` for buttons etc.).
    pub fn selected_url(&self) -> Option<&str> {
        self.hints.get(self.selected)?.url.as_deref()
    }

    /// Hop the selection to the nearest hint in direction `dir` (a dominant-axis
    /// step from the router, e.g. `(0, -1)` = up). Returns whether it moved;
    /// `false` means the edge (no hint further that way) — the router turns a
    /// vertical edge into a page scroll.
    pub fn move_sel(&mut self, dir: (i32, i32)) -> bool {
        let Some(from) = self.selected_center() else {
            return false;
        };
        let d = (dir.0 as f32, dir.1 as f32);
        let mut best: Option<(f32, usize)> = None;
        for (i, hint) in self.hints.iter().enumerate() {
            if i == self.selected {
                continue;
            }
            let c = hint.center();
            let v = (c.0 - from.0, c.1 - from.1);
            // Distance along the pressed direction; must actually advance.
            let along = v.0 * d.0 + v.1 * d.1;
            if along < 4.0 {
                continue;
            }
            // Penalize sideways offset so a slightly-farther but in-line hint
            // beats a near one off to the side.
            let aside = (v.0 * d.1 - v.1 * d.0).abs();
            let score = along + 2.5 * aside;
            if best.is_none_or(|(s, _)| score < s) {
                best = Some((score, i));
            }
        }
        if let Some((_, i)) = best {
            self.selected = i;
            return true;
        }
        false
    }

    /// Scrolling shifts the page under the badges: schedule a re-collect for
    /// when it settles (each call pushes the deadline out — a debounce).
    pub fn mark_stale(&mut self) {
        if self.visible {
            self.refresh_at = Some(Instant::now() + REFRESH_DEBOUNCE);
        }
    }

    /// Like [`Self::mark_stale`], but pins where the selection lands once the
    /// re-collect arrives — an edge auto-scroll points it at the leading edge so
    /// the selection snaps onto a newly-revealed hint there.
    pub fn mark_stale_at(&mut self, near: (f32, f32)) {
        if self.visible {
            self.refresh_at = Some(Instant::now() + REFRESH_DEBOUNCE);
            self.next_near = Some(near);
        }
    }

    /// Whether the scheduled re-collect is due; taking it clears the schedule
    /// (the caller starts a new collection round).
    pub fn take_refresh_due(&mut self) -> bool {
        if self.refresh_at.is_some_and(|t| Instant::now() >= t) {
            self.refresh_at = None;
            self.collecting = true;
            return true;
        }
        false
    }

    /// Time until the scheduled re-collect, for the main loop's wait timeout
    /// (the loop blocks on input otherwise and the refresh would never fire).
    pub fn refresh_in(&self) -> Option<Duration> {
        self.refresh_at
            .map(|t| t.saturating_duration_since(Instant::now()))
    }
}

/// Index of the hint whose center is closest to `p`.
fn nearest_to(hints: &[Hint], p: (f32, f32)) -> usize {
    let mut best = (f32::MAX, 0);
    for (i, hint) in hints.iter().enumerate() {
        let c = hint.center();
        let d2 = (c.0 - p.0).powi(2) + (c.1 - p.1).powi(2);
        if d2 < best.0 {
            best = (d2, i);
        }
    }
    best.1
}
