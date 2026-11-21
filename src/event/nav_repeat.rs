//! Held-stick overlay navigation: a stick vector becomes the discrete `Nav`
//! steps the keyboard arrows emit, one on the push and then at a steady pace
//! while the stick is held.

use std::time::{Duration, Instant};

/// Where held-stick steps start and how fast they repeat (`[controls] nav_*`).
pub struct Pace {
    /// Deflection below which the stick counts as centred.
    pub threshold: f32,
    /// The wait between the first step and the first repeat.
    pub initial: Duration,
    /// The wait between repeats.
    pub repeat: Duration,
}

/// Auto-repeat state of the held direction.
pub struct NavRepeat {
    dir: (i32, i32),
    /// When the held direction steps again; read only while one is held.
    next: Instant,
}

impl NavRepeat {
    pub fn new() -> Self {
        Self {
            dir: (0, 0),
            next: Instant::now(),
        }
    }

    /// Reads one frame's stick vector; returns the step to fire this frame, if any.
    pub fn step(&mut self, v: (f32, f32), pace: &Pace, now: Instant) -> Option<(i32, i32)> {
        let dir = dominant_step(v, pace.threshold);
        if dir != self.dir {
            self.dir = dir;
            self.next = now + pace.initial;
            return Some(dir).filter(|&d| d != (0, 0));
        }
        if dir != (0, 0) && now >= self.next {
            self.next = now + pace.repeat;
            return Some(dir);
        }
        None
    }
}

/// Reduce a stick vector to a single discrete grid step along its dominant axis,
/// or `(0, 0)` when the stick is within the navigation dead zone (`threshold`).
fn dominant_step(v: (f32, f32), threshold: f32) -> (i32, i32) {
    if v.0.abs().max(v.1.abs()) < threshold {
        (0, 0)
    } else if v.0.abs() >= v.1.abs() {
        (v.0.signum() as i32, 0)
    } else {
        (0, v.1.signum() as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACE: Pace = Pace {
        threshold: 0.5,
        initial: Duration::from_millis(300),
        repeat: Duration::from_millis(100),
    };
    const DOWN: (f32, f32) = (0.0, 1.0);

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn a_push_steps_at_once() {
        let t = Instant::now();
        assert_eq!(NavRepeat::new().step(DOWN, &PACE, t), Some((0, 1)));
    }

    #[test]
    fn a_held_stick_repeats_after_the_initial_wait_then_at_the_pace() {
        let (mut nav, t) = (NavRepeat::new(), Instant::now());
        nav.step(DOWN, &PACE, t);
        assert_eq!(nav.step(DOWN, &PACE, t + ms(299)), None);
        assert_eq!(nav.step(DOWN, &PACE, t + ms(300)), Some((0, 1)));
        assert_eq!(nav.step(DOWN, &PACE, t + ms(399)), None);
        assert_eq!(nav.step(DOWN, &PACE, t + ms(400)), Some((0, 1)));
    }

    #[test]
    fn letting_go_rearms_the_push() {
        let (mut nav, t) = (NavRepeat::new(), Instant::now());
        nav.step(DOWN, &PACE, t);
        assert_eq!(nav.step((0.0, 0.0), &PACE, t + ms(10)), None);
        assert_eq!(nav.step(DOWN, &PACE, t + ms(20)), Some((0, 1)));
    }

    #[test]
    fn a_new_direction_steps_at_once() {
        let (mut nav, t) = (NavRepeat::new(), Instant::now());
        nav.step(DOWN, &PACE, t);
        assert_eq!(nav.step((-1.0, 0.0), &PACE, t + ms(10)), Some((-1, 0)));
    }

    #[test]
    fn a_stick_inside_the_dead_zone_steps_nothing() {
        let t = Instant::now();
        assert_eq!(NavRepeat::new().step((0.3, -0.4), &PACE, t), None);
    }

    #[test]
    fn a_diagonal_steps_along_its_dominant_axis() {
        let t = Instant::now();
        assert_eq!(NavRepeat::new().step((0.6, -0.9), &PACE, t), Some((0, -1)));
        assert_eq!(NavRepeat::new().step((0.8, 0.8), &PACE, t), Some((1, 0)));
    }
}
