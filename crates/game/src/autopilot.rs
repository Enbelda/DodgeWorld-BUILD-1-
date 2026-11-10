//! `--autopilot`: drives the local player through the normal client path (move intent,
//! camera yaw, queued actions) so the real app can be play-tested without a human.
//! It cycles through straight, curved and lobbed throws and tries to catch.

use bevy::prelude::*;

use crate::camera::FollowCamera;
use crate::player::{Autopilot, LocalBody, LockOn, MoveIntent, FEET_OFFSET};
use crate::session::{CatchHold, LocalActions, View};
use crate::sim::{bot::yaw_toward, Action, BallState, HOLD_HEIGHT};

const THROW_EVERY: f32 = 1.4;
const PREFERRED_RANGE: f32 = 12.0;

#[derive(Default)]
pub struct AutoState {
    throw_timer: f32,
    throws: u32,
    last_pos: Option<Vec3>,
    caught_attempts: u32,
    was_incoming: bool,
    strafe_timer: f32,
    strafe: f32,
}

#[allow(clippy::too_many_arguments)]
pub fn drive(
    time: Res<Time>,
    auto: Res<Autopilot>,
    view: Res<View>,
    body: Single<&Transform, With<LocalBody>>,
    mut cam: Single<&mut FollowCamera>,
    mut intent: ResMut<MoveIntent>,
    mut actions: ResMut<LocalActions>,
    mut catch_hold: ResMut<CatchHold>,
    mut lock: ResMut<LockOn>,
    mut st: Local<AutoState>,
) {
    if !auto.0 {
        return;
    }
    let dt = time.delta_secs();
    *intent = MoveIntent::default();
    catch_hold.0 = false;
    let (Some(snap), Some(me)) = (view.snap.as_ref(), view.me()) else { return };
    if !me.alive() {
        return;
    }
    let feet = body.translation - Vec3::Y * FEET_OFFSET;

    let enemy = snap
        .players
        .iter()
        .filter(|p| p.team != me.team && p.alive())
        .map(|p| (p.id, Vec3::from_array(p.pos)))
        .min_by(|a, b| a.1.distance(feet).total_cmp(&b.1.distance(feet)));

    // Lock on to the nearest enemy while holding a ball.
    let want_lock = if me.holding { enemy.map(|(id, _)| id) } else { None };
    if want_lock != lock.0 {
        lock.0 = want_lock;
        info!("autopilot: lock {:?}", want_lock);
        actions.0.push(Action::Lock { target: want_lock });
    }
    let enemy = enemy.map(|(_, pos)| pos);

    // Incoming enemy ball close by: face it and try to catch every other one.
    let chest = feet + Vec3::Y * 1.1;
    let incoming = snap.balls.iter().find_map(|b| match b.state {
        BallState::Live { team, .. } if team != me.team => {
            let p = Vec3::from_array(b.pos);
            (p.distance(chest) < 7.0).then_some(p)
        }
        _ => None,
    });
    // Every other incoming ball: hold catch early and let it arrive.
    if incoming.is_some() && !st.was_incoming {
        st.caught_attempts += 1;
        if st.caught_attempts % 2 == 1 && !me.holding {
            info!("autopilot: holding catch");
        }
    }
    st.was_incoming = incoming.is_some();
    if let Some(p) = incoming {
        cam.yaw = yaw_toward(Vec3::new(p.x - feet.x, 0.0, p.z - feet.z));
        catch_hold.0 = !me.holding && st.caught_attempts % 2 == 1;
    }

    let goal = if me.holding {
        enemy.map(|e| {
            let to = Vec3::new(e.x - feet.x, 0.0, e.z - feet.z);
            st.strafe_timer -= dt;
            if st.strafe_timer <= 0.0 {
                st.strafe = if st.strafe > 0.0 { -1.0 } else { 1.0 };
                st.strafe_timer = 1.5;
            }
            feet + to.normalize_or_zero() * (to.length() - PREFERRED_RANGE)
                + to.normalize_or_zero().cross(Vec3::Y) * st.strafe * 3.0
        })
    } else {
        snap.balls
            .iter()
            .filter(|b| b.state == BallState::Free)
            .map(|b| Vec3::from_array(b.pos))
            .min_by(|a, b| a.distance(feet).total_cmp(&b.distance(feet)))
    };

    if let Some(g) = goal {
        let d = Vec3::new(g.x - feet.x, 0.0, g.z - feet.z);
        if d.length() > 0.5 {
            intent.dir = d.normalize();
            // Blocked (e.g. by the low wall)? Jump over it.
            if let Some(last) = st.last_pos {
                let moved = Vec3::new(feet.x - last.x, 0.0, feet.z - last.z).length();
                intent.jump = moved < 7.0 * dt * 0.3;
            }
        }
    }
    st.last_pos = Some(feet);

    if me.holding {
        if let Some(e) = enemy {
            let flat = Vec3::new(e.x - feet.x, 0.0, e.z - feet.z);
            if incoming.is_none() {
                cam.yaw = yaw_toward(flat);
            }
            st.throw_timer -= dt;
            if st.throw_timer <= 0.0 && flat.length() < 24.0 {
                st.throw_timer = THROW_EVERY;
                let forward = Vec3::new(-cam.yaw.sin(), 0.0, -cam.yaw.cos());
                let hold = feet + Vec3::Y * HOLD_HEIGHT + forward * 0.45;
                let aim = (e + Vec3::Y * 1.0 - hold).normalize_or(forward).to_array();
                let kind = st.throws % 4;
                st.throws += 1;
                let (curve, lob, name) = match kind {
                    0 => (0.0, false, "straight"),
                    1 => (-1.0, false, "curve left"),
                    2 => (1.0, false, "curve right"),
                    _ => (0.0, true, "lob"),
                };
                info!("autopilot: throw {name}{}", if me.powered { " (powered)" } else { "" });
                actions.0.push(Action::Throw { aim, charge: 1.0, curve, lob });
            }
        }
    } else if incoming.is_none() && intent.dir != Vec3::ZERO {
        cam.yaw = yaw_toward(intent.dir);
    }
}
