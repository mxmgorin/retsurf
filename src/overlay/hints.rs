//! Link-hint navigation state — Vimium adapted for a gamepad. L3 asks the page
//! for its visible clickable elements (collected via JavaScript, see
//! [`crate::browser`]); codes are drawn over them ([`crate::ui`]) and the stick
//! hops the selection spatially (nearest element in the pressed direction)
//! instead of typing hint letters. A clicks the selected element, B exits.
//! Scrolling keeps working and schedules a re-collect once it settles, since
//! the rects are viewport-relative and go stale as the page moves; while they
//! are stale nothing is drawn and nothing can be activated, because a hint
//! would mark whatever moved into its place. Pushing the selection past the last
//! hint at a vertical edge scrolls a chunk and re-collects, so one stick reaches
//! the whole document.

use crate::command::InputCommand;
use std::time::{Duration, Instant};

/// How long after the last scroll input before the hints are re-collected.
const REFRESH_DEBOUNCE: Duration = Duration::from_millis(150);

/// How much of the viewport a hint-mode edge auto-scroll covers — a chunk shy of
/// a full screen, so a strip of the old hints stays on-screen for continuity.
const EDGE_SCROLL_FRACTION: f32 = 0.8;

/// What hint mode makes of an [`InputCommand`].
#[derive(PartialEq, Debug)]
pub enum Reading {
    /// Not hint mode's: the caller gives it the bare page's meaning.
    Pass,
    /// Taken with nothing left for the caller to do.
    Taken,
    /// Hint mode is left; click the page at this browser-relative point.
    Click((f32, f32)),
    /// Hint mode is left; load this link in a background tab.
    OpenInBackground(String),
    /// Scroll the page by `dy`, hit-tested at `at` (browser-relative).
    Scroll { dy: f32, at: (f32, f32) },
}

/// The app state [`Hints::read`] consults.
pub struct Context {
    /// How long A/Enter is held to open a link in the background.
    pub hold: Duration,
    pub now: Instant,
    /// The browser area's width and height (logical px).
    pub area: (f32, f32),
    /// The cursor, browser-relative: the scroll anchor with no hint selected.
    pub cursor: (f32, f32),
}

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
/// on-screen code order.
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

/// One cell of a hint code: a gamepad button symbol (controller combos) or a
/// keyboard key (typed letters). Which alphabet a round uses is fixed by the
/// input that opened hint mode — see [`HintLabels`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Label {
    Sym(Sym),
    Key(char),
}

/// The alphabet a hint round draws its codes from, chosen from whatever opened
/// the mode: a controller types button combos, a keyboard types letters.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HintLabels {
    Gamepad,
    Keyboard,
}

/// Keyboard label letters, home-row-first so the common (short) codes sit under
/// the fingers — the Vimium default set. 14 letters keeps codes <= 2 cells for
/// the whole 150-hint collection cap (14^2 = 196).
const KEY_ALPHABET: [char; 14] = [
    's', 'a', 'd', 'f', 'j', 'k', 'l', 'e', 'w', 'c', 'm', 'p', 'g', 'h',
];

impl HintLabels {
    /// The ordered symbol set for this source; index 0 takes the shortest codes.
    fn alphabet(self) -> Vec<Label> {
        match self {
            HintLabels::Gamepad => Sym::ALL.iter().copied().map(Label::Sym).collect(),
            HintLabels::Keyboard => KEY_ALPHABET.iter().copied().map(Label::Key).collect(),
        }
    }
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
    /// Whether hint mode draws codes; off = plain spatial hops.
    pub show_codes: bool,
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
    /// Which alphabet this round's codes use (gamepad combos vs typed letters),
    /// fixed when the mode is entered and kept across post-scroll re-collects.
    labels: HintLabels,
    /// Typed combo code per hint (parallel to `hints`), reassigned every collect.
    codes: Vec<Vec<Label>>,
    /// Symbols pressed so far toward a code; cleared on activate or re-collect.
    typed: Vec<Label>,
    /// When A/Enter went down on a hint, for the tap-vs-hold split (tap clicks
    /// the hint, hold opens its link in a background tab). `None` when no press
    /// is in flight over a hint.
    press_at: Option<Instant>,
}

impl Hints {
    pub fn new(show_codes: bool) -> Self {
        Self {
            visible: false,
            show_codes,
            hints: vec![],
            selected: 0,
            collecting: false,
            refresh_at: None,
            next_near: None,
            labels: HintLabels::Gamepad,
            codes: vec![],
            typed: vec![],
            press_at: None,
        }
    }

    /// A collection round was started; the results arrive asynchronously.
    /// `labels` fixes the code alphabet for the whole session (the input that
    /// opened the mode) — a re-collect via [`Self::take_refresh_due`] keeps it.
    pub fn begin_collect(&mut self, labels: HintLabels) {
        self.collecting = true;
        self.labels = labels;
    }

    /// Whether this round's codes are typed on a keyboard (vs gamepad combos).
    pub fn is_keyboard(&self) -> bool {
        self.labels == HintLabels::Keyboard
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
        self.codes = assign_codes(&hints, order_from, &self.labels.alphabet());
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
        self.press_at = None;
    }

    /// Reads one input intent in hint mode; returns what the caller still has to do.
    pub fn read(&mut self, command: &InputCommand, cx: &Context) -> Reading {
        match command {
            // Tap vs hold on the selected hint: the press starts the clock, and a
            // hold opens the link in a background tab.
            InputCommand::Confirm(true) => {
                self.press_at = Some(cx.now);
                Reading::Taken
            }
            InputCommand::Confirm(false) => self.release(cx),
            // B drops a half-typed combo first, then exits hint mode.
            InputCommand::Cancel => {
                if self.has_typed() {
                    self.clear_typed();
                } else {
                    self.hide();
                }
                Reading::Taken
            }
            InputCommand::Nav(dx, dy) => self.nav((*dx, *dy), cx),
            // With combos on, X, Y, L1/R1 and the D-pad type symbols; with them
            // off, Y leaves hint mode and the rest pass to the page.
            InputCommand::ToggleOsk if self.show_codes => self.push(Label::Sym(Sym::X)),
            InputCommand::Hints if self.show_codes => self.push(Label::Sym(Sym::Y)),
            InputCommand::Hints => {
                self.hide();
                Reading::Taken
            }
            InputCommand::Shoulder(delta) if self.show_codes => {
                self.push(Label::Sym(if *delta < 0 { Sym::L1 } else { Sym::R1 }))
            }
            InputCommand::DpadPress(dx, dy) if self.show_codes => match dpad_sym(*dx, *dy) {
                Some(sym) => self.push(Label::Sym(sym)),
                None => Reading::Taken,
            },
            InputCommand::HintKey(c) if self.show_codes => self.push(Label::Key(*c)),
            InputCommand::ToggleOsk
            | InputCommand::Shoulder(_)
            | InputCommand::DpadPress(..)
            | InputCommand::HintKey(_)
            | InputCommand::Trigger { .. }
            | InputCommand::Zoom(_)
            | InputCommand::Osk(_)
            | InputCommand::CycleTab(_)
            | InputCommand::Click { .. }
            | InputCommand::OskButton(_)
            | InputCommand::Analog(_) => Reading::Pass,
        }
    }

    /// A/Enter released: a tap clicks the selected hint, a hold opens its link.
    fn release(&mut self, cx: &Context) -> Reading {
        let pressed_at = self.press_at.take();
        // Nothing is on screen to have aimed at, and the selection no longer sits
        // where the page does.
        if self.is_stale() {
            return Reading::Taken;
        }
        let held_long = pressed_at.is_some_and(|t| cx.now.saturating_duration_since(t) >= cx.hold);
        match self.selected_url().filter(|_| held_long).map(str::to_owned) {
            Some(url) => {
                self.hide();
                Reading::OpenInBackground(url)
            }
            None => self.activate(),
        }
    }

    /// Hint-mode directional input: hop the selection, or scroll a chunk and
    /// re-collect when it is already at a vertical edge. Horizontal edges stay
    /// put — pages rarely scroll sideways.
    fn nav(&mut self, dir: (i32, i32), cx: &Context) -> Reading {
        let (width, height) = cx.area;
        if self.move_sel(dir) || dir.1 == 0 || height <= 0.0 {
            return Reading::Taken;
        }
        // The scroll is hit-tested to pick its scroller: the hint's column, at
        // mid-viewport, because an edge hint often sits in a sticky header.
        let (sx, _) = self.anchor(cx.cursor);
        let sx = sx.clamp(1.0, (width - 1.0).max(1.0));
        // dy > 0 = down: reveal lower content (positive Servo dy) and re-anchor
        // the selection to the bottom edge; up is the mirror.
        let dy = dir.1 as f32 * height * EDGE_SCROLL_FRACTION;
        let edge_y = if dir.1 > 0 { height } else { 0.0 };
        self.mark_stale_at((sx, edge_y));
        Reading::Scroll {
            dy,
            at: (sx, height / 2.0),
        }
    }

    /// Feeds one combo symbol or typed letter; a code it completes clicks its hint.
    fn push(&mut self, label: Label) -> Reading {
        // Stale rects show no codes, so a press types nothing.
        if self.is_stale() {
            return Reading::Taken;
        }
        match self.push_label(label) {
            HintInput::Activate(idx) => {
                self.select(idx);
                self.activate()
            }
            HintInput::Pending | HintInput::NoMatch => Reading::Taken,
        }
    }

    /// Leaves hint mode; returns the click on the selected hint, if there is one.
    fn activate(&mut self) -> Reading {
        let center = self.selected_center();
        self.hide();
        center.map_or(Reading::Taken, Reading::Click)
    }

    pub fn hints(&self) -> &[Hint] {
        &self.hints
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    fn selected_center(&self) -> Option<(f32, f32)> {
        self.hints.get(self.selected).map(Hint::center)
    }

    /// The point hint mode works from: the selected hint's center, else `cursor`.
    /// Scrolls hit-test it and a re-collect selects near it, so all of them agree.
    pub fn anchor(&self, cursor: (f32, f32)) -> (f32, f32) {
        self.selected_center().unwrap_or(cursor)
    }

    /// The selected hint's link URL, if it is a link (`None` for buttons etc.).
    pub fn selected_url(&self) -> Option<&str> {
        self.hints.get(self.selected)?.url.as_deref()
    }

    /// Point the selection at `idx` (a combo resolved to this hint) so the click
    /// targets it. No-op if stale.
    pub fn select(&mut self, idx: usize) {
        if idx < self.hints.len() {
            self.selected = idx;
        }
    }

    /// Feed one label (a gamepad symbol or a typed letter) into the buffer (see
    /// [`HintInput`]). Codes are uniform-length and unique, so a full buffer that
    /// still prefixes a code is an exact, single match.
    pub fn push_label(&mut self, s: Label) -> HintInput {
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

    fn has_prefix(&self, idx: usize, prefix: &[Label]) -> bool {
        self.codes.get(idx).is_some_and(|c| c.starts_with(prefix))
    }

    /// The combo code assigned to `hints[idx]` this round (for the renderer).
    pub fn code(&self, idx: usize) -> &[Label] {
        self.codes.get(idx).map(Vec::as_slice).unwrap_or_default()
    }

    /// Symbols typed so far toward a code (the renderer dims these leading cells).
    pub fn typed(&self) -> &[Label] {
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
    /// `false` means the edge (no hint further that way) — [`Self::read`] turns a
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

    /// Scrolling shifts the page under the hints: schedule a re-collect for
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

    /// Whether the rects no longer describe the page: it has scrolled under them
    /// and the re-collect they scheduled has yet to land. The renderer draws
    /// nothing meanwhile, and the router lets no input aim at them.
    pub fn is_stale(&self) -> bool {
        self.refresh_at.is_some() || self.collecting
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

/// Map a discrete D-pad press direction to its combo symbol (hint mode).
fn dpad_sym(dx: i32, dy: i32) -> Option<Sym> {
    Some(match (dx, dy) {
        (0, -1) => Sym::Up,
        (0, 1) => Sym::Down,
        (-1, 0) => Sym::Left,
        (1, 0) => Sym::Right,
        _ => return None,
    })
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

/// Assign each hint a uniform-length code from `alphabet`, low codes to the hints
/// nearest `anchor` (the viewport top-center) so the obvious targets take the
/// fewest presses. The length is the smallest that gives every hint a unique code
/// (`base^len >= n`): the 8 gamepad symbols need <= 3 cells, the 14 keyboard
/// letters <= 2, across the 150-hint collection cap.
fn assign_codes(hints: &[Hint], anchor: (f32, f32), alphabet: &[Label]) -> Vec<Vec<Label>> {
    let n = hints.len();
    let len = code_len(n, alphabet.len());
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        dist2(hints[a].center(), anchor).total_cmp(&dist2(hints[b].center(), anchor))
    });
    let mut codes = vec![Vec::new(); n];
    for (rank, &idx) in order.iter().enumerate() {
        codes[idx] = nth_code(rank, len, alphabet);
    }
    codes
}

/// Smallest code length whose `base`-symbol space holds `n` unique codes.
fn code_len(n: usize, base: usize) -> usize {
    let base = base.max(2); // a 0/1-symbol alphabet can't enumerate; guard anyway
    let mut len = 1;
    let mut cap = base;
    while cap < n {
        cap = cap.saturating_mul(base);
        len += 1;
    }
    len
}

/// The `rank`-th code of `len` cells: base-`alphabet.len()` over `alphabet`,
/// most-significant cell first, so codes enumerate in a stable order.
fn nth_code(mut rank: usize, len: usize, alphabet: &[Label]) -> Vec<Label> {
    let base = alphabet.len();
    let mut code = vec![alphabet[0]; len];
    for slot in code.iter_mut().rev() {
        *slot = alphabet[rank % base];
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
        let mut h = Hints::new(true);
        h.begin_collect(HintLabels::Gamepad);
        let hints = centers.iter().map(|&(x, y)| at(x, y)).collect();
        h.show(hints, anchor, anchor);
        h
    }

    /// The gamepad alphabet (8 symbols), as the assignment helpers take it.
    fn pad() -> Vec<Label> {
        HintLabels::Gamepad.alphabet()
    }

    #[test]
    fn code_length_scales_with_count() {
        let len_for = |n: usize| {
            let hints: Vec<Hint> = (0..n).map(|i| at(i as f32, 0.0)).collect();
            assign_codes(&hints, (0.0, 0.0), &pad())[0].len()
        };
        assert_eq!(len_for(8), 1);
        assert_eq!(len_for(9), 2);
        assert_eq!(len_for(64), 2);
        assert_eq!(len_for(65), 3);
    }

    #[test]
    fn keyboard_codes_stay_short() {
        // 14 letters: 1 cell up to 14, 2 cells past it — the 150-hint cap is <= 2.
        let len_for = |n: usize| {
            let hints: Vec<Hint> = (0..n).map(|i| at(i as f32, 0.0)).collect();
            assign_codes(&hints, (0.0, 0.0), &HintLabels::Keyboard.alphabet())[0].len()
        };
        assert_eq!(len_for(14), 1);
        assert_eq!(len_for(15), 2);
        assert_eq!(len_for(150), 2);
    }

    #[test]
    fn codes_are_unique() {
        let hints: Vec<Hint> = (0..50).map(|i| at(i as f32, 0.0)).collect();
        let codes = assign_codes(&hints, (0.0, 0.0), &pad());
        let unique: HashSet<&Vec<Label>> = codes.iter().collect();
        assert_eq!(unique.len(), codes.len());
    }

    #[test]
    fn nearest_hint_gets_the_first_code() {
        // The anchor sits on the last hint, which should win rank 0 (code `X`).
        let codes = assign_codes(
            &[at(100.0, 0.0), at(50.0, 0.0), at(0.0, 0.0)],
            (0.0, 0.0),
            &pad(),
        );
        assert_eq!(codes[2], vec![Label::Sym(Sym::X)]);
    }

    #[test]
    fn push_narrows_then_activates() {
        // 10 hints -> length-2 codes; the nearest (index 0) is `XX`.
        let mut h = shown(
            &(0..10).map(|i| (i as f32, 0.0)).collect::<Vec<_>>(),
            (0.0, 0.0),
        );
        assert_eq!(h.code(0), [Label::Sym(Sym::X), Label::Sym(Sym::X)]);
        assert!(matches!(
            h.push_label(Label::Sym(Sym::X)),
            HintInput::Pending
        ));
        assert!(h.has_typed());
        assert!(matches!(
            h.push_label(Label::Sym(Sym::X)),
            HintInput::Activate(0)
        ));
        assert!(!h.has_typed()); // cleared on activate
    }

    #[test]
    fn push_dead_end_is_nomatch() {
        // Only ranks 0..10 are assigned, none starting with `Right`.
        let mut h = shown(
            &(0..10).map(|i| (i as f32, 0.0)).collect::<Vec<_>>(),
            (0.0, 0.0),
        );
        assert!(matches!(
            h.push_label(Label::Sym(Sym::Right)),
            HintInput::NoMatch
        ));
        assert!(!h.has_typed());
    }

    #[test]
    fn rects_are_stale_from_the_scroll_until_the_recollect_lands() {
        let mut h = shown(&[(10.0, 10.0)], (0.0, 0.0));
        assert!(!h.is_stale());
        h.mark_stale();
        assert!(h.is_stale());
        // Still stale while the round it scheduled runs: the page has moved and
        // the fresh rects have not arrived.
        h.begin_collect(HintLabels::Gamepad);
        assert!(h.is_stale());
        h.show(vec![at(10.0, 40.0)], (0.0, 0.0), (0.0, 0.0));
        assert!(!h.is_stale());
    }

    #[test]
    fn an_unfinished_round_leaves_nothing_stale_behind() {
        let mut h = shown(&[(10.0, 10.0)], (0.0, 0.0));
        h.mark_stale();
        h.hide();
        assert!(!h.is_stale());
    }

    #[test]
    fn matches_prefix_tracks_the_buffer() {
        let mut h = shown(
            &(0..10).map(|i| (i as f32, 0.0)).collect::<Vec<_>>(),
            (0.0, 0.0),
        );
        assert!(h.matches_prefix(0)); // nothing typed: everything matches
        h.push_label(Label::Sym(Sym::X));
        assert!(h.matches_prefix(0)); // code `XX` still matches `X`
        assert!(!h.matches_prefix(8)); // code `YX` no longer matches
    }

    const HOLD: Duration = Duration::from_millis(500);

    fn cx(now: Instant) -> Context {
        Context {
            hold: HOLD,
            now,
            area: (800.0, 600.0),
            cursor: (0.0, 0.0),
        }
    }

    fn link_at(x: f32, y: f32) -> Hints {
        let mut h = Hints::new(true);
        h.begin_collect(HintLabels::Gamepad);
        let link = Hint {
            url: Some("https://a.test/".to_string()),
            ..at(x, y)
        };
        h.show(vec![link], (0.0, 0.0), (0.0, 0.0));
        h
    }

    #[test]
    fn a_tap_clicks_the_selected_hint_and_leaves() {
        let mut h = link_at(10.0, 20.0);
        let t = Instant::now();
        assert_eq!(h.read(&InputCommand::Confirm(true), &cx(t)), Reading::Taken);
        assert_eq!(
            h.read(&InputCommand::Confirm(false), &cx(t)),
            Reading::Click((10.0, 20.0))
        );
        assert!(!h.visible);
    }

    #[test]
    fn a_hold_on_a_link_opens_it_in_the_background() {
        let mut h = link_at(10.0, 20.0);
        let t = Instant::now();
        h.read(&InputCommand::Confirm(true), &cx(t));
        assert_eq!(
            h.read(&InputCommand::Confirm(false), &cx(t + HOLD)),
            Reading::OpenInBackground("https://a.test/".to_string())
        );
        assert!(!h.visible);
    }

    #[test]
    fn a_hold_on_a_button_still_clicks() {
        let mut h = shown(&[(10.0, 20.0)], (0.0, 0.0));
        let t = Instant::now();
        h.read(&InputCommand::Confirm(true), &cx(t));
        assert_eq!(
            h.read(&InputCommand::Confirm(false), &cx(t + HOLD)),
            Reading::Click((10.0, 20.0))
        );
    }

    #[test]
    fn a_press_from_an_exited_round_is_no_hold() {
        let t = Instant::now();
        let mut h = link_at(10.0, 20.0);
        h.read(&InputCommand::Confirm(true), &cx(t));
        h.read(&InputCommand::Cancel, &cx(t));
        let link = Hint {
            url: Some("https://a.test/".to_string()),
            ..at(10.0, 20.0)
        };
        h.begin_collect(HintLabels::Gamepad);
        h.show(vec![link], (0.0, 0.0), (0.0, 0.0));
        assert_eq!(
            h.read(&InputCommand::Confirm(false), &cx(t + HOLD)),
            Reading::Click((10.0, 20.0))
        );
    }

    #[test]
    fn b_drops_a_half_typed_combo_before_leaving() {
        // 10 hints -> length-2 codes, so one X is a live prefix.
        let mut h = shown(
            &(0..10).map(|i| (i as f32, 0.0)).collect::<Vec<_>>(),
            (0.0, 0.0),
        );
        let t = Instant::now();
        assert_eq!(h.read(&InputCommand::ToggleOsk, &cx(t)), Reading::Taken);
        assert!(h.has_typed());
        h.read(&InputCommand::Cancel, &cx(t));
        assert!(h.visible && !h.has_typed());
        h.read(&InputCommand::Cancel, &cx(t));
        assert!(!h.visible);
    }

    #[test]
    fn a_typed_letter_code_clicks_its_hint() {
        let mut h = Hints::new(true);
        h.begin_collect(HintLabels::Keyboard);
        h.show(vec![at(10.0, 20.0)], (0.0, 0.0), (0.0, 0.0));
        let Label::Key(c) = h.code(0)[0] else {
            panic!("a keyboard round assigns letter codes");
        };
        assert_eq!(
            h.read(&InputCommand::HintKey(c), &cx(Instant::now())),
            Reading::Click((10.0, 20.0))
        );
    }

    #[test]
    fn stale_rects_take_presses_without_clicking() {
        // One hint: its code is the single symbol X.
        let mut h = shown(&[(10.0, 20.0)], (0.0, 0.0));
        h.mark_stale();
        let t = Instant::now();
        assert_eq!(h.read(&InputCommand::ToggleOsk, &cx(t)), Reading::Taken);
        h.read(&InputCommand::Confirm(true), &cx(t));
        assert_eq!(
            h.read(&InputCommand::Confirm(false), &cx(t)),
            Reading::Taken
        );
        assert!(h.visible);
    }

    #[test]
    fn nav_past_the_last_hint_down_scrolls_a_chunk() {
        let mut h = shown(&[(100.0, 300.0)], (0.0, 0.0));
        let reading = h.read(&InputCommand::Nav(0, 1), &cx(Instant::now()));
        assert!(matches!(
            reading,
            Reading::Scroll { dy, at: (100.0, 300.0) } if dy > 0.0 && dy < 600.0
        ));
        assert!(h.is_stale());
    }

    #[test]
    fn nav_past_the_last_hint_sideways_stays_put() {
        let mut h = shown(&[(100.0, 300.0)], (0.0, 0.0));
        let reading = h.read(&InputCommand::Nav(1, 0), &cx(Instant::now()));
        assert_eq!(reading, Reading::Taken);
        assert!(!h.is_stale());
    }

    #[test]
    fn with_combos_off_x_and_shoulders_pass_to_the_page() {
        let mut h = shown(&[(10.0, 20.0)], (0.0, 0.0));
        h.show_codes = false;
        let ctx = cx(Instant::now());
        assert_eq!(h.read(&InputCommand::ToggleOsk, &ctx), Reading::Pass);
        assert_eq!(h.read(&InputCommand::Shoulder(1), &ctx), Reading::Pass);
        assert_eq!(h.read(&InputCommand::Hints, &ctx), Reading::Taken);
        assert!(!h.visible);
    }
}
