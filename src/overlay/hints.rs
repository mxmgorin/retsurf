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

/// The combo alphabet: gamepad controls usable as typed hint codes. A and B are
/// reserved (click / cancel), so the symbols are drawn from the rest. The order
/// here is the code-assignment order (low codes go to the nearest hints) and the
/// on-screen badge order.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Sym {
    X,
    Y,
    L1,
    R1,
    Up,
    Down,
    Left,
    Right,
}

impl Sym {
    pub const ALL: [Sym; 8] = [
        Sym::X,
        Sym::Y,
        Sym::L1,
        Sym::R1,
        Sym::Up,
        Sym::Down,
        Sym::Left,
        Sym::Right,
    ];
}

/// Outcome of feeding one symbol into the typed buffer (see [`Hints::push_sym`]).
pub enum HintInput {
    /// The buffer is a live prefix of one or more codes — keep typing.
    Pending,
    /// The buffer completed exactly one code; activate that hint.
    Activate(usize),
    /// No code has the buffer as a prefix (a dead end, or a completed unused
    /// code). The buffer is cleared; the caller may flash feedback.
    NoMatch,
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
    /// Typed combo code per hint (parallel to `hints`), reassigned every collect.
    codes: Vec<Vec<Sym>>,
    /// Symbols pressed so far toward a code; cleared on activate or re-collect.
    typed: Vec<Sym>,
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
            codes: vec![],
            typed: vec![],
        }
    }

    /// A collection round was started; the results arrive asynchronously.
    pub fn begin_collect(&mut self) {
        self.collecting = true;
    }

    /// Fresh rects from the page. Keeps the selection near `near` (the previous
    /// selection, or the gamepad cursor on entry); an empty result exits the mode
    /// (nothing to select on this page/viewport). `order_from` (the viewport
    /// top-center) seeds the combo-code order so the nearest-to-top hints get the
    /// shortest codes. Codes are reassigned and any partial combo is dropped.
    pub fn show(&mut self, hints: Vec<Hint>, near: (f32, f32), order_from: (f32, f32)) {
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
        self.codes = assign_codes(&hints, order_from);
        self.typed.clear();
        self.hints = hints;
        self.visible = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.collecting = false;
        self.refresh_at = None;
        self.next_near = None;
        self.hints.clear();
        self.codes.clear();
        self.typed.clear();
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

    /// Point the selection at `idx` (a combo resolved to this hint) so the shared
    /// click path ([`crate::app`]'s `activate_hint`) targets it. No-op if stale.
    pub fn select(&mut self, idx: usize) {
        if idx < self.hints.len() {
            self.selected = idx;
        }
    }

    /// Feed one combo symbol into the typed buffer (see [`HintInput`]). Codes are
    /// uniform-length and unique, so a full buffer that still prefixes a code is
    /// an exact, single match.
    pub fn push_sym(&mut self, s: Sym) -> HintInput {
        self.typed.push(s);
        if !(0..self.codes.len()).any(|i| self.has_prefix(i, &self.typed)) {
            self.typed.clear();
            return HintInput::NoMatch;
        }
        let len = self.codes.first().map_or(0, Vec::len);
        if self.typed.len() >= len {
            let idx = (0..self.codes.len())
                .find(|&i| self.codes[i] == self.typed)
                .expect("a full-length prefix match is an exact match");
            self.typed.clear();
            return HintInput::Activate(idx);
        }
        HintInput::Pending
    }

    /// Whether `hints[idx]`'s code still matches the typed buffer (its prefix) —
    /// the renderer fades the rest. True for every hint when nothing is typed.
    pub fn matches_prefix(&self, idx: usize) -> bool {
        self.typed.is_empty() || self.has_prefix(idx, &self.typed)
    }

    fn has_prefix(&self, idx: usize, prefix: &[Sym]) -> bool {
        self.codes.get(idx).is_some_and(|c| c.starts_with(prefix))
    }

    /// The combo code assigned to `hints[idx]` this round (for the renderer).
    pub fn code(&self, idx: usize) -> &[Sym] {
        self.codes.get(idx).map(Vec::as_slice).unwrap_or_default()
    }

    /// Symbols typed so far toward a code (the renderer dims these leading cells).
    pub fn typed(&self) -> &[Sym] {
        &self.typed
    }

    /// Whether a partial combo is buffered (B clears it before exiting the mode).
    pub fn has_typed(&self) -> bool {
        !self.typed.is_empty()
    }

    /// Drop a partially-typed combo (B with a non-empty buffer).
    pub fn clear_typed(&mut self) {
        self.typed.clear();
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
        let d2 = dist2(hint.center(), p);
        if d2 < best.0 {
            best = (d2, i);
        }
    }
    best.1
}

/// Squared distance between two points (ordering only, so the root is needless).
fn dist2(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)
}

/// Assign each hint a uniform-length code, low codes to the hints nearest
/// `anchor` (the viewport top-center) so the obvious targets take the fewest
/// presses. With 8 symbols, 1 press covers <= 8 hints, 2 <= 64, 3 <= 512 — past
/// the 150-hint collection cap, so the length is never more than 3.
fn assign_codes(hints: &[Hint], anchor: (f32, f32)) -> Vec<Vec<Sym>> {
    let n = hints.len();
    let len = if n <= 8 {
        1
    } else if n <= 64 {
        2
    } else {
        3
    };
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| dist2(hints[a].center(), anchor).total_cmp(&dist2(hints[b].center(), anchor)));
    let mut codes = vec![Vec::new(); n];
    for (rank, &idx) in order.iter().enumerate() {
        codes[idx] = nth_code(rank, len);
    }
    codes
}

/// The `rank`-th code of `len` symbols: base-8 over [`Sym::ALL`], most-
/// significant symbol first, so codes enumerate in a stable order.
fn nth_code(mut rank: usize, len: usize) -> Vec<Sym> {
    let base = Sym::ALL.len();
    let mut code = vec![Sym::X; len];
    for slot in code.iter_mut().rev() {
        *slot = Sym::ALL[rank % base];
        rank /= base;
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// A hint at a point (size zero, so its center is exactly that point).
    fn at(x: f32, y: f32) -> Hint {
        Hint {
            x,
            y,
            w: 0.0,
            h: 0.0,
            url: None,
        }
    }

    fn shown(centers: &[(f32, f32)], anchor: (f32, f32)) -> Hints {
        let mut h = Hints::new();
        h.begin_collect();
        let hints = centers.iter().map(|&(x, y)| at(x, y)).collect();
        h.show(hints, anchor, anchor);
        h
    }

    #[test]
    fn code_length_scales_with_count() {
        let len_for = |n: usize| {
            let hints: Vec<Hint> = (0..n).map(|i| at(i as f32, 0.0)).collect();
            assign_codes(&hints, (0.0, 0.0))[0].len()
        };
        assert_eq!(len_for(8), 1);
        assert_eq!(len_for(9), 2);
        assert_eq!(len_for(64), 2);
        assert_eq!(len_for(65), 3);
    }

    #[test]
    fn codes_are_unique() {
        let hints: Vec<Hint> = (0..50).map(|i| at(i as f32, 0.0)).collect();
        let codes = assign_codes(&hints, (0.0, 0.0));
        let unique: HashSet<&Vec<Sym>> = codes.iter().collect();
        assert_eq!(unique.len(), codes.len());
    }

    #[test]
    fn nearest_hint_gets_the_first_code() {
        // The anchor sits on the last hint, which should win rank 0 (code `X`).
        let codes = assign_codes(&[at(100.0, 0.0), at(50.0, 0.0), at(0.0, 0.0)], (0.0, 0.0));
        assert_eq!(codes[2], vec![Sym::X]);
    }

    #[test]
    fn push_sym_narrows_then_activates() {
        // 10 hints -> length-2 codes; the nearest (index 0) is `XX`.
        let mut h = shown(
            &(0..10).map(|i| (i as f32, 0.0)).collect::<Vec<_>>(),
            (0.0, 0.0),
        );
        assert_eq!(h.code(0), [Sym::X, Sym::X]);
        assert!(matches!(h.push_sym(Sym::X), HintInput::Pending));
        assert!(h.has_typed());
        assert!(matches!(h.push_sym(Sym::X), HintInput::Activate(0)));
        assert!(!h.has_typed()); // cleared on activate
    }

    #[test]
    fn push_sym_dead_end_is_nomatch() {
        // Only ranks 0..10 are assigned, none starting with `Right`.
        let mut h = shown(
            &(0..10).map(|i| (i as f32, 0.0)).collect::<Vec<_>>(),
            (0.0, 0.0),
        );
        assert!(matches!(h.push_sym(Sym::Right), HintInput::NoMatch));
        assert!(!h.has_typed());
    }

    #[test]
    fn matches_prefix_tracks_the_buffer() {
        let mut h = shown(
            &(0..10).map(|i| (i as f32, 0.0)).collect::<Vec<_>>(),
            (0.0, 0.0),
        );
        assert!(h.matches_prefix(0)); // nothing typed: everything matches
        h.push_sym(Sym::X);
        assert!(h.matches_prefix(0)); // code `XX` still matches `X`
        assert!(!h.matches_prefix(8)); // code `YX` no longer matches
    }
}
