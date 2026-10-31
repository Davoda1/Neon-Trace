//! Quiet, interruptible following of the currently animated journey card.
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

pub const SCROLL_ID: &str = "neontrace-journey";
pub const CARD_WIDTH: f32 = 150.0;
pub const CARD_GAP: f32 = 14.0;
const MARGIN: f32 = 16.0;
const EASING: Duration = Duration::from_millis(240);
const MANUAL_GRACE: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Copy)]
struct Viewport {
    offset: f32,
    width: f32,
    content_width: f32,
}

#[derive(Debug, Clone, Copy)]
struct Motion {
    start: Instant,
    from: f32,
    to: f32,
}

#[derive(Debug, Default)]
pub struct Follow {
    viewport: Option<Viewport>,
    hovered: bool,
    manual_until: Option<Instant>,
    motion: Option<Motion>,
    route: Option<(u64, u64)>,
    last_active: Option<usize>,
    jump_next: bool,
    // Widget operations report their changed viewport on a later redraw. Keep
    // acknowledgments separate from user scrolling, including queued frames.
    commanded: VecDeque<f32>,
}

impl Follow {
    pub fn restart(&mut self) {
        self.last_active = None;
        self.jump_next = true;
        self.manual_until = None;
        self.cancel();
    }

    pub fn route(&mut self, key: (u64, u64)) {
        if self.route != Some(key) {
            self.route = Some(key);
            self.last_active = None;
            self.jump_next = true;
            self.cancel();
        }
    }

    pub fn observe(&mut self, now: Instant, offset: f32, width: f32, content_width: f32) {
        if !offset.is_finite() || !width.is_finite() || !content_width.is_finite() || width <= 0.0 {
            return;
        }
        let viewport = Viewport {
            offset: offset.clamp(0.0, (content_width - width).max(0.0)),
            width,
            content_width,
        };
        let resized = self.viewport.is_none_or(|old| {
            (old.width - width).abs() > 0.5 || (old.content_width - content_width).abs() > 0.5
        });
        if resized {
            self.cancel();
        } else if let Some(acknowledged) = self
            .commanded
            .iter()
            .position(|x| (*x - viewport.offset).abs() < 0.5)
        {
            self.commanded.drain(..=acknowledged);
        } else if self
            .viewport
            .is_some_and(|old| (old.offset - viewport.offset).abs() > 0.5)
        {
            self.manual_until = Some(now + MANUAL_GRACE);
            self.cancel();
        }
        self.viewport = Some(viewport);
    }

    pub fn hover(&mut self, now: Instant, hovered: bool) {
        if self.hovered != hovered {
            self.hovered = hovered;
            self.manual_until = Some(now + MANUAL_GRACE);
            self.cancel();
        }
    }

    pub fn cancel(&mut self) {
        self.motion = None;
        self.commanded.clear();
    }

    /// Keep the active card visible with the smallest necessary movement.
    /// Reduced-motion mode uses the same correction without interpolation.
    pub fn advance(
        &mut self,
        now: Instant,
        active: usize,
        enabled: bool,
        animate: bool,
    ) -> Option<f32> {
        if self
            .last_active
            .replace(active)
            .is_some_and(|previous| active < previous)
        {
            // Avoid an animated sweep across every earlier hop at loop/replay.
            self.jump_next = true;
            self.motion = None;
        }
        if !enabled || self.hovered || self.manual_until.is_some_and(|until| now < until) {
            self.cancel();
            return None;
        }
        let viewport = self.viewport?;
        let target = visibility_offset(viewport, active);
        if (target - viewport.offset).abs() < 0.5 {
            self.motion = None;
            self.jump_next = false;
            return None;
        }
        let next = if animate && !self.jump_next {
            let motion = self.motion.get_or_insert(Motion {
                start: now,
                from: viewport.offset,
                to: target,
            });
            if (motion.to - target).abs() > 0.5 {
                *motion = Motion {
                    start: now,
                    from: viewport.offset,
                    to: target,
                };
            }
            let phase = (now.saturating_duration_since(motion.start).as_secs_f32()
                / EASING.as_secs_f32())
            .min(1.0);
            let eased = 1.0 - (1.0 - phase).powi(3);
            motion.from + (motion.to - motion.from) * eased
        } else {
            self.motion = None;
            self.jump_next = false;
            target
        };
        if (next - viewport.offset).abs() < 0.1 {
            return None;
        }
        self.commanded.push_back(next);
        while self.commanded.len() > 32 {
            self.commanded.pop_front();
        }
        Some(next)
    }
}

fn visibility_offset(viewport: Viewport, active: usize) -> f32 {
    let left = active as f32 * (CARD_WIDTH + CARD_GAP);
    let right = left + CARD_WIDTH;
    let margin = MARGIN.min((viewport.width - CARD_WIDTH).max(0.0) / 2.0);
    let offset = if left < viewport.offset + margin {
        left - margin
    } else if right > viewport.offset + viewport.width - margin {
        right - viewport.width + margin
    } else {
        viewport.offset
    };
    offset.clamp(0.0, (viewport.content_width - viewport.width).max(0.0))
}

pub fn active_segment(fractions: &[f32]) -> Option<usize> {
    fractions
        .iter()
        .position(|fraction| *fraction < 1.0)
        .or_else(|| fractions.len().checked_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn follow(now: Instant) -> Follow {
        let mut follow = Follow::default();
        follow.observe(now, 0.0, 650.0, 1954.0);
        follow
    }

    #[test]
    fn visibility_follows_weighted_active_segment_with_minimal_travel() {
        let now = Instant::now();
        let mut follow = follow(now);
        assert_eq!(active_segment(&[1.0, 1.0, 0.25, 0.0]), Some(2));
        assert_eq!(active_segment(&[0.0, 0.0]), Some(0));
        assert_eq!(active_segment(&[]), None);
        assert_eq!(follow.advance(now, 2, true, false), None);
        let offset = follow.advance(now, 4, true, false).unwrap();
        assert_eq!(offset, 172.0);
        follow.observe(now, offset, 650.0, 1954.0);
        assert_eq!(follow.advance(now, 4, true, false), None);
        let at_end = follow.advance(now, 11, true, false).unwrap();
        assert_eq!(at_end, 1304.0);
        follow.observe(now, at_end, 650.0, 1954.0);
        assert_eq!(follow.advance(now, 0, true, false), Some(0.0));
    }

    #[test]
    fn automatic_easing_acks_do_not_look_like_manual_scrolling() {
        let now = Instant::now();
        let mut follow = follow(now);
        assert_eq!(follow.advance(now, 4, true, true), None);
        let midway = follow.advance(now + EASING / 2, 4, true, true).unwrap();
        assert!(midway > 0.0 && midway < 172.0);
        follow.observe(now + EASING / 2, midway, 650.0, 1954.0);
        assert!(follow.manual_until.is_none());
        let end = follow.advance(now + EASING, 4, true, true).unwrap();
        assert_eq!(end, 172.0);
        follow.observe(now + EASING, end, 650.0, 1954.0);
        assert_eq!(follow.advance(now + EASING, 4, true, true), None);
    }

    #[test]
    fn manual_scroll_and_hover_take_priority_then_follow_resumes() {
        let now = Instant::now();
        let mut follow = follow(now);
        follow.observe(now, 800.0, 650.0, 1954.0);
        assert_eq!(
            follow.advance(now + Duration::from_secs(3), 0, true, false),
            None
        );
        assert_eq!(
            follow.advance(now + MANUAL_GRACE, 0, true, false),
            Some(0.0)
        );
        follow.hover(now + MANUAL_GRACE, true);
        assert_eq!(
            follow.advance(now + Duration::from_secs(30), 0, true, false),
            None
        );
        follow.hover(now + Duration::from_secs(30), false);
        assert_eq!(
            follow.advance(now + Duration::from_secs(33), 0, true, false),
            None
        );
        assert_eq!(
            follow.advance(now + Duration::from_secs(34), 0, true, false),
            Some(0.0)
        );
    }

    #[test]
    fn pause_cancels_motion_resize_is_not_manual_and_reduced_motion_is_immediate() {
        let now = Instant::now();
        let mut follow = follow(now);
        follow.advance(now, 6, true, true);
        assert_eq!(follow.advance(now + EASING, 6, false, true), None);
        assert!(follow.motion.is_none());
        follow.observe(now + EASING, 0.0, 500.0, 1954.0);
        assert!(follow.manual_until.is_none());
        assert_eq!(follow.advance(now + EASING, 6, true, false), Some(650.0));
    }

    #[test]
    fn replay_and_loop_rewind_without_a_fast_animated_sweep() {
        let now = Instant::now();
        let mut follow = follow(now);
        let end = follow.advance(now, 11, true, false).unwrap();
        follow.observe(now, end, 650.0, 1954.0);
        assert_eq!(follow.advance(now + EASING, 0, true, true), Some(0.0));
        assert!(follow.motion.is_none());
        follow.observe(now + EASING, 0.0, 650.0, 1954.0);
        follow.observe(now + EASING, 900.0, 650.0, 1954.0);
        follow.restart();
        assert_eq!(follow.advance(now + EASING, 0, true, true), Some(0.0));
        follow.observe(now + EASING, 0.0, 650.0, 1954.0);
        follow.route((2, 1));
        assert_eq!(follow.advance(now + EASING, 6, true, true), Some(500.0));
    }
}
