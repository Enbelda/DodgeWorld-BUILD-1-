//! Ball flight: gravity, curve, lob arcs and bounces off the arena.

use bevy::math::Vec3;

use super::layout::Aabb;

pub const BALL_RADIUS: f32 = 0.3;
/// Gravity on a straight throw: low so fast throws fly mostly flat.
pub const THROW_GRAVITY: f32 = 4.0;
/// Full gravity once a ball is dead or lobbed.
pub const FULL_GRAVITY: f32 = 18.0;
pub const MIN_THROW_SPEED: f32 = 20.0;
pub const MAX_THROW_SPEED: f32 = 32.0;
/// Extra speed for a ball that was just caught.
pub const POWER_BONUS: f32 = 6.0;
/// Sideways acceleration at full curve, m/s^2.
pub const CURVE_ACCEL: f32 = 26.0;
pub const LOB_SPEED: f32 = 14.0;
pub const LOB_ANGLE: f32 = 0.85; // radians above the aim direction
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
        let pitch = aim.y.clamp(-0.5, 0.5).asin() + LOB_ANGLE;
        let dir = flat * pitch.cos() + Vec3::Y * pitch.sin();
        return (dir * LOB_SPEED, 0.0);
    }
    let mut speed = MIN_THROW_SPEED + (MAX_THROW_SPEED - MIN_THROW_SPEED) * p.charge.clamp(0.0, 1.0);
    if p.powered {
        speed += POWER_BONUS;
    }
    // A curved ball starts angled away so it bends back toward the aim point.
    let right = aim.cross(Vec3::Y).normalize_or(Vec3::X);
    let curve = p.curve.clamp(-1.0, 1.0);
    let dir = (aim - right * curve * 0.22).normalize();
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
