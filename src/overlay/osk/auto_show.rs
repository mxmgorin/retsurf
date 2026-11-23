//! When the on-screen keyboard rises by itself (`[osk] auto_show`): on a text
//! field just focused, on a tap into the chrome's field, or on a tap into the
//! page's field once the page has had time to report where focus went.

use std::time::{Duration, Instant};

/// How long a page tap waits before raising the keyboard: the page reports a
/// blur only after the click reaches it.
const TAP_SETTLE: Duration = Duration::from_millis(300);

/// One frame's text focus and input, as [`AutoShow::read`] takes it.
pub struct Frame {
    /// A tap landed this frame.
    pub tapped: bool,
    /// A chrome text field has focus.
    pub chrome: bool,
    /// A page text field has focus.
    pub page: bool,
    /// The keyboard has focus.
    pub osk_open: bool,
    /// Home has focus.
    pub home: bool,
    /// No other keyboard types for the user (physical, or Android's own).
    pub needed: bool,
    pub now: Instant,
}

/// What a frame calls for.
#[derive(Default, PartialEq, Debug)]
pub struct Outcome {
    /// Raise the keyboard.
    pub show: bool,
    /// A page tap is waiting out its settle: the caller keeps frames coming.
    pub settling: bool,
}

/// The state the decision carries between frames.
pub struct AutoShow {
    /// Whether a chrome and a page text field had focus last frame.
    focused: (bool, bool),
    /// When a page tap began waiting out [`TAP_SETTLE`].
    tap_at: Option<Instant>,
}

impl AutoShow {
    pub fn new() -> Self {
        Self {
            focused: (false, false),
            tap_at: None,
        }
    }

    /// Reads one frame; returns whether to raise the keyboard and whether a tap
    /// is still settling.
    pub fn read(&mut self, frame: &Frame) -> Outcome {
        // Per side: focus passing straight from the chrome's field to the page's
        // leaves no frame without one.
        let (chrome, page) = std::mem::replace(&mut self.focused, (frame.chrome, frame.page));
        let rose = (frame.chrome && !chrome) || (frame.page && !page);
        if frame.osk_open {
            self.tap_at = None;
            return Outcome::default();
        }
        // egui moves focus within the pass that saw the tap; the page does not.
        if frame.tapped && frame.page && !frame.chrome {
            self.tap_at = Some(frame.now);
        }
        let (settled, settling) = match self.tap_at {
            Some(at) if frame.now.saturating_duration_since(at) >= TAP_SETTLE => {
                self.tap_at = None;
                (true, false)
            }
            Some(_) => (false, true),
            None => (false, false),
        };
        // Home's field is selected like its tiles: Confirm opens the
        // keyboard, or it would cover the grid on every visit.
        let show = frame.needed
            && ((rose && !frame.home) || (frame.tapped && frame.chrome) || (settled && frame.page));
        Outcome { show, settling }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame with nothing focused or tapped, where the keyboard is needed.
    fn idle(now: Instant) -> Frame {
        Frame {
            tapped: false,
            chrome: false,
            page: false,
            osk_open: false,
            home: false,
            needed: true,
            now,
        }
    }

    #[test]
    fn focusing_a_page_field_raises_the_keyboard_once() {
        let (mut auto, t) = (AutoShow::new(), Instant::now());
        let focused = Frame {
            page: true,
            ..idle(t)
        };
        assert!(auto.read(&focused).show);
        assert!(!auto.read(&focused).show);
    }

    #[test]
    fn focus_moving_from_the_chrome_to_the_page_raises_the_keyboard() {
        let (mut auto, t) = (AutoShow::new(), Instant::now());
        auto.read(&Frame {
            chrome: true,
            ..idle(t)
        });
        let moved = Frame {
            page: true,
            ..idle(t)
        };
        assert!(auto.read(&moved).show);
    }

    #[test]
    fn the_home_field_waits_for_confirm() {
        let (mut auto, t) = (AutoShow::new(), Instant::now());
        let focused = Frame {
            chrome: true,
            home: true,
            ..idle(t)
        };
        assert!(!auto.read(&focused).show);
    }

    #[test]
    fn a_tap_into_a_focused_chrome_field_raises_it_at_once() {
        let (mut auto, t) = (AutoShow::new(), Instant::now());
        auto.read(&Frame {
            chrome: true,
            ..idle(t)
        });
        let tap = Frame {
            chrome: true,
            tapped: true,
            ..idle(t)
        };
        assert!(auto.read(&tap).show);
    }

    #[test]
    fn a_tap_into_a_focused_page_field_raises_it_once_settled() {
        let (mut auto, t) = (AutoShow::new(), Instant::now());
        auto.read(&Frame {
            page: true,
            ..idle(t)
        });
        let tap = Frame {
            page: true,
            tapped: true,
            ..idle(t)
        };
        assert_eq!(
            auto.read(&tap),
            Outcome {
                show: false,
                settling: true
            }
        );
        let later = Frame {
            page: true,
            ..idle(t + TAP_SETTLE)
        };
        assert_eq!(
            auto.read(&later),
            Outcome {
                show: true,
                settling: false
            }
        );
    }

    #[test]
    fn a_page_tap_that_moved_focus_away_raises_nothing() {
        let (mut auto, t) = (AutoShow::new(), Instant::now());
        auto.read(&Frame {
            page: true,
            tapped: true,
            ..idle(t)
        });
        assert!(!auto.read(&idle(t + TAP_SETTLE)).show);
    }

    #[test]
    fn an_open_keyboard_drops_a_settling_tap() {
        let (mut auto, t) = (AutoShow::new(), Instant::now());
        auto.read(&Frame {
            page: true,
            ..idle(t)
        });
        auto.read(&Frame {
            page: true,
            tapped: true,
            ..idle(t)
        });
        auto.read(&Frame {
            page: true,
            osk_open: true,
            ..idle(t)
        });
        let later = Frame {
            page: true,
            ..idle(t + TAP_SETTLE)
        };
        assert!(!auto.read(&later).show);
    }

    #[test]
    fn another_keyboard_keeps_it_down() {
        let (mut auto, t) = (AutoShow::new(), Instant::now());
        let focused = Frame {
            page: true,
            needed: false,
            ..idle(t)
        };
        assert!(!auto.read(&focused).show);
    }
}
