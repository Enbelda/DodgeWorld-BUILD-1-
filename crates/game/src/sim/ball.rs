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
    let max_turn = HOMING_RATE * dt;
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepResult {
    /// The ball touched the floor or a solid this step.
    pub touched: bool,
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
    let steps = ((travel / (BALL_RADIUS * 0.8)).ceil() as usize).clamp(1, 16);
    let h = dt / steps as f32;
    let mut touched = false;
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
            if resolve_box(pos, vel, s) {
                touched = true;
            }
        }
    }
    if pos.y <= BALL_RADIUS + 0.01 && vel.length() < SLEEP_SPEED {
        *vel = Vec3::ZERO;
    }
    StepResult { touched }
}

/// Push the ball out of a box along the shallowest axis and bounce.
fn resolve_box(pos: &mut Vec3, vel: &mut Vec3, b: &Aabb) -> bool {
    let e = b.expanded(BALL_RADIUS);
    if !e.contains(*pos) {
        return false;
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
    true
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
