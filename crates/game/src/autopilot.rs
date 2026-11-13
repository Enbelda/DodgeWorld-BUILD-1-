//! `--autopilot`: drives the local player through the normal client path (move intent,
//! camera yaw, queued actions) so the real app can be play-tested without a human.
//! It cycles through straight, curved and lobbed throws, goes for perfect catches, sprints,
//! dashes now and then, and every few seconds does a double-jump flip or an air spin.
//!
//! With `--drill` it instead loops a parkour drill on the +X flank: wall run along the outer
//! wall, wall jump across to the panel and wall run on it, then climb the panel, hang from
//! its ledge (throwing from there if holding a ball), shimmy, climb up on top and drop off.

use bevy::prelude::*;

use crate::camera::FollowCamera;
use crate::parkour;
use crate::player::{Autopilot, LocalBody, LocalParkour, LockOn, MoveIntent, Side, FEET_OFFSET};
use crate::session::{LocalActions, View};
use crate::sim::{bot::yaw_toward, Action, BallState, HOLD_HEIGHT};

const THROW_EVERY: f32 = 1.4;
const PREFERRED_RANGE: f32 = 12.0;

/// `--drill`: the autopilot runs the parkour drill.
#[derive(Resource, Default)]
pub struct ParkourDrill(pub bool);

/// Where the parkour drill is.
#[derive(Default)]
pub struct DrillState {
    step: u32,
    /// Seconds in the current step.
    t: f32,
    laps: u32,
    /// The jump for this step was already pressed (a key press lasts one frame).
    jumped: bool,
}

#[derive(Default)]
pub struct AutoState {
    throw_timer: f32,
    throws: u32,
    last_pos: Option<Vec3>,
    caught_attempts: u32,
    was_incoming: bool,
    strafe_timer: f32,
    strafe: f32,
    /// Trick routine: counts down to the next one; `trick_step` walks through its inputs.
    trick_timer: f32,
    trick_step: u32,
    tricks: u32,
    dash_timer: f32,
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
    mut lock: ResMut<LockOn>,
    mut st: Local<AutoState>,
    drill_on: Res<ParkourDrill>,
    pk: Res<LocalParkour>,
    mut ds: Local<DrillState>,
) {
    if !auto.0 {
        return;
    }
    let dt = time.delta_secs();
    *intent = MoveIntent::default();
    let (Some(snap), Some(me)) = (view.snap.as_ref(), view.me()) else { return };
    if !me.alive() {
        ds.step = 0;
        return;
    }
    let feet = body.translation - Vec3::Y * FEET_OFFSET;
    if drill_on.0 {
        let enemy = snap
            .players
            .iter()
            .filter(|p| p.team != me.team && p.alive())
            .map(|p| Vec3::from_array(p.pos))
            .min_by(|a, b| a.distance(feet).total_cmp(&b.distance(feet)));
        drill(&mut ds, dt, feet, me.holding, enemy, &pk.0, &mut cam, &mut intent, &mut actions);
        return;
    }

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
            let speed = Vec3::from_array(b.vel).length().max(1.0);
            (p.distance(chest) < 7.0).then_some((p, p.distance(chest) / speed))
        }
        _ => None,
    });
    // Every other incoming ball: wait for the perfect-catch moment, then press catch.
    if incoming.is_some() && !st.was_incoming {
        st.caught_attempts += 1;
    }
    st.was_incoming = incoming.is_some();
    let eta = crate::hud::closest_eta(snap, me);
    let incoming = incoming.map(|(p, _)| {
        cam.yaw = yaw_toward(Vec3::new(p.x - feet.x, 0.0, p.z - feet.z));
        let go = st.caught_attempts % 2 == 1;
        let perfect_now = eta.is_some_and(|e| e <= crate::sim::PERFECT_SECS * 0.7);
        if go && perfect_now && !me.holding && !me.catching && me.catch_cooldown <= 0.0 {
            info!("autopilot: going for a perfect catch");
            actions.0.push(Action::Catch);
        }
        p
    });

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
            // Sprint when the goal is far; dash toward it every few seconds.
            intent.sprint = d.length() > 6.0;
            st.dash_timer -= dt;
            if st.dash_timer <= 0.0 && d.length() > 4.0 {
                st.dash_timer = 5.0;
                intent.dash = true;
                info!("autopilot: dash");
            }
            // Blocked (e.g. by the low wall)? Jump over it.
            if let Some(last) = st.last_pos {
                let moved = Vec3::new(feet.x - last.x, 0.0, feet.z - last.z).length();
                intent.jump = moved < 7.0 * dt * 0.3;
            }
        }
    }
    st.last_pos = Some(feet);

    // Empty-handed with an enemy close by: dash into them for a tackle.
    if let Some(e) = enemy.filter(|_| !me.holding) {
        let to = Vec3::new(e.x - feet.x, 0.0, e.z - feet.z);
        if to.length() < 4.5 && st.dash_timer <= 2.0 {
            intent.dir = to.normalize_or_zero();
            intent.dash = true;
            st.dash_timer = 5.0;
            info!("autopilot: tackle dash");
        }
    }

    // Every few seconds: double-jump flip, or jump + Q/E spin, alternating.
    st.trick_timer -= dt;
    if st.trick_timer <= 0.0 && incoming.is_none() {
        let spin = st.tricks % 2 == 1;
        match st.trick_step {
            0 => intent.jump = true,
            8 if spin => intent.trick = Some(if st.tricks % 4 == 1 { Side::Left } else { Side::Right }),
            8 => intent.jump = true, // second jump: automatic flip
            _ => {}
        }
        st.trick_step += 1;
        if st.trick_step > 30 {
            info!("autopilot: did {}", if spin { "air spin" } else { "double-jump flip" });
            st.trick_step = 0;
            st.tricks += 1;
            st.trick_timer = 4.0;
        }
    }

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

/// Walk toward `goal` (flat); true once there.
fn walk_to(feet: Vec3, goal: Vec3, cam: &mut FollowCamera, intent: &mut MoveIntent) -> bool {
    let d = Vec3::new(goal.x - feet.x, 0.0, goal.z - feet.z);
    if d.length() < 0.4 {
        return true;
    }
    intent.dir = d.normalize();
    cam.yaw = yaw_toward(d);
    false
}

/// One frame of the parkour drill. Steps:
/// 0 go to the corridor, 1-2 jump and wall run along the outer wall, 3 wall jump across and
/// wall run on the panel, 4-6 walk round to the panel's open face (over the slime pad, to
/// pick up a ball), 7 jump and climb into a hang, 8 throw from the hang, 9 shimmy,
/// 10 climb up on top, 11 walk off.
#[allow(clippy::too_many_arguments)]
fn drill(
    ds: &mut DrillState,
    dt: f32,
    feet: Vec3,
    holding: bool,
    enemy: Option<Vec3>,
    pk: &parkour::Parkour,
    cam: &mut FollowCamera,
    intent: &mut MoveIntent,
    actions: &mut LocalActions,
) {
    const CORRIDOR: Vec3 = Vec3::new(19.0, 0.0, 10.0);
    let along_outer = Vec3::new(0.25, 0.0, -1.0).normalize();
    let to_panel = Vec3::new(-0.25, 0.0, -1.0).normalize();
    let path = [Vec3::new(17.5, 0.0, -6.0), Vec3::new(13.0, 0.0, -6.0), Vec3::new(13.0, 0.0, -0.8)];
    let face = Vec3::new(14.5, 0.0, -0.5);
    ds.t += dt;
    let pose = pk.pose();
    let next = |ds: &mut DrillState, why: &str| {
        info!("autopilot drill: step {} done ({why})", ds.step);
        ds.step += 1;
        ds.t = 0.0;
        ds.jumped = false;
    };
    let fail = |ds: &mut DrillState, why: &str| {
        info!("autopilot drill: step {} failed ({why}), starting over", ds.step);
        ds.step = 0;
        ds.t = 0.0;
        ds.jumped = false;
    };
    match ds.step {
        0 => {
            if walk_to(feet, CORRIDOR, cam, intent) && feet.y < 0.1 {
                next(ds, "in the corridor");
            } else if ds.t > 20.0 {
                fail(ds, "could not reach the corridor");
            }
        }
        1 => {
            cam.yaw = yaw_toward(Vec3::NEG_Z);
            intent.dir = along_outer;
            intent.jump = true;
            next(ds, "jumped");
        }
        2 => {
            cam.yaw = yaw_toward(Vec3::NEG_Z);
            intent.dir = along_outer;
            let running = matches!(pose, crate::sim::ParkourPose::WallRunLeft | crate::sim::ParkourPose::WallRunRight);
            if running && ds.t > 0.45 {
                intent.jump = true; // wall jump toward the panel
                next(ds, "wall ran on the outer wall, wall jump");
            } else if ds.t > 1.5 {
                fail(ds, "no wall run");
            }
        }
        3 => {
            cam.yaw = yaw_toward(Vec3::NEG_Z);
            intent.dir = to_panel;
            if matches!(pk.state, parkour::State::WallRun { normal, .. } if normal == Vec3::X) {
                info!("autopilot drill: wall-to-wall: running on the panel");
            }
            if ds.t > 1.2 && feet.y < 0.1 {
                next(ds, "landed");
            } else if ds.t > 4.0 {
                fail(ds, "never landed");
            }
        }
        4..=6 => {
            if walk_to(feet, path[(ds.step - 4) as usize], cam, intent) {
                next(ds, "waypoint");
            } else if ds.t > 12.0 {
                fail(ds, "stuck on the way to the panel");
            }
        }
        7 => {
            if ds.t < 0.6 {
                walk_to(feet, face, cam, intent);
                return;
            }
            cam.yaw = yaw_toward(Vec3::X);
            intent.dir = Vec3::X;
            intent.jump = !ds.jumped;
            ds.jumped = true;
            if pose == crate::sim::ParkourPose::Hang {
                next(ds, "climbed into a ledge hang");
            } else if ds.t > 3.0 {
                fail(ds, "no hang");
            }
        }
        8 => {
            cam.yaw = yaw_toward(Vec3::X);
            if pose != crate::sim::ParkourPose::Hang {
                fail(ds, "fell off");
            } else if ds.t > 0.6 {
                if let (true, Some(e)) = (holding, enemy) {
                    let hold = feet + Vec3::Y * HOLD_HEIGHT;
                    let aim = (e + Vec3::Y * 1.0 - hold).normalize_or(Vec3::NEG_X).to_array();
                    cam.yaw = yaw_toward(Vec3::new(e.x - feet.x, 0.0, e.z - feet.z));
                    intent.face_camera = true;
                    actions.0.push(Action::Throw { aim, charge: 1.0, curve: 0.0, lob: false });
                    info!("autopilot: threw while hanging");
                }
                next(ds, "hung on");
            }
        }
        9 => {
            cam.yaw = yaw_toward(Vec3::X);
            intent.dir = Vec3::NEG_Z; // shimmy (along the ledge)
            if ds.t > 0.6 {
                next(ds, "shimmied");
            }
        }
        10 => {
            cam.yaw = yaw_toward(Vec3::X);
            // One press to climb up, then stand still on the narrow top.
            intent.dir = if ds.jumped { Vec3::ZERO } else { Vec3::X };
            intent.jump = !ds.jumped;
            ds.jumped = true;
            if ds.t > 0.8 {
                if feet.y > 5.0 {
                    info!("autopilot: on top of the panel (y {:.2})", feet.y);
                    next(ds, "climbed up");
                } else {
                    fail(ds, "did not climb up");
                }
            }
        }
        _ => {
            cam.yaw = yaw_toward(Vec3::NEG_X);
            intent.dir = Vec3::NEG_X;
            if ds.t > 1.5 {
                ds.laps += 1;
                info!("autopilot drill: lap {} complete", ds.laps);
                ds.step = 0;
                ds.t = 0.0;
            }
        }
    }
}
