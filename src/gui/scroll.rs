//! Convert device deltas into whole grid rows without amplifying tiny touchpad events.
//! GPUI/platform momentum is preserved; adding another inertia timer would double it.
use std::time::{Duration, Instant};

use gpui::ScrollDelta;

#[derive(Default)]
pub(super) struct WheelAccumulator {
    remainder: f32,
    last_event: Option<Instant>,
}

impl WheelAccumulator {
    pub fn reset(&mut self) {
        self.remainder = 0.0;
        self.last_event = None;
    }

    pub fn rows(&mut self, delta: ScrollDelta, row_height: f32) -> i32 {
        let rows = match delta {
            ScrollDelta::Lines(delta) => delta.y,
            ScrollDelta::Pixels(delta) => f32::from(delta.y) / row_height.max(1.0),
        };
        self.accumulate(rows, Instant::now())
    }

    fn accumulate(&mut self, rows: f32, now: Instant) -> i32 {
        if !rows.is_finite() || rows == 0.0 {
            return 0;
        }
        // A partial gesture must not affect a later gesture or resist a reversal.
        if self.last_event.is_some_and(|last| now.duration_since(last) > Duration::from_millis(250))
            || self.remainder.signum() != rows.signum()
        {
            self.remainder = 0.0;
        }
        self.last_event = Some(now);
        // Bound pathological device input, not normal wheel/trackpad acceleration.
        let total = self.remainder + rows.clamp(-120.0, 120.0);
        let whole = total.trunc() as i32;
        self.remainder = total - whole as f32;
        whole
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px};

    #[test]
    fn tiny_deltas_accumulate_instead_of_each_becoming_a_wheel_tick() {
        let mut wheel = WheelAccumulator::default();
        let now = Instant::now();
        for _ in 0..7 {
            assert_eq!(wheel.accumulate(0.125, now), 0);
        }
        assert_eq!(wheel.accumulate(0.125, now), 1);
    }

    #[test]
    fn preserves_magnitude_and_fraction_in_both_directions() {
        let mut wheel = WheelAccumulator::default();
        let now = Instant::now();
        assert_eq!(wheel.accumulate(3.5, now), 3);
        assert_eq!(wheel.accumulate(0.5, now), 1);
        assert_eq!(wheel.accumulate(-4.5, now), -4);
        assert_eq!(wheel.accumulate(-0.5, now), -1);
    }

    #[test]
    fn reversal_and_idle_discard_stale_fraction() {
        let mut wheel = WheelAccumulator::default();
        let now = Instant::now();
        assert_eq!(wheel.accumulate(0.75, now), 0);
        assert_eq!(wheel.accumulate(-0.5, now), 0);
        assert_eq!(wheel.accumulate(-0.5, now), -1);
        assert_eq!(wheel.accumulate(0.75, now), 0);
        assert_eq!(wheel.accumulate(0.5, now + Duration::from_secs(1)), 0);
    }

    #[test]
    fn pixels_use_actual_cell_height_and_lines_keep_device_magnitude() {
        let mut wheel = WheelAccumulator::default();
        assert_eq!(wheel.rows(ScrollDelta::Pixels(point(px(0.0), px(12.0))), 24.0), 0);
        assert_eq!(wheel.rows(ScrollDelta::Pixels(point(px(0.0), px(12.0))), 24.0), 1);
        assert_eq!(wheel.rows(ScrollDelta::Lines(point(0.0, -3.0)), 24.0), -3);
    }

    #[test]
    fn reset_and_invalid_device_input_are_safe() {
        let mut wheel = WheelAccumulator::default();
        let now = Instant::now();
        wheel.accumulate(0.75, now);
        wheel.reset();
        assert_eq!(wheel.accumulate(0.5, now), 0);
        assert_eq!(wheel.accumulate(f32::NAN, now), 0);
        assert_eq!(wheel.accumulate(f32::INFINITY, now), 0);
        assert_eq!(wheel.accumulate(0.5, now), 1);
        assert_eq!(wheel.accumulate(10000.0, now), 120);
    }
}
