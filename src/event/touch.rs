//! Touch over the web-content area. [`PageTouches`] tracks the fingers handed
//! to the page, which the engine pans, flings, pinches and clicks with;
//! [`TouchState`] tells a tap from a drag on the first finger, and on e-ink also
//! drives the plain drag-to-scroll and tap-to-click. On Android, SDL's
//! touch→mouse synthesis is disabled (see [`crate::run_app`]) so it can't fire
//! a phantom click at the end of a scroll; egui handles touch for the chrome
//! (toolbar/overlays) on its own via its `on_touch`, so this only drives the
//! browser.
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

/// Fingers handed to the page. Points are taken against the web view's origin
/// as it stood when the first of them landed, so chrome moving mid-gesture does
/// not jump them.
#[derive(Default)]
pub struct PageTouches {
    fingers: Vec<i64>,
    origin: (f32, f32),
}

impl PageTouches {
    /// Hand `finger` to the page; returns its web-view point. `at` and `origin`
    /// (the web view's top-left now) are in points.
    pub fn down(&mut self, finger: i64, at: (f32, f32), origin: (f32, f32)) -> (f32, f32) {
        if self.fingers.is_empty() {
            self.origin = origin;
        }
        if !self.fingers.contains(&finger) {
            self.fingers.push(finger);
        }
        self.point(at)
    }

    /// The web-view point of `finger` at `at` (points); `None` for a finger the
    /// page does not hold.
    pub fn motion(&self, finger: i64, at: (f32, f32)) -> Option<(f32, f32)> {
        self.fingers.contains(&finger).then(|| self.point(at))
    }

    /// Release `finger`; returns its last web-view point, `None` as in [`Self::motion`].
    pub fn up(&mut self, finger: i64, at: (f32, f32)) -> Option<(f32, f32)> {
        let i = self.fingers.iter().position(|&f| f == finger)?;
        self.fingers.swap_remove(i);
        Some(self.point(at))
    }

    fn point(&self, (x, y): (f32, f32)) -> (f32, f32) {
        (x - self.origin.0, y - self.origin.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The origin of the first finger holds for the whole gesture, and is
    /// taken afresh once every finger has lifted.
    #[test]
    fn page_points_keep_the_origin_the_gesture_started_with() {
        let mut touches = PageTouches::default();
        assert_eq!(touches.down(1, (50.0, 100.0), (0.0, 40.0)), (50.0, 60.0));
        assert_eq!(touches.down(2, (80.0, 100.0), (0.0, 0.0)), (80.0, 60.0));
        assert_eq!(touches.motion(1, (50.0, 90.0)), Some((50.0, 50.0)));
        assert_eq!(touches.up(1, (50.0, 90.0)), Some((50.0, 50.0)));
        assert_eq!(touches.up(2, (80.0, 100.0)), Some((80.0, 60.0)));
        assert_eq!(touches.down(3, (50.0, 100.0), (0.0, 0.0)), (50.0, 100.0));
    }

    /// A finger that landed on the chrome never reaches the page.
    #[test]
    fn a_finger_the_page_does_not_hold_is_not_forwarded() {
        let mut touches = PageTouches::default();
        touches.down(1, (10.0, 10.0), (0.0, 0.0));
        assert_eq!(touches.motion(7, (20.0, 20.0)), None);
        assert_eq!(touches.up(7, (20.0, 20.0)), None);
        assert_eq!(touches.up(1, (10.0, 10.0)), Some((10.0, 10.0)));
        assert_eq!(touches.up(1, (10.0, 10.0)), None);
    }
}
