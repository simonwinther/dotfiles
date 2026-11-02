use smithay_client_toolkit::seat::pointer::AxisScroll;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Scroll {
    remainder: f64,
    resolution: Option<Resolution>,
    last: Option<Instant>,
}
#[derive(Clone, Copy, PartialEq)]
enum Resolution {
    Wheel,
    Continuous,
}
impl Scroll {
    pub fn steps(&mut self, axis: AxisScroll) -> i32 {
        let movement = if axis.value120 != 0 {
            Some((Resolution::Wheel, axis.value120 as f64 / 120.0))
        } else if axis.discrete != 0 {
            Some((Resolution::Wheel, axis.discrete as f64))
        } else if axis.absolute.is_finite() && axis.absolute != 0.0 {
            Some((Resolution::Continuous, axis.absolute / 36.0))
        } else {
            None
        };
        let steps = if let Some((resolution, amount)) = movement {
            if self.resolution != Some(resolution) {
                self.remainder = 0.0;
            }
            self.resolution = Some(resolution);
            self.last = Some(Instant::now());
            self.remainder += amount;
            let steps = (self.remainder.abs() + 1e-9)
                .floor()
                .copysign(self.remainder) as i32;
            self.remainder -= steps as f64;
            steps
        } else {
            0
        };
        if axis.stop {
            self.remainder = 0.0;
            self.resolution = None;
        }
        steps.clamp(-8, 8)
    }
    pub fn active(&self) -> bool {
        self.last
            .is_some_and(|last| last.elapsed() < Duration::from_millis(120))
    }
}

/// Search fields retain plain j/k; only the editor-style control chords navigate.
pub fn navigation_key(
    key: smithay_client_toolkit::seat::keyboard::Keysym,
    ctrl: bool,
    alt: bool,
    logo: bool,
) -> smithay_client_toolkit::seat::keyboard::Keysym {
    use smithay_client_toolkit::seat::keyboard::Keysym;
    if ctrl && !alt && !logo {
        match key {
            Keysym::j | Keysym::J => Keysym::Down,
            Keysym::k | Keysym::K => Keysym::Up,
            Keysym::d | Keysym::D => Keysym::Next,
            Keysym::u | Keysym::U => Keysym::Prior,
            _ => key,
        }
    } else {
        key
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn control_navigation_does_not_consume_search_characters_or_other_modifiers() {
        use smithay_client_toolkit::seat::keyboard::Keysym;
        assert_eq!(
            super::navigation_key(Keysym::j, true, false, false),
            Keysym::Down
        );
        assert_eq!(
            super::navigation_key(Keysym::k, true, false, false),
            Keysym::Up
        );
        assert_eq!(
            super::navigation_key(Keysym::d, true, false, false),
            Keysym::Next
        );
        assert_eq!(
            super::navigation_key(Keysym::u, true, false, false),
            Keysym::Prior
        );
        assert_eq!(
            super::navigation_key(Keysym::d, false, false, false),
            Keysym::d
        );
        assert_eq!(
            super::navigation_key(Keysym::u, true, true, false),
            Keysym::u
        );
        assert_eq!(
            super::navigation_key(Keysym::j, false, false, false),
            Keysym::j
        );
        assert_eq!(
            super::navigation_key(Keysym::k, true, true, false),
            Keysym::k
        );
        assert_eq!(
            super::navigation_key(Keysym::k, true, false, true),
            Keysym::k
        );
    }
    use super::*;
    #[test]
    fn wheel_events_use_one_resolution_and_ignore_stop_frames() {
        let mut scroll = Scroll::default();
        assert_eq!(
            scroll.steps(AxisScroll {
                absolute: 15.0,
                discrete: 1,
                value120: 120,
                ..Default::default()
            }),
            1
        );
        assert_eq!(
            scroll.steps(AxisScroll {
                stop: true,
                ..Default::default()
            }),
            0
        );
        assert_eq!(scroll.steps(AxisScroll::default()), 0);
        assert_eq!(
            scroll.steps(AxisScroll {
                value120: -60,
                ..Default::default()
            }),
            0
        );
        assert_eq!(
            scroll.steps(AxisScroll {
                value120: -60,
                ..Default::default()
            }),
            -1
        );
    }
    #[test]
    fn touchpad_deltas_accumulate_and_stops_discard_the_remainder() {
        let mut scroll = Scroll::default();
        for _ in 0..5 {
            assert_eq!(
                scroll.steps(AxisScroll {
                    absolute: 6.0,
                    ..Default::default()
                }),
                0
            );
        }
        assert_eq!(
            scroll.steps(AxisScroll {
                absolute: 6.0,
                ..Default::default()
            }),
            1
        );
        assert_eq!(
            scroll.steps(AxisScroll {
                absolute: 20.0,
                ..Default::default()
            }),
            0
        );
        assert_eq!(
            scroll.steps(AxisScroll {
                stop: true,
                ..Default::default()
            }),
            0
        );
        assert_eq!(
            scroll.steps(AxisScroll {
                absolute: -18.0,
                ..Default::default()
            }),
            0
        );
        assert_eq!(
            scroll.steps(AxisScroll {
                absolute: -18.0,
                ..Default::default()
            }),
            -1
        );
    }

    #[test]
    fn slow_partial_steps_survive_pauses_without_mixing_device_resolutions() {
        let mut scroll = Scroll::default();
        assert!(!scroll.active());
        assert_eq!(
            scroll.steps(AxisScroll {
                value120: 60,
                ..Default::default()
            }),
            0
        );
        scroll.last = Some(Instant::now() - Duration::from_secs(1));
        assert_eq!(
            scroll.steps(AxisScroll {
                value120: 60,
                ..Default::default()
            }),
            1
        );
        assert_eq!(
            scroll.steps(AxisScroll {
                absolute: 18.0,
                ..Default::default()
            }),
            0
        );
        scroll.last = Some(Instant::now() - Duration::from_secs(1));
        assert_eq!(
            scroll.steps(AxisScroll {
                absolute: 18.0,
                ..Default::default()
            }),
            1
        );
        assert_eq!(
            scroll.steps(AxisScroll {
                value120: 60,
                ..Default::default()
            }),
            0
        );
        assert_eq!(
            scroll.steps(AxisScroll {
                absolute: 18.0,
                ..Default::default()
            }),
            0
        );
        assert_eq!(
            scroll.steps(AxisScroll {
                absolute: 18.0,
                ..Default::default()
            }),
            1
        );
    }
}
