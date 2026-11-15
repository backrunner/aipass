//! CSS-compatible timing shared by the native transition implementation and
//! frame-level comparison. Coordinates are normalized to viewport height.

pub const UNLOCK_DURATION_MS: f64 = 1200.0;
pub const COVERED_AT_MS: f64 = 540.0;
pub const UNCOVER_AT_MS: f64 = 660.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Unlock,
    Lock,
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct Milestones {
    pub covered: bool,
    pub done: bool,
}

/// One run's event state. Replacing this value cancels the old run without
/// retaining callbacks that could reveal a later session.
#[derive(Default)]
pub struct Timeline {
    covered: bool,
    done: bool,
}

impl Timeline {
    pub fn advance(&mut self, elapsed_ms: f64, reduced: bool) -> Milestones {
        let covered = !self.covered && (reduced || elapsed_ms >= COVERED_AT_MS);
        let done = !self.done && (reduced || elapsed_ms >= UNLOCK_DURATION_MS);
        self.covered |= covered;
        self.done |= done;
        Milestones { covered, done }
    }
}

fn coordinate(t: f64, first: f64, second: f64) -> f64 {
    3.0 * (1.0 - t).powi(2) * t * first + 3.0 * (1.0 - t) * t * t * second + t.powi(3)
}

/// CSS timing uses x as elapsed time, not the Bezier parameter t.
/// Bisection also handles zero endpoint derivatives without division by zero.
pub fn cubic_bezier(time: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let time = time.clamp(0.0, 1.0);
    if time == 0.0 || time == 1.0 {
        return time;
    }
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..48 {
        let mid = (low + high) / 2.0;
        if coordinate(mid, x1, x2) < time {
            low = mid;
        } else {
            high = mid;
        }
    }
    coordinate((low + high) / 2.0, y1, y2)
}

pub fn unlock_offset(elapsed_ms: f64, down: bool, reduced_motion: bool) -> f64 {
    let value = if reduced_motion || elapsed_ms >= UNLOCK_DURATION_MS {
        -1.0
    } else if elapsed_ms < COVERED_AT_MS {
        1.1 * (1.0 - cubic_bezier(elapsed_ms / COVERED_AT_MS, 0.22, 1.0, 0.36, 1.0))
    } else if elapsed_ms <= UNCOVER_AT_MS {
        0.0
    } else {
        -cubic_bezier(
            (elapsed_ms - UNCOVER_AT_MS) / (UNLOCK_DURATION_MS - UNCOVER_AT_MS),
            0.64,
            0.0,
            0.78,
            0.0,
        )
    };
    if down {
        -value
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_hold_and_exit_preserve_css_boundaries() {
        assert_eq!(unlock_offset(0.0, false, false), 1.1);
        assert_eq!(unlock_offset(COVERED_AT_MS, false, false), 0.0);
        assert_eq!(unlock_offset(600.0, false, false), 0.0);
        assert_eq!(unlock_offset(UNCOVER_AT_MS, false, false), 0.0);
        assert_eq!(unlock_offset(UNLOCK_DURATION_MS, false, false), -1.0);
        assert_eq!(unlock_offset(0.0, false, true), -1.0);
    }

    #[test]
    fn time_to_bezier_mapping_is_inverted() {
        // At t=.5 this curve has x=.3425, y=.875, not y=.5.
        let x = coordinate(0.5, 0.22, 0.36);
        assert!((cubic_bezier(x, 0.22, 1.0, 0.36, 1.0) - 0.875).abs() < 1e-12);
    }

    #[test]
    fn lock_is_mirrored_and_motion_does_not_reverse() {
        let mut previous = 1.1;
        for ms in 0..=1200 {
            let offset = unlock_offset(f64::from(ms), false, false);
            assert!(offset <= previous + 1e-12);
            assert_eq!(offset, -unlock_offset(f64::from(ms), true, false));
            previous = offset;
        }
    }

    #[test]
    fn milestones_are_once_only_even_when_frames_are_skipped() {
        let mut timeline = Timeline::default();
        assert_eq!(timeline.advance(539.0, false), Milestones::default());
        assert_eq!(
            timeline.advance(900.0, false),
            Milestones {
                covered: true,
                done: false
            }
        );
        assert_eq!(
            timeline.advance(1200.0, false),
            Milestones {
                covered: false,
                done: true
            }
        );
        assert_eq!(timeline.advance(2000.0, false), Milestones::default());
        let mut skipped = Timeline::default();
        assert_eq!(
            skipped.advance(1400.0, false),
            Milestones {
                covered: true,
                done: true
            }
        );
    }

    #[test]
    fn reduced_motion_completes_in_order_and_reversal_has_fresh_milestones() {
        let mut timeline = Timeline::default();
        assert_eq!(
            timeline.advance(0.0, true),
            Milestones {
                covered: true,
                done: true
            }
        );
        assert_eq!(timeline.advance(0.0, true), Milestones::default());
        let mut replacement = Timeline::default();
        assert_eq!(replacement.advance(0.0, false), Milestones::default());
        assert!(replacement.advance(540.0, false).covered);
    }
}
