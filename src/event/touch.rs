//! Single-finger touch gestures for the web-content area: a drag scrolls the
//! page, a tap clicks it. On Android, SDL's touch→mouse synthesis is disabled
//! (see [`crate::run_app`]) so it can't fire a phantom click at the end of a
//! scroll; egui handles touch for the chrome (toolbar/overlays) on its own via
//! its `on_touch`, so this only drives the browser.
//!
//! SDL finger coordinates are normalized to the window (`0.0..=1.0`); callers
//! convert to pixels before handing positions here.

/// Movement (px) past which a press is treated as a scroll rather than a tap —
/// keeps tiny finger jitter on a tap from nudging the page or eating the click.
const TAP_SLOP: f32 = 12.0;

#[derive(Default)]
pub struct TouchState {
    active: Option<Active>,
}

struct Active {
    finger: i64,
    last: (f32, f32),
    start: (f32, f32),
    /// Accumulated travel distance; once it passes [`TAP_SLOP`] the gesture is a
    /// scroll and the release won't click.
    travel: f32,
}

/// What a finger-up resolved to.
pub enum TouchEnd {
    /// Released without dragging — a tap to click at this pixel position.
    Tap(f32, f32),
    /// A scroll/drag (or a non-tracked finger) — no click.
    None,
}

impl TouchState {
    pub fn new() -> Self {
        Self::default()
    }

    /// First finger down starts a gesture; extra fingers are ignored (no
    /// multi-touch yet).
    pub fn down(&mut self, finger: i64, x: f32, y: f32) {
        if self.active.is_none() {
            self.active = Some(Active {
                finger,
                last: (x, y),
                start: (x, y),
                travel: 0.0,
            });
        }
    }

    /// The scroll delta (px) for this motion, once the gesture has passed the tap
    /// slop; `None` for the tracked finger's initial jitter or other fingers.
    pub fn motion(&mut self, finger: i64, x: f32, y: f32) -> Option<(f32, f32)> {
        let a = self.active.as_mut()?;
        if a.finger != finger {
            return None;
        }
        let (dx, dy) = (x - a.last.0, y - a.last.1);
        a.last = (x, y);
        let from_start = ((x - a.start.0).powi(2) + (y - a.start.1).powi(2)).sqrt();
        a.travel = a.travel.max(from_start);
        (a.travel > TAP_SLOP).then_some((dx, dy))
    }

    /// Resolve the gesture: a [`TouchEnd::Tap`] at the press point if the finger
    /// never passed the slop, else [`TouchEnd::None`].
    pub fn up(&mut self, finger: i64) -> TouchEnd {
        match self.active.take() {
            Some(a) if a.finger == finger && a.travel <= TAP_SLOP => {
                TouchEnd::Tap(a.start.0, a.start.1)
            }
            _ => TouchEnd::None,
        }
    }
}
