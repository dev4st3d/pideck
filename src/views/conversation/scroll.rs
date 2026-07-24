//! Low-latency frame bridging for discrete mouse-wheel input.

use std::time::Instant;

use gpui::{Pixels, px};

const EASE_TIME_CONSTANT_SECONDS: f32 = 0.018;
const MAX_QUEUED_DISTANCE: f32 = 360.0;
const SETTLE_DISTANCE: f32 = 0.5;
const MIN_FRAME_SECONDS: f32 = 1.0 / 240.0;
const MAX_FRAME_SECONDS: f32 = 1.0 / 30.0;

#[derive(Debug, Default)]
pub(in crate::views) struct ConversationScrollMotion {
    remaining: f32,
    last_frame: Option<Instant>,
    frame_pending: bool,
}

impl ConversationScrollMotion {
    pub(in crate::views) fn push(&mut self, distance: Pixels, now: Instant) -> bool {
        let distance = f32::from(distance);
        if !distance.is_finite() || distance.abs() < f32::EPSILON {
            return false;
        }

        if self.remaining != 0.0 && self.remaining.signum() != distance.signum() {
            // A wheel reversal is a new direct instruction. Discard the old tail
            // rather than making the user push through synthetic momentum.
            self.remaining = distance;
        } else {
            self.remaining =
                (self.remaining + distance).clamp(-MAX_QUEUED_DISTANCE, MAX_QUEUED_DISTANCE);
        }
        self.last_frame.get_or_insert(now);
        true
    }

    pub(in crate::views) fn advance(&mut self, now: Instant) -> Option<Pixels> {
        if !self.is_active() {
            return None;
        }
        let elapsed = self
            .last_frame
            .replace(now)
            .map(|last| now.saturating_duration_since(last).as_secs_f32())
            .unwrap_or(MIN_FRAME_SECONDS)
            .clamp(MIN_FRAME_SECONDS, MAX_FRAME_SECONDS);
        Some(px(self.advance_by(elapsed)))
    }

    pub(in crate::views) fn cancel(&mut self) {
        self.remaining = 0.0;
        self.last_frame = None;
        self.frame_pending = false;
    }

    pub(in crate::views) fn begin_frame(&mut self) {
        self.frame_pending = false;
    }

    pub(in crate::views) fn schedule_frame(&mut self) -> bool {
        if !self.is_active() || self.frame_pending {
            return false;
        }
        self.frame_pending = true;
        true
    }

    fn is_active(&self) -> bool {
        self.remaining.abs() >= SETTLE_DISTANCE
    }

    fn advance_by(&mut self, elapsed: f32) -> f32 {
        let elapsed = elapsed.clamp(MIN_FRAME_SECONDS, MAX_FRAME_SECONDS);
        let alpha = 1.0 - (-elapsed / EASE_TIME_CONSTANT_SECONDS).exp();
        let mut step = self.remaining * alpha;
        self.remaining -= step;
        if !self.is_active() {
            step += self.remaining;
            self.remaining = 0.0;
            self.last_frame = None;
        }
        step
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settle(motion: &mut ConversationScrollMotion, frame_seconds: f32) -> f32 {
        let mut distance = 0.0;
        for _ in 0..1_000 {
            if !motion.is_active() {
                break;
            }
            distance += motion.advance_by(frame_seconds);
        }
        distance
    }

    #[test]
    fn converges_to_the_same_distance_at_60_and_144_hz() {
        let now = Instant::now();
        let mut sixty = ConversationScrollMotion::default();
        let mut high_refresh = ConversationScrollMotion::default();
        assert!(sixty.push(px(180.0), now));
        assert!(high_refresh.push(px(180.0), now));

        let sixty_distance = settle(&mut sixty, 1.0 / 60.0);
        let high_refresh_distance = settle(&mut high_refresh, 1.0 / 144.0);
        assert!((sixty_distance - 180.0).abs() < 0.01);
        assert!((high_refresh_distance - 180.0).abs() < 0.01);
    }

    #[test]
    fn repeated_input_accumulates_but_stays_bounded() {
        let now = Instant::now();
        let mut motion = ConversationScrollMotion::default();
        for _ in 0..100 {
            motion.push(px(60.0), now);
        }
        assert!((settle(&mut motion, 1.0 / 144.0) - MAX_QUEUED_DISTANCE).abs() < 0.01);
    }

    #[test]
    fn opposite_input_retargets_immediately_without_synthetic_momentum() {
        let now = Instant::now();
        let mut motion = ConversationScrollMotion::default();
        motion.push(px(240.0), now);
        let before = motion.advance_by(1.0 / 144.0);
        motion.push(px(-120.0), now);
        let after = motion.advance_by(1.0 / 144.0);

        assert!(before > 0.0);
        assert!(after < 0.0);
        assert!(settle(&mut motion, 1.0 / 144.0) < 0.0);
    }

    #[test]
    fn repeated_input_does_not_restart_the_frame_clock() {
        let start = Instant::now();
        let mut motion = ConversationScrollMotion::default();
        motion.push(px(60.0), start);
        let first = motion
            .advance(start + std::time::Duration::from_millis(8))
            .unwrap();
        motion.push(px(60.0), start + std::time::Duration::from_millis(12));
        let second = motion
            .advance(start + std::time::Duration::from_millis(16))
            .unwrap();

        assert!(first > px(20.0));
        assert!(second > px(20.0));
    }

    #[test]
    fn cancellation_stops_frames_and_motion() {
        let now = Instant::now();
        let mut motion = ConversationScrollMotion::default();
        motion.push(px(60.0), now);
        assert!(motion.schedule_frame());
        motion.cancel();
        assert!(!motion.schedule_frame());
        assert!(motion.advance(now).is_none());
    }
}
