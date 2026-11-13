//! Taking cover behind walls (every character).
//!
//! F next to a wall at least waist-high puts you in cover: crouched against it, A/D slides
//! along it. Hold the throw button to pre-charge while hidden (lock-on works as usual);
//! you rise up to throw over a low wall and drop back down after. From behind a wall too
//! tall to throw over, the throw curves around its end instead. Sprint, jump, dash or F
//! again leaves cover.
//!
//! Pure logic over the arena's boxes, like `parkour.rs`.

use bevy::math::Vec3;

use crate::parkour::{contacts, Contact};
use crate::sim::ball::BALL_RADIUS;
use crate::sim::layout::Aabb;
use crate::sim::BODY_RADIUS;

/// A wall face within this distance (beyond the body's radius) can be taken as cover.
pub const COVER_REACH: f32 = 0.9;
/// Cover must reach at least this high above the feet.
pub const COVER_MIN_HEIGHT: f32 = 1.0;
/// Walls up to this high above the feet are thrown over; taller ones are curved around.
pub const LOW_COVER_MAX: f32 = 2.6;
/// Sliding along the wall while crouched.
pub const COVER_SPEED: f32 = 3.2;
/// How long you stay up after a throw before ducking again.
pub const POP_SECS: f32 = 0.45;
/// How far from the face the body sits.
const HUG: f32 = BODY_RADIUS + 0.06;
/// The most a throw from cover is tilted up to clear the wall.
const MAX_LIFT: f32 = 0.6;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cover {
    pub wall: usize,
    /// Out of the wall, toward the player.
    pub normal: Vec3,
    pub top: f32,
    /// The face's extent along its axis.
    pub span: (f32, f32),
    /// Seconds left standing up after a throw.
    pub popped: f32,
}

impl Cover {
    /// The best wall to take cover behind from `feet`, if any.
    pub fn find(feet: Vec3, solids: &[Aabb]) -> Option<Cover> {
        contacts(feet, solids, COVER_REACH)
            .into_iter()
            .find(|c: &Contact| c.height_above(feet) >= COVER_MIN_HEIGHT)
            .map(|c| Cover { wall: c.wall, normal: c.normal, top: c.top, span: c.span, popped: 0.0 })
    }

    pub fn along(&self) -> Vec3 {
        Vec3::Y.cross(self.normal)
    }

    /// Low enough to throw over standing up.
    pub fn is_low(&self, feet: Vec3) -> bool {
        self.top - feet.y <= LOW_COVER_MAX
    }

    /// Crouched (hidden) unless aiming or just thrown.
    pub fn crouched(&self, aiming: bool) -> bool {
        !aiming && self.popped <= 0.0
    }

    /// Mark a throw: stand for a moment.
    pub fn popped_up(&mut self) {
        self.popped = POP_SECS;
    }

    /// Where the feet should be after sliding along the wall by input `dir` for `dt`.
    /// `None` when the wall is no longer there (it can't happen with static walls, but
    /// keeps the caller honest).
    pub fn slide(&mut self, feet: Vec3, dir: Vec3, dt: f32, solids: &[Aabb]) -> Option<Vec3> {
        self.popped = (self.popped - dt).max(0.0);
        let b = solids.get(self.wall)?;
        let along = self.along();
        let moved = feet + along * dir.dot(along) * COVER_SPEED * dt;
        let edge = BODY_RADIUS * 0.5;
        let (lo, hi) = (self.span.0 + edge, (self.span.1 - edge).max(self.span.0 + edge));
        // Stay hugging the face, inside its span.
        let p = if self.normal.x != 0.0 {
            let x = if self.normal.x > 0.0 { b.max.x + HUG } else { b.min.x - HUG };
            Vec3::new(x, feet.y, moved.z.clamp(lo, hi))
        } else {
            let z = if self.normal.z > 0.0 { b.max.z + HUG } else { b.min.z - HUG };
            Vec3::new(moved.x.clamp(lo, hi), feet.y, z)
        };
        Some(p)
    }

    /// Which way a throw from behind tall cover should curve to go around it: toward the
    /// nearer end of the wall. Returns the `curve` value for `Action::Throw` (+1 / -1).
    pub fn curve_around(&self, feet: Vec3, aim: Vec3) -> f32 {
        let along = self.along();
        let coord = if self.normal.x != 0.0 { feet.z } else { feet.x };
        let to_lo = coord - self.span.0;
        let to_hi = self.span.1 - coord;
        // Sign of the along axis that points to the nearer end.
        let axis_sign = if self.normal.x != 0.0 { along.z.signum() } else { along.x.signum() };
        let toward_hi = to_hi < to_lo;
        let end_dir = along * if toward_hi { axis_sign } else { -axis_sign };
        // A +curve ball starts out to the thrower's left and bulges round that side
        // (see `ball::launch` / `wrap_waypoint`), so going round the right end needs -1.
        let right = aim.cross(Vec3::Y).normalize_or_zero();
        if end_dir.dot(right) >= 0.0 {
            -1.0
        } else {
            1.0
        }
    }
}

/// Does the straight path `from` -> `to` pass through any box (fattened by the ball)?
pub fn blocked(from: Vec3, to: Vec3, solids: &[Aabb]) -> bool {
    let steps = ((to - from).length() / 0.2).ceil().max(1.0) as usize;
    (1..steps).any(|i| {
        let p = from.lerp(to, i as f32 / steps as f32);
        solids.iter().any(|b| b.expanded(BALL_RADIUS * 0.9).contains(p))
    })
}

/// Tilt `aim` up just enough that a throw from `from` clears the wall in front, still
/// heading for `target` (homing / gravity bring it back down). Leaves it alone if the line
/// is already clear or nothing within `MAX_LIFT` clears it.
pub fn clear_aim(from: Vec3, target: Vec3, wall: &Aabb) -> Vec3 {
    let to = target - from;
    let flat = Vec3::new(to.x, 0.0, to.z);
    let dist = flat.length().max(0.1);
    let base = to.y.atan2(dist);
    let dir = flat / dist;
    // Only the wall right in front matters (the other cover is the throw's business).
    let probe = |pitch: f32| {
        let d = dir * pitch.cos() + Vec3::Y * pitch.sin();
        blocked(from, from + d * 3.0, std::slice::from_ref(wall))
    };
    if !probe(base) {
        return to.normalize_or(dir);
    }
    let mut pitch = base;
    while pitch < base + MAX_LIFT {
        pitch += 0.02;
        if !probe(pitch) {
            // A little margin over the edge.
            let p = pitch + 0.04;
            return dir * p.cos() + Vec3::Y * p.sin();
        }
    }
    to.normalize_or(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::layout::{self, ArenaId};
    use crate::sim::COVER_HOLD_HEIGHT;

    /// The classic arena's low wall in front of Red's spawn (x -3..3, z 6.6..7.4, 1.2 m).
    fn low_wall() -> (usize, Aabb) {
        layout::solids(ArenaId::Classic)
            .into_iter()
            .enumerate()
            .find(|(_, b)| b.max.y < 1.5 && b.min.z > 6.0)
            .expect("low wall")
    }

    #[test]
    fn finds_waist_high_walls_only() {
        let solids = layout::solids(ArenaId::Classic);
        let (i, w) = low_wall();
        let behind = Vec3::new(0.0, 0.0, w.max.z + 0.8);
        let c = Cover::find(behind, &solids).expect("cover behind the low wall");
        assert_eq!(c.wall, i);
        assert_eq!(c.normal, Vec3::Z);
        assert!(c.is_low(behind));
        // In the open: nothing.
        assert!(Cover::find(Vec3::new(0.0, 0.0, 12.0), &solids).is_none());
        // A knee-high box is not cover.
        let knee = [Aabb::from_center(Vec3::new(0.0, 0.3, 0.0), Vec3::new(2.0, 0.3, 0.3))];
        assert!(Cover::find(Vec3::new(0.0, 0.0, 0.9), &knee).is_none());
    }

    #[test]
    fn slides_along_the_wall_and_stops_at_its_end() {
        let solids = layout::solids(ArenaId::Classic);
        let (_, w) = low_wall();
        let mut feet = Vec3::new(0.0, 0.0, w.max.z + 0.8);
        let mut c = Cover::find(feet, &solids).unwrap();
        feet = c.slide(feet, Vec3::ZERO, 0.016, &solids).unwrap();
        assert!((feet.z - (w.max.z + HUG)).abs() < 1e-4, "hugs the face");
        for _ in 0..300 {
            feet = c.slide(feet, Vec3::X, 0.016, &solids).unwrap();
        }
        assert!(feet.x <= w.max.x && feet.x > w.max.x - 0.5, "stops at the end: {}", feet.x);
        // Pressing into / away from the wall doesn't move you off it.
        let before = feet;
        feet = c.slide(feet, Vec3::Z, 0.5, &solids).unwrap();
        assert_eq!(feet, before);
    }

    #[test]
    fn crouched_until_aiming_and_after_a_throw() {
        let solids = layout::solids(ArenaId::Classic);
        let (_, w) = low_wall();
        let feet = Vec3::new(0.0, 0.0, w.max.z + 0.8);
        let mut c = Cover::find(feet, &solids).unwrap();
        assert!(c.crouched(false));
        assert!(!c.crouched(true), "stands up to aim");
        c.popped_up();
        assert!(!c.crouched(false), "still up right after throwing");
        for _ in 0..40 {
            c.slide(feet, Vec3::ZERO, 0.016, &solids);
        }
        assert!(c.crouched(false), "ducks again");
    }

    #[test]
    fn throw_from_cover_clears_the_low_wall() {
        let (_, w) = low_wall();
        let feet = Vec3::new(0.0, 0.0, w.max.z + HUG);
        let from = feet + Vec3::Y * COVER_HOLD_HEIGHT + Vec3::NEG_Z * 0.45;
        // Risen up, a throw at someone far off already clears the wall.
        assert!(!blocked(from, Vec3::new(0.0, 1.0, -8.0), &[w]));
        // Someone crouching close behind it: the straight line clips the top, so lift it.
        let target = Vec3::new(0.0, 0.3, 5.0);
        assert!(blocked(from, target, &[w]), "a straight throw hits the wall");
        let aim = clear_aim(from, target, &w);
        assert!(!blocked(from, from + aim * 3.0, &[w]), "lifted over it");
        assert!(aim.y > (target - from).normalize().y && aim.z < -0.5, "still heading for the target: {aim}");
        // Nothing in the way: the aim is untouched.
        let open = clear_aim(Vec3::new(10.0, 1.3, 10.0), Vec3::new(10.0, 1.0, -5.0), &w);
        assert!(open.abs_diff_eq((Vec3::new(10.0, 1.0, -5.0) - Vec3::new(10.0, 1.3, 10.0)).normalize(), 1e-5));
    }

    #[test]
    fn curves_toward_the_nearer_end_of_tall_cover() {
        // A tall wall along X (x -4..4) at z=0, player behind it at +Z, near the +X end.
        let wall = Aabb::from_center(Vec3::new(0.0, 2.5, 0.0), Vec3::new(4.0, 2.5, 0.3));
        let feet = Vec3::new(3.0, 0.0, 0.8);
        let c = Cover::find(feet, &[wall]).unwrap();
        assert!(!c.is_low(feet));
        // Throwing toward -Z, the thrower's right is +X, which is also the nearer end:
        // bulge round the right side = curve -1.
        let aim = Vec3::NEG_Z;
        assert_eq!(aim.cross(Vec3::Y), Vec3::X);
        let curve = c.curve_around(feet, aim);
        assert_eq!(curve, -1.0);
        let far = c.curve_around(Vec3::new(-3.0, 0.0, 0.8), aim);
        assert_eq!(far, -curve, "other end, other way");
    }
}
