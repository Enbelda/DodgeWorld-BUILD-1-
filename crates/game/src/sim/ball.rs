//! Ball flight: gravity, curve, lob arcs and bounces off the arena.

use bevy::math::Vec3;
use serde::{Deserialize, Serialize};

use super::layout::Aabb;

/// The kinds of ball in the arena.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum BallKind {
    Normal,
    /// Flies back to its thrower after it hits someone or a surface, arriving charged.
    Boomerang,
    /// Slimes whoever it hits: knocked to the nearest wall and stuck there.
    Slime,
    /// Heavy red brick: big damage and knocks the target flat (stunned).
    Brick,
}

impl BallKind {
    /// Damage in half-hearts.
    pub fn damage(self, powered: bool) -> u8 {
        let base = match self {
            BallKind::Brick => 4,
            _ => 1,
        };
        if powered {
            base * 2
        } else {
            base
        }
    }
}

pub const BALL_RADIUS: f32 = 0.3;
/// Gravity on a straight throw: low so fast throws fly mostly flat.
pub const THROW_GRAVITY: f32 = 4.0;
/// Full gravity once a ball is dead or lobbed.
pub const FULL_GRAVITY: f32 = 18.0;
pub const MIN_THROW_SPEED: f32 = 20.0;
pub const MAX_THROW_SPEED: f32 = 32.0;
/// Extra speed for a ball that was just caught.
pub const POWER_BONUS: f32 = 6.0;
/// Curved and lobbed throws fly this much faster (+30%)...
pub const SPECIAL_SPEED: f32 = 1.3;
/// ...and bend this much more (+40%) on top of the earlier +30% tuning.
pub const SPECIAL_BEND: f32 = 1.4;
/// Curve tuning before this round: 26 m/s^2 sideways, 0.22 rad launch offset, both x1.3.
const PREV_CURVE_ACCEL: f32 = 26.0 * 1.3;
const PREV_CURVE_OFFSET: f32 = 0.22 * 1.3;
/// Sideways acceleration at full curve, m/s^2. Path bend goes with accel / speed^2, so the
/// faster ball needs SPECIAL_SPEED^2 more accel to keep bending, times SPECIAL_BEND.
pub const CURVE_ACCEL: f32 = PREV_CURVE_ACCEL * SPECIAL_BEND * SPECIAL_SPEED * SPECIAL_SPEED;
/// How far a curved throw starts angled away from the aim (it bends back).
pub const CURVE_LAUNCH_OFFSET: f32 = PREV_CURVE_OFFSET * SPECIAL_BEND;
/// Lob tuning before this round (14 m/s at 0.85 rad, arc height x1.3).
const PREV_LOB_FORWARD: f32 = 9.2398;
const PREV_LOB_UP: f32 = 10.5179 * 1.140_175;
/// Lob launch: forward speed x1.3; arc height x1.4 (height goes with up-speed squared).
pub const LOB_FORWARD_SPEED: f32 = PREV_LOB_FORWARD * SPECIAL_SPEED;
pub const LOB_UP_SPEED: f32 = PREV_LOB_UP * 1.183_216; // sqrt(SPECIAL_BEND)
/// How fast a locked-on throw turns toward its target, radians per second.
pub const HOMING_RATE: f32 = 1.2;
/// Speed of a boomerang flying back to its thrower.
pub const RETURN_SPEED: f32 = 24.0;
/// A ball that hits a wall bounces off this much faster.
pub const RICOCHET_SPEEDUP: f32 = 2.0;
/// Nothing flies faster than this (keeps stacked + ricocheting balls sane).
pub const MAX_BALL_SPEED: f32 = 180.0;
/// Perfect-catch speed stacks: each adds the base speed again (x2, x3, x4, x5).
pub const MAX_STACKS: u8 = 4;
/// How fast a curved ball turns to get around cover, and to track its target after.
pub const WRAP_TURN_RATE: f32 = 9.0;
pub const TRACK_TURN_RATE: f32 = 8.0;
/// Room a curving ball leaves between itself and the cover it goes around.
pub const WRAP_CLEARANCE: f32 = 1.2;
const RESTITUTION: f32 = 0.45;
const FRICTION: f32 = 0.7;
/// Below this speed a resting ball stops moving.
const SLEEP_SPEED: f32 = 0.4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThrowParams {
    /// 0..1, how long the throw was charged.
    pub charge: f32,
    /// -1 (curve left) .. 1 (curve right).
    pub curve: f32,
    pub lob: bool,
    pub powered: bool,
}

/// Starting velocity and per-second curve for a throw in `aim` direction.
pub fn launch(aim: Vec3, p: ThrowParams) -> (Vec3, f32) {
    let aim = aim.normalize_or(Vec3::NEG_Z);
    if p.lob {
        let flat = Vec3::new(aim.x, 0.0, aim.z).normalize_or(Vec3::NEG_Z);
        return (flat * LOB_FORWARD_SPEED + Vec3::Y * LOB_UP_SPEED, 0.0);
    }
    let mut speed = MIN_THROW_SPEED + (MAX_THROW_SPEED - MIN_THROW_SPEED) * p.charge.clamp(0.0, 1.0);
    if p.powered {
        speed += POWER_BONUS;
    }
    // A curved ball flies faster and starts angled away so it bends back toward the aim.
    let right = aim.cross(Vec3::Y).normalize_or(Vec3::X);
    let curve = p.curve.clamp(-1.0, 1.0);
    speed *= 1.0 + (SPECIAL_SPEED - 1.0) * curve.abs();
    let dir = (aim - right * curve * CURVE_LAUNCH_OFFSET).normalize();
    (dir * speed, curve)
}

/// Speed multiplier for a ball with `stacks` perfect catches behind it.
pub fn stack_multiplier(stacks: u8) -> f32 {
    1.0 + stacks.min(MAX_STACKS) as f32
}

/// Lob launch that comes down on `target` (e.g. the top of a head): the arc height stays
/// the normal lob height and the forward speed is solved so the ball falls onto the point.
pub fn lob_to(from: Vec3, target: Vec3) -> Vec3 {
    let up = LOB_UP_SPEED;
    let g = FULL_GRAVITY;
    let drop = from.y - target.y;
    // Time until the falling ball is at the target height.
    let disc = up * up + 2.0 * g * drop;
    let t = if disc > 0.0 { (up + disc.sqrt()) / g } else { up / g };
    let flat = Vec3::new(target.x - from.x, 0.0, target.z - from.z);
    let speed = (flat.length() / t.max(0.05)).clamp(2.0, 30.0);
    flat.normalize_or(Vec3::NEG_Z) * speed + Vec3::Y * up
}

/// Bounce off a wall with normal `n`, twice as fast, capped.
pub fn ricochet(v: Vec3, n: Vec3) -> Vec3 {
    let n = n.normalize_or_zero();
    let out = (v - 2.0 * v.dot(n) * n) * RICOCHET_SPEEDUP;
    out.clamp_length_max(MAX_BALL_SPEED)
}

/// Turn the horizontal heading of `vel` toward `to` at `rate` rad/s, keeping speed.
pub fn steer_flat(vel: &mut Vec3, to: Vec3, rate: f32, dt: f32) {
    let flat = Vec3::new(vel.x, 0.0, vel.z);
    let want = Vec3::new(to.x, 0.0, to.z);
    let (a, b) = (flat.normalize_or_zero(), want.normalize_or_zero());
    if a == Vec3::ZERO || b == Vec3::ZERO {
        return;
    }
    let angle = a.angle_between(b);
    let turned = if angle <= rate * dt {
        b * flat.length()
    } else {
        let axis = if a.cross(b).y >= 0.0 { Vec3::Y } else { Vec3::NEG_Y };
        bevy::math::Quat::from_axis_angle(axis, rate * dt) * flat
    };
    vel.x = turned.x;
    vel.z = turned.z;
}

/// Does the flat segment a->b cross the box (seen from above, grown by `grow`)?
fn segment_crosses_box(a: Vec3, b: Vec3, bx: &Aabb, grow: f32) -> bool {
    let (min, max) = (bx.min - Vec3::splat(grow), bx.max + Vec3::splat(grow));
    let d = b - a;
    let (mut t0, mut t1) = (0.0_f32, 1.0_f32);
    for (o, dd, lo, hi) in [(a.x, d.x, min.x, max.x), (a.z, d.z, min.z, max.z)] {
        if dd.abs() < 1e-6 {
            if o < lo || o > hi {
                return false;
            }
        } else {
            let (mut ta, mut tb) = ((lo - o) / dd, (hi - o) / dd);
            if ta > tb {
                std::mem::swap(&mut ta, &mut tb);
            }
            t0 = t0.max(ta);
            t1 = t1.min(tb);
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

/// Where a curving ball at `pos` should head to get around cover between it and `target`.
/// `side` is +1 to pass on the right of the line to the target, -1 on the left.
/// Returns `None` when the way is clear.
pub fn wrap_waypoint(pos: Vec3, target: Vec3, side: f32, cover: &[Aabb]) -> Option<Vec3> {
    let blocker = cover
        .iter()
        .filter(|c| c.max.y > pos.y - BALL_RADIUS && c.min.y < pos.y + BALL_RADIUS)
        .filter(|c| segment_crosses_box(pos, target, c, BALL_RADIUS))
        .min_by(|a, b| a.center().distance(pos).total_cmp(&b.center().distance(pos)))?;
    let to = Vec3::new(target.x - pos.x, 0.0, target.z - pos.z).normalize_or(Vec3::NEG_Z);
    let right = Vec3::new(-to.z, 0.0, to.x);
    let c = WRAP_CLEARANCE + BALL_RADIUS;
    let (min, max) = (blocker.min, blocker.max);
    let corners = [
        Vec3::new(min.x - c, pos.y, min.z - c),
        Vec3::new(min.x - c, pos.y, max.z + c),
        Vec3::new(max.x + c, pos.y, min.z - c),
        Vec3::new(max.x + c, pos.y, max.z + c),
    ];
    let path = |k: &Vec3| k.distance(pos) + k.distance(target);
    corners
        .iter()
        .filter(|k| (**k - pos).dot(right) * side > 0.0)
        .filter(|k| (**k - pos).dot(to) > -0.5)
        .min_by(|a, b| path(a).total_cmp(&path(b)))
        .or_else(|| corners.iter().min_by(|a, b| path(a).total_cmp(&path(b))))
        .copied()
}

/// Gravity for a ball in flight.
pub fn gravity(lob: bool, live: bool) -> f32 {
    if live && !lob {
        THROW_GRAVITY
    } else {
        FULL_GRAVITY
    }
}

/// Turn `vel` toward `to_target` by at most `HOMING_RATE * dt` radians, keeping its speed.
/// With `flat_only` only the horizontal heading turns (used for lobs, so the arc stays).
pub fn home(vel: &mut Vec3, to_target: Vec3, dt: f32, flat_only: bool) {
    turn_toward(vel, to_target, HOMING_RATE, dt, flat_only);
}

/// Turn `vel` toward `to_target` by at most `rate * dt` radians, keeping its speed.
pub fn turn_toward(vel: &mut Vec3, to_target: Vec3, rate: f32, dt: f32, flat_only: bool) {
    let max_turn = rate * dt;
    let turn = |from: Vec3, to: Vec3| -> Vec3 {
        let (a, b) = (from.normalize_or_zero(), to.normalize_or_zero());
        if a == Vec3::ZERO || b == Vec3::ZERO {
            return from;
        }
        let angle = a.angle_between(b);
        if angle <= max_turn {
            return b * from.length();
        }
        let axis = a.cross(b).normalize_or(Vec3::Y);
        bevy::math::Quat::from_axis_angle(axis, max_turn) * from
    };
    if flat_only {
        let flat = turn(Vec3::new(vel.x, 0.0, vel.z), Vec3::new(to_target.x, 0.0, to_target.z));
        vel.x = flat.x;
        vel.z = flat.z;
    } else {
        *vel = turn(*vel, to_target);
    }
}

/// Fly a returning boomerang toward `target`. Returns true once it arrives.
pub fn fly_back(pos: &mut Vec3, vel: &mut Vec3, target: Vec3, dt: f32) -> bool {
    let to = target - *pos;
    let step = RETURN_SPEED * dt;
    if to.length() <= step.max(0.7) {
        *pos = target;
        *vel = Vec3::ZERO;
        return true;
    }
    *vel = to.normalize() * RETURN_SPEED;
    *pos += *vel * dt;
    false
}

/// Result of moving a ball for one step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StepResult {
    /// The ball touched the floor or a solid this step.
    pub touched: bool,
    /// Outward normal of the first wall side it hit (not floors or box tops).
    pub wall: Option<Vec3>,
    /// Velocity just before that wall contact.
    pub vel_before_wall: Vec3,
}

/// Integrate one step. Curve bends velocity sideways in the horizontal plane.
pub fn step(
    pos: &mut Vec3,
    vel: &mut Vec3,
    curve: f32,
    gravity: f32,
    dt: f32,
    solids: &[Aabb],
) -> StepResult {
    if curve != 0.0 {
        let flat = Vec3::new(vel.x, 0.0, vel.z);
        if flat.length_squared() > 1e-4 {
            let right = flat.normalize().cross(Vec3::Y);
            *vel += right * curve * CURVE_ACCEL * dt;
        }
    }
    vel.y -= gravity * dt;

    // Sub-step fast balls so they never tunnel through thin walls.
    let travel = vel.length() * dt;
    let steps = ((travel / (BALL_RADIUS * 0.8)).ceil() as usize).clamp(1, 64);
    let h = dt / steps as f32;
    let mut touched = false;
    let mut wall = None;
    let mut vel_before_wall = *vel;
    for _ in 0..steps {
        *pos += *vel * h;
        if pos.y < BALL_RADIUS {
            pos.y = BALL_RADIUS;
            if vel.y < 0.0 {
                vel.y = -vel.y * RESTITUTION;
                vel.x *= FRICTION;
                vel.z *= FRICTION;
            }
            touched = true;
        }
        for s in solids {
            let before = *vel;
            if let Some(normal) = resolve_box(pos, vel, s) {
                touched = true;
                if normal.y == 0.0 && wall.is_none() {
                    wall = Some(normal);
                    vel_before_wall = before;
                }
            }
        }
    }
    if pos.y <= BALL_RADIUS + 0.01 && vel.length() < SLEEP_SPEED {
        *vel = Vec3::ZERO;
    }
    StepResult { touched, wall, vel_before_wall }
}

/// Push the ball out of a box along the shallowest axis and bounce.
/// Returns the outward normal of the side it hit.
fn resolve_box(pos: &mut Vec3, vel: &mut Vec3, b: &Aabb) -> Option<Vec3> {
    let e = b.expanded(BALL_RADIUS);
    if !e.contains(*pos) {
        return None;
    }
    let to_min = *pos - e.min;
    let to_max = e.max - *pos;
    let pen = [to_min.x, to_max.x, to_min.y, to_max.y, to_min.z, to_max.z];
    let (axis, _) = pen
        .iter()
        .enumerate()
        .fold((0, f32::MAX), |acc, (i, &d)| if d < acc.1 { (i, d) } else { acc });
    let (component, sign) = (axis / 2, if axis % 2 == 0 { -1.0 } else { 1.0 });
    match component {
        0 => pos.x = if sign < 0.0 { e.min.x } else { e.max.x },
        1 => pos.y = if sign < 0.0 { e.min.y } else { e.max.y },
        _ => pos.z = if sign < 0.0 { e.min.z } else { e.max.z },
    }
    let v = &mut vel[component];
    if *v * sign < 0.0 {
        *v = -*v * RESTITUTION;
    }
    let mut normal = Vec3::ZERO;
    normal[component] = sign;
    Some(normal)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn throw(charge: f32, curve: f32, lob: bool) -> ThrowParams {
        ThrowParams { charge, curve, lob, powered: false }
    }

    fn fly(mut pos: Vec3, mut vel: Vec3, curve: f32, g: f32, secs: f32) -> Vec3 {
        let dt = 1.0 / 60.0;
        for _ in 0..(secs / dt) as usize {
            step(&mut pos, &mut vel, curve, g, dt, &[]);
        }
        pos
    }

    #[test]
    fn charged_throw_is_faster() {
        let (slow, _) = launch(Vec3::NEG_Z, throw(0.0, 0.0, false));
        let (fast, _) = launch(Vec3::NEG_Z, throw(1.0, 0.0, false));
        assert!(fast.length() > slow.length());
        assert!((slow.length() - MIN_THROW_SPEED).abs() < 1e-3);
    }

    #[test]
    fn powered_throw_adds_speed() {
        let p = ThrowParams { charge: 1.0, curve: 0.0, lob: false, powered: true };
        assert!((launch(Vec3::NEG_Z, p).0.length() - (MAX_THROW_SPEED + POWER_BONUS)).abs() < 1e-3);
    }

    #[test]
    fn right_curve_bends_right_and_comes_back() {
        let (v, c) = launch(Vec3::NEG_Z, throw(0.5, 1.0, false));
        // Starts angled left (-X) when aiming down -Z with right curve.
        assert!(v.x < 0.0);
        let end = fly(Vec3::new(0.0, 1.5, 0.0), v, c, THROW_GRAVITY, 0.8);
        // Ends to the right of where it started bending.
        let mid = fly(Vec3::new(0.0, 1.5, 0.0), v, c, THROW_GRAVITY, 0.2);
        assert!(end.x > mid.x);
    }

    #[test]
    fn left_curve_mirrors_right_curve() {
        let (vr, cr) = launch(Vec3::NEG_Z, throw(0.5, 1.0, false));
        let (vl, cl) = launch(Vec3::NEG_Z, throw(0.5, -1.0, false));
        let r = fly(Vec3::Y * 1.5, vr, cr, THROW_GRAVITY, 0.5);
        let l = fly(Vec3::Y * 1.5, vl, cl, THROW_GRAVITY, 0.5);
        assert!((r.x + l.x).abs() < 1e-3);
        assert!((r.z - l.z).abs() < 1e-3);
    }

    #[test]
    fn lob_goes_high_and_lands() {
        let (v, c) = launch(Vec3::NEG_Z, throw(0.0, 0.0, true));
        assert!(v.y > 5.0);
        let mut pos = Vec3::new(0.0, 1.5, 0.0);
        let mut vel = v;
        let mut peak: f32 = 0.0;
        let mut landed = false;
        for _ in 0..240 {
            let r = step(&mut pos, &mut vel, c, FULL_GRAVITY, 1.0 / 60.0, &[]);
            peak = peak.max(pos.y);
            if r.touched {
                landed = true;
                break;
            }
        }
        assert!(peak > 4.0, "peak {peak}");
        assert!(landed);
        assert!(pos.z < -5.0, "lob should carry forward, z={}", pos.z);
    }

    #[test]
    fn bounces_off_wall() {
        let wall = Aabb::from_center(Vec3::new(0.0, 2.0, -5.0), Vec3::new(5.0, 2.0, 0.5));
        let mut pos = Vec3::new(0.0, 1.5, 0.0);
        let mut vel = Vec3::new(0.0, 0.0, -30.0);
        let mut hit = false;
        for _ in 0..30 {
            hit |= step(&mut pos, &mut vel, 0.0, 0.0, 1.0 / 60.0, &[wall]).touched;
        }
        assert!(hit);
        assert!(vel.z > 0.0, "should bounce back");
        assert!(pos.z > -4.5 + BALL_RADIUS - 0.01, "must not pass through, z={}", pos.z);
    }

    #[test]
    fn curved_throw_is_thirty_percent_faster() {
        let (straight, _) = launch(Vec3::NEG_Z, throw(0.5, 0.0, false));
        let (curved, _) = launch(Vec3::NEG_Z, throw(0.5, 1.0, false));
        assert!((curved.length() / straight.length() - 1.3).abs() < 1e-4);
    }

    /// Sideways distance a full curve travels away from (and back to) the aim line.
    fn max_side_offset(curve_accel_scale: f32, speed: f32, offset: f32) -> f32 {
        let lat_v = speed * offset.sin();
        let accel = PREV_CURVE_ACCEL * curve_accel_scale;
        lat_v * lat_v / (2.0 * accel)
    }

    #[test]
    fn curve_bends_forty_percent_more_over_the_same_distance() {
        // Path curvature ~ accel / speed^2 must grow by 1.4 even though the ball is 1.3x faster.
        let old = PREV_CURVE_ACCEL / (26.0_f32 * 26.0);
        let new = CURVE_ACCEL / (26.0_f32 * 1.3 * 26.0 * 1.3);
        assert!((new / old - 1.4).abs() < 1e-3);
        assert!((CURVE_LAUNCH_OFFSET / PREV_CURVE_OFFSET - 1.4).abs() < 1e-4);
        // Sanity: the outward bulge is not absurd at full charge.
        let bulge = max_side_offset(CURVE_ACCEL / PREV_CURVE_ACCEL, MAX_THROW_SPEED * 1.3, CURVE_LAUNCH_OFFSET);
        assert!(bulge < 3.0, "bulge {bulge}");
    }

    #[test]
    fn lob_is_faster_and_arcs_higher() {
        let (v, _) = launch(Vec3::NEG_Z, throw(0.0, 0.0, true));
        let fwd = Vec3::new(v.x, 0.0, v.z).length();
        assert!((fwd / PREV_LOB_FORWARD - 1.3).abs() < 1e-3, "forward x1.3");
        let height_ratio = (v.y * v.y) / (PREV_LOB_UP * PREV_LOB_UP);
        assert!((height_ratio - 1.4).abs() < 0.01, "arc height x1.4: {height_ratio}");
        // Peak stays under the 8 m walls when thrown from hand height.
        let peak = 1.3 + v.y * v.y / (2.0 * FULL_GRAVITY);
        assert!(peak < super::super::layout::WALL_HEIGHT, "peak {peak}");
    }

    #[test]
    fn lob_lands_on_top_of_the_head() {
        let from = Vec3::new(0.0, 1.3, 0.0);
        for dist in [6.0_f32, 12.0, 20.0] {
            let head = Vec3::new(0.0, 1.9, -dist);
            let mut pos = from;
            let mut vel = lob_to(from, head);
            let mut closest = f32::MAX;
            for _ in 0..300 {
                step(&mut pos, &mut vel, 0.0, FULL_GRAVITY, 1.0 / 120.0, &[]);
                closest = closest.min(pos.distance(head));
                if vel.y < 0.0 && pos.y < head.y - 0.5 {
                    break;
                }
            }
            assert!(closest < 0.4, "{dist} m lob missed the head by {closest}");
        }
    }

    #[test]
    fn ricochet_reflects_at_double_speed() {
        let v = ricochet(Vec3::new(3.0, 0.0, -10.0), Vec3::Z);
        assert!((v - Vec3::new(6.0, 0.0, 20.0)).length() < 1e-4);
        assert!(ricochet(Vec3::new(0.0, 0.0, -150.0), Vec3::Z).length() <= MAX_BALL_SPEED + 1e-3);
    }

    #[test]
    fn wall_hits_report_a_normal_floor_hits_do_not() {
        let wall = Aabb::from_center(Vec3::new(0.0, 2.0, -5.0), Vec3::new(5.0, 2.0, 0.5));
        let (mut pos, mut vel) = (Vec3::new(0.0, 1.5, 0.0), Vec3::new(0.0, 0.0, -30.0));
        let mut normal = None;
        for _ in 0..30 {
            let r = step(&mut pos, &mut vel, 0.0, 0.0, 1.0 / 60.0, &[wall]);
            normal = normal.or(r.wall);
        }
        assert_eq!(normal, Some(Vec3::Z));
        let (mut pos, mut vel) = (Vec3::new(0.0, 0.5, 0.0), Vec3::new(0.0, -10.0, 0.0));
        let r = step(&mut pos, &mut vel, 0.0, 0.0, 0.1, &[]);
        assert!(r.touched && r.wall.is_none());
    }

    #[test]
    fn stacks_add_base_speed_up_to_four() {
        assert_eq!(stack_multiplier(0), 1.0);
        assert_eq!(stack_multiplier(1), 2.0);
        assert_eq!(stack_multiplier(4), 5.0);
        assert_eq!(stack_multiplier(9), 5.0);
    }

    #[test]
    fn waypoint_goes_around_cover_on_the_requested_side() {
        let pillar = Aabb::from_center(Vec3::new(0.0, 1.5, -5.0), Vec3::new(1.0, 1.5, 1.0));
        let pos = Vec3::new(0.0, 1.3, 0.0);
        let target = Vec3::new(0.0, 1.3, -10.0);
        let right = wrap_waypoint(pos, target, 1.0, &[pillar]).unwrap();
        let left = wrap_waypoint(pos, target, -1.0, &[pillar]).unwrap();
        assert!(right.x > 1.0 && left.x < -1.0, "{right} {left}");
        // Clear line: no waypoint.
        assert!(wrap_waypoint(Vec3::new(5.0, 1.3, 0.0), Vec3::new(5.0, 1.3, -10.0), 1.0, &[pillar]).is_none());
    }

    #[test]
    fn steer_flat_turns_at_rate_and_keeps_height_speed() {
        let mut v = Vec3::new(0.0, 2.0, -10.0);
        steer_flat(&mut v, Vec3::X, 1.0, 0.1);
        assert_eq!(v.y, 2.0);
        assert!((Vec3::new(v.x, 0.0, v.z).length() - 10.0).abs() < 1e-4);
        assert!(v.x > 0.0);
    }

    #[test]
    fn brick_and_powered_damage() {
        assert_eq!(BallKind::Normal.damage(false), 1);
        assert_eq!(BallKind::Normal.damage(true), 2);
        assert_eq!(BallKind::Brick.damage(false), 4);
        assert_eq!(BallKind::Slime.damage(false), 1);
    }

    #[test]
    fn homing_turns_toward_target_at_limited_rate() {
        let mut vel = Vec3::new(0.0, 0.0, -30.0);
        home(&mut vel, Vec3::X, 0.1, false);
        assert!((vel.length() - 30.0).abs() < 1e-3, "keeps speed");
        let turned = Vec3::NEG_Z.angle_between(vel.normalize());
        assert!((turned - HOMING_RATE * 0.1).abs() < 1e-3);
    }

    #[test]
    fn flat_homing_keeps_vertical_speed() {
        let mut vel = Vec3::new(0.0, 10.0, -9.0);
        home(&mut vel, Vec3::X, 1.0, true);
        assert_eq!(vel.y, 10.0);
        assert!(vel.x > 0.0);
    }

    #[test]
    fn boomerang_flies_back() {
        let mut pos = Vec3::new(0.0, 1.0, -10.0);
        let mut vel = Vec3::ZERO;
        let target = Vec3::new(0.0, 1.3, 0.0);
        let mut arrived = false;
        for _ in 0..60 {
            if fly_back(&mut pos, &mut vel, target, 1.0 / 60.0) {
                arrived = true;
                break;
            }
        }
        assert!(arrived);
        assert_eq!(pos, target);
    }

    #[test]
    fn dead_ball_comes_to_rest() {
        let mut pos = Vec3::new(0.0, 2.0, 0.0);
        let mut vel = Vec3::new(3.0, 0.0, 0.0);
        for _ in 0..600 {
            step(&mut pos, &mut vel, 0.0, FULL_GRAVITY, 1.0 / 60.0, &[]);
        }
        assert_eq!(vel, Vec3::ZERO);
        assert!((pos.y - BALL_RADIUS).abs() < 0.02);
    }
}
