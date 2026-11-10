//! Simple bots so one person can test a 2-4 player match.
//! They fetch balls, keep a throwing distance, throw (sometimes curved or lobbed),
//! and try to catch incoming balls.

use bevy::math::Vec3;

use super::layout::{Aabb, ARENA_HALF};
use super::{Action, BallState, Match, PlayerId, Team, BODY_RADIUS};

pub const BOT_SPEED: f32 = 5.5;
const THROW_RANGE: f32 = 22.0;
const PREFERRED_RANGE: f32 = 13.0;
const THREAT_RANGE: f32 = 8.0;
/// Chance a bot tries to catch a given incoming ball.
const CATCH_SKILL: f32 = 0.35;
/// Press catch when the ball is this many seconds away (inside the catch window).
const CATCH_LEAD_SECS: f32 = 0.25;
/// Chance a bot locks on before a throw.
const BOT_LOCK_CHANCE: f32 = 0.35;

#[derive(Clone, Debug, Default)]
pub struct Brain {
    pub wander: Option<Vec3>,
    pub aim_timer: f32,
    pub strafe: f32,
    pub strafe_timer: f32,
    /// Ball index we already decided about, and whether we go for the catch.
    pub judged: Option<(usize, bool)>,
    /// Which way we go around obstacles (+1 left, -1 right); sticks so we don't jitter.
    pub detour: f32,
    /// Whether we lock on for the throw we are holding (rolled once per ball).
    pub lock_this_throw: Option<bool>,
}

pub fn yaw_toward(dir: Vec3) -> f32 {
    (-dir.x).atan2(-dir.z)
}

/// Decide this frame's movement (applied directly) and actions (returned).
pub fn think(m: &mut Match, id: PlayerId, dt: f32) -> Vec<Action> {
    let Some(pi) = m.players.iter().position(|p| p.id == id) else { return Vec::new() };
    let me = m.players[pi].clone();
    let mut brain = me.brain.clone();
    let mut actions = Vec::new();
    let chest = me.pos + Vec3::Y * 1.1;

    let enemy = m
        .players
        .iter()
        .filter(|p| p.team != me.team && p.alive())
        .min_by(|a, b| a.pos.distance(me.pos).total_cmp(&b.pos.distance(me.pos)))
        .map(|p| (p.id, p.pos));

    // Lock on to the nearest enemy for about half of our throws; let go otherwise.
    if me.holding.is_none() {
        brain.lock_this_throw = None;
    }
    let lock_roll = *brain.lock_this_throw.get_or_insert_with(|| m.rng.f32() < BOT_LOCK_CHANCE);
    let want_lock = if me.holding.is_some() && lock_roll { enemy.map(|(id, _)| id) } else { None };
    if want_lock != me.lock {
        actions.push(Action::Lock { target: want_lock });
    }
    let enemy = enemy.map(|(_, pos)| pos);

    // Closest enemy ball flying at us.
    let threat = m
        .balls
        .iter()
        .enumerate()
        .filter(|(_, b)| matches!(b.state, BallState::Live { team, .. } if team != me.team))
        .filter(|(_, b)| {
            let to_me = chest - b.pos;
            to_me.length() < THREAT_RANGE && b.vel.normalize_or_zero().dot(to_me.normalize_or_zero()) > 0.85
        })
        .map(|(i, b)| (i, b.pos, b.vel.length()))
        .next();

    let mut face: Option<Vec3> = None;
    let mut goal: Option<Vec3> = None;

    if let Some((index, ball_pos, speed)) = threat {
        face = Some(ball_pos - me.pos);
        let go = match brain.judged {
            Some((i, go)) if i == index => go,
            _ => {
                let go = m.rng.f32() < CATCH_SKILL;
                brain.judged = Some((index, go));
                go
            }
        };
        let eta = ball_pos.distance(chest) / speed.max(1.0);
        if go && eta < CATCH_LEAD_SECS && me.holding.is_none() && me.catch_cooldown <= 0.0 {
            actions.push(Action::Catch);
        }
    } else {
        brain.judged = None;
    }

    if me.holding.is_some() {
        if let Some(e) = enemy {
            let to_e = e - me.pos;
            let flat = Vec3::new(to_e.x, 0.0, to_e.z);
            let dist = flat.length();
            face.get_or_insert(flat);
            brain.strafe_timer -= dt;
            if brain.strafe_timer <= 0.0 {
                brain.strafe = if m.rng.f32() < 0.5 { -1.0 } else { 1.0 };
                brain.strafe_timer = 0.8 + m.rng.f32() * 1.2;
            }
            let side = flat.normalize_or_zero().cross(Vec3::Y) * brain.strafe * 3.0;
            let approach = flat.normalize_or_zero() * (dist - PREFERRED_RANGE);
            goal = Some(me.pos + approach + side);

            brain.aim_timer -= dt;
            if brain.aim_timer <= 0.0 && dist < THROW_RANGE && me.throw_cooldown <= 0.0 {
                let roll = m.rng.f32();
                let lob = roll < 0.12;
                let curve = if roll > 0.75 { if m.rng.f32() < 0.5 { -0.8 } else { 0.8 } } else { 0.0 };
                // Aim at the chest; drop the aim a touch to account for gravity over distance.
                let target = e + Vec3::Y * (1.0 + dist * 0.01);
                let aim = target - me.hold_point();
                actions.push(Action::Throw {
                    aim: aim.to_array(),
                    charge: 0.3 + m.rng.f32() * 0.7,
                    curve,
                    lob,
                });
                brain.aim_timer = 0.8 + m.rng.f32() * 1.2;
            }
        }
    } else {
        brain.aim_timer = brain.aim_timer.max(0.4);
        let free = m
            .balls
            .iter()
            .filter(|b| b.state == BallState::Free)
            .min_by(|a, b| a.pos.distance(me.pos).total_cmp(&b.pos.distance(me.pos)))
            .map(|b| b.pos);
        goal = free.or_else(|| {
            let reached = brain.wander.is_none_or(|w| w.distance(me.pos) < 1.0);
            if reached {
                let side = if me.team == Team::Red { 1.0 } else { -1.0 };
                let x = (m.rng.f32() - 0.5) * 2.0 * (ARENA_HALF - 3.0);
                let z = side * (3.0 + m.rng.f32() * (ARENA_HALF - 6.0));
                brain.wander = Some(Vec3::new(x, 0.0, z));
            }
            brain.wander
        });
    }

    let mut pos = me.pos;
    let mut yaw = me.yaw;
    if me.slime.is_some() {
        // Stuck in slime: the sim moves us; we can only turn and throw.
        goal = None;
    }
    if let Some(g) = goal {
        let d = Vec3::new(g.x - pos.x, 0.0, g.z - pos.z);
        if d.length() > 0.2 {
            let want = (BOT_SPEED * dt).min(d.length());
            let (next, side) = steer(pos, d.normalize() * want, brain.detour, &m.solids);
            brain.detour = side;
            pos = next;
            face.get_or_insert(d);
        }
    }
    pos = collide_feet(pos, &m.solids);
    pos.y = 0.0;
    if let Some(f) = face {
        if f.x != 0.0 || f.z != 0.0 {
            yaw = yaw_toward(f);
        }
    }

    let p = &mut m.players[pi];
    p.pos = pos;
    p.yaw = yaw;
    p.brain = brain;
    actions
}

/// Move by `step`, going around cover: try straight, then angled, then sideways,
/// preferring the side we used last time. Returns the new position and the side used.
pub fn steer(pos: Vec3, step: Vec3, side: f32, solids: &[Aabb]) -> (Vec3, f32) {
    let side = if side == 0.0 { 1.0 } else { side };
    let want = step.length();
    let mut best = (collide_feet(pos + step, solids), side);
    for s in [side, -side] {
        for angle in [0.0_f32, 0.8, 1.57, 2.2] {
            let dir = bevy::math::Quat::from_rotation_y(angle * s) * step;
            let next = collide_feet(pos + dir, solids);
            let progress = Vec3::new(next.x - pos.x, 0.0, next.z - pos.z).length();
            let need = if angle == 0.0 { 0.9 } else { 0.5 };
            if progress >= want * need {
                return (next, if angle == 0.0 { side } else { s });
            }
            if progress > Vec3::new(best.0.x - pos.x, 0.0, best.0.z - pos.z).length() {
                best = (next, s);
            }
        }
    }
    best
}

/// Keep a bot's feet out of solids and inside the arena (2D, ignores box height above head).
pub fn collide_feet(mut pos: Vec3, solids: &[Aabb]) -> Vec3 {
    for s in solids {
        let e = s.expanded(BODY_RADIUS);
        if pos.x > e.min.x && pos.x < e.max.x && pos.z > e.min.z && pos.z < e.max.z {
            let pens = [pos.x - e.min.x, e.max.x - pos.x, pos.z - e.min.z, e.max.z - pos.z];
            let i = (0..4).min_by(|&a, &b| pens[a].total_cmp(&pens[b])).unwrap();
            match i {
                0 => pos.x = e.min.x,
                1 => pos.x = e.max.x,
                2 => pos.z = e.min.z,
                _ => pos.z = e.max.z,
            }
        }
    }
    let lim = ARENA_HALF - BODY_RADIUS;
    pos.x = pos.x.clamp(-lim, lim);
    pos.z = pos.z.clamp(-lim, lim);
    pos
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{Phase, KO_TARGET};

    #[test]
    fn yaw_matches_forward() {
        for dir in [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z, Vec3::new(1.0, 0.0, 1.0).normalize()] {
            let yaw = yaw_toward(dir);
            let fwd = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
            assert!(fwd.abs_diff_eq(dir, 1e-5), "{dir} -> {fwd}");
        }
    }

    #[test]
    fn steers_around_wall() {
        // Wall straight ahead (-Z); the bot must still make progress.
        let wall = Aabb::from_center(Vec3::new(0.0, 0.6, -1.0), Vec3::new(3.0, 0.6, 0.4));
        let mut pos = Vec3::ZERO;
        let mut side = 0.0;
        for _ in 0..400 {
            let goal = Vec3::new(0.0, 0.0, -5.0);
            let d = (goal - pos).normalize() * 0.1;
            (pos, side) = steer(pos, d, side, &[wall]);
            pos = collide_feet(pos, &[wall]);
        }
        assert!(pos.z < -2.0, "got around the wall: {pos}");
    }

    #[test]
    fn feet_pushed_out_of_cover() {
        let b = Aabb::from_center(Vec3::new(0.0, 1.0, 0.0), Vec3::ONE);
        let p = collide_feet(Vec3::new(0.9, 0.0, 0.1), &[b]);
        assert!(p.x >= 1.0 + BODY_RADIUS - 1e-5);
    }

    /// Two bots playing each other must finish a match: proves the whole loop works.
    #[test]
    fn bot_match_finishes() {
        let mut m = Match::new();
        for i in 0..4 {
            m.add_player(&format!("bot{i}"), true);
        }
        assert!(m.start());
        let dt = 1.0 / 60.0;
        let mut secs = 0.0;
        while m.phase == Phase::Playing && secs < 900.0 {
            m.step(dt);
            m.events.clear();
            secs += dt;
        }
        assert!(matches!(m.phase, Phase::Ended { .. }), "score after {secs}s: {:?}", m.score);
        eprintln!("bot match took {secs:.0}s, score {:?}", m.score);
        assert!(m.score.contains(&KO_TARGET));
        for p in &m.players {
            assert!(p.pos.is_finite());
        }
    }
}

