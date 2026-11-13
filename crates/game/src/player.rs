//! The local player's body (Rapier character controller) and combat input.
//!
//! Controls: WASD move, Shift sprint, Ctrl dash (tackles), Space jump (again in the air to
//! double jump with a flip), hold Left Mouse to charge (locks the enemy nearest the crosshair, else the nearest one;
//! move the mouse toward another enemy while charging to switch, or Tab / Middle Mouse)
//! and release to throw, hold Q / E while throwing to curve, F while charging to lob,
//! Right Mouse to catch (time it).
//! Cover (see `cover.rs`): F next to a waist-high wall to crouch behind it, A/D to slide,
//! charge to rise and throw over it; F again or sprint, jump or dash to leave.
//! Mounts (see `ride.rs`, `mounts.rs`): 1-6 hop on a skateboard, longboard, BMX, inline
//! skates, scooter or soap shoes (same key or X hops off). Riding: WASD carve / push / brake,
//! Space ollie, in the air Q + keys flip trick, E + keys grab, R + W/S front/back flip;
//! hold F coming down onto a rail or ledge to grind. Throwing and catching still work.
//! Parkour (see `parkour.rs`): run along a tall wall in the air to wall run, push into it to
//! climb, grab ledges at hand height, Space to climb up / jump off, Ctrl to let go.
//! In the air: Q / E spin the body 180 left / right (the camera stays put); W+Q/E front flip,
//! S+Q/E back flip. After a perfect catch (or a returning boomerang) the ball is already
//! fully charged: a click throws it at once.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use bevy::input::mouse::AccumulatedMouseMotion;

use crate::camera::{CameraShake, CursorLocked, FollowCamera};
use crate::cover::{self, Cover};
use crate::mounts::{Dir, MountKind, MountLibrary, TrickInput};
use crate::ride::{Ride, RideEvent, RideInput};
use crate::parkour::{self, Drive, Parkour};
use crate::session::{LocalActions, LocalTrick, View};
use crate::sim::bot::yaw_toward;
use crate::sim::{
    layout, sewer, Action, ArenaId, GameEvent, PlayerId, TrickKind, TrickState, BODY_HEIGHT, BODY_RADIUS, HOLD_HEIGHT,
    LOCK_RANGE, RECOIL_DECAY,
};

const MOVE_SPEED: f32 = 7.0;
/// Shift: run this much faster.
pub const SPRINT_MULT: f32 = 1.4;
/// Jumping or dashing while sprinting goes this much farther.
pub const SPRINT_JUMP_MULT: f32 = 1.24;
/// Dash: distance, duration, charges and cooldowns.
pub const DASH_DIST: f32 = 6.0;
pub const DASH_SECS: f32 = 0.12;
pub const DASH_CHARGES: u8 = 2;
pub const DASH_GAP: f32 = 0.2;
pub const DASH_RECHARGE: f32 = 1.5;
const JUMP_SPEED: f32 = 8.0;
const DOUBLE_JUMP_SPEED: f32 = 7.5;
/// Extra jumps allowed in the air.
pub const AIR_JUMPS: u8 = 1;
const GRAVITY: f32 = 22.0;
/// How long a flip or spin takes.
pub const TRICK_SECS: f32 = 0.55;
/// How much of the stick counts as "moving forward/backward" for picking a flip.
const FLIP_INPUT: f32 = 0.3;
/// Lock-on picks the enemy closest to the crosshair inside this cone (radians).
pub const LOCK_CONE: f32 = 0.6;
/// How quickly the camera turns toward a locked target (higher = snappier).
const LOCK_CAMERA_RATE: f32 = 5.0;
/// The camera stops pulling toward the target for this long after the mouse moves, so the
/// mouse can swing over to another enemy.
const LOCK_MOUSE_GRACE: f32 = 0.35;
/// Moving the mouse switches the lock to an enemy at least this much closer (radians) to
/// the crosshair than the current one.
pub const LOCK_SWITCH_MARGIN: f32 = 0.1;
/// Mouse movement (pixels per frame) that counts as aiming.
const MOUSE_AIM_PIXELS: f32 = 1.5;
/// Jump pressed this long before landing still jumps on landing; walking off a ledge you
/// can still jump from the ground for this long.
pub const JUMP_BUFFER_SECS: f32 = 0.12;
pub const COYOTE_SECS: f32 = 0.12;
/// Part of a dash's speed you keep when it ends in the air.
pub const DASH_CARRY: f32 = 0.3;
/// How quickly the body turns to face a wall during parkour / cover.
const WALL_FACE_RATE: f32 = 16.0;
/// Seconds of holding the throw button for a full-power throw.
pub const FULL_CHARGE_SECS: f32 = 1.0;
/// The body's origin sits at the capsule centre, this far above the feet.
pub const FEET_OFFSET: f32 = BODY_HEIGHT / 2.0;

#[derive(Component, Default)]
pub struct LocalBody {
    pub vel_y: f32,
    pub air_jumps: u8,
    /// Took off while sprinting: the jump carries farther.
    pub sprint_jump: bool,
    /// Last host teleport counter we obeyed.
    pub teleports: Option<u32>,
    /// Horizontal momentum from wall jumps (and the end of an air dash).
    pub push: Vec3,
    /// Push-back from catching a ball (fades on the ground too).
    pub recoil: Vec3,
    /// Seconds since we were last on the ground (coyote time).
    pub air_time: f32,
    /// Seconds left on a buffered jump press.
    pub jump_buffer: f32,
    /// Mid-dash last frame (to carry momentum out of it).
    pub was_dashing: bool,
    /// Riding: this frame's velocity, or where to put the feet (grinding).
    pub ride_vel: Vec3,
    pub ride_place: Option<Vec3>,
}

/// The local player's cover state.
#[derive(Resource, Default)]
pub struct LocalCover {
    pub cover: Option<Cover>,
    /// Crouched behind it right now (a smaller target), or risen up to throw.
    pub crouched: bool,
    pub pose: crate::sim::CoverPose,
    /// F was used for cover this frame (so it doesn't also lob).
    pub toggled: bool,
}

/// The arena the local body moves in (follows the host's choice). Down the hole, `solids`
/// and `rails` are the sewer's instead.
#[derive(Resource, Default)]
pub struct LocalArena {
    pub arena: Option<ArenaId>,
    pub solids: Vec<layout::Aabb>,
    /// Rails and ledges to grind.
    pub rails: Vec<layout::Rail>,
    /// In the sewer right now.
    pub below: bool,
    /// The hole in the middle of the floor is open (sewer race).
    pub hole: bool,
}

/// The local player's mount, and recent trick messages for the HUD.
#[derive(Resource, Default)]
pub struct LocalRide {
    pub ride: Option<Ride>,
    /// The last mount ridden (dropping into the sewer puts you back on it).
    pub last_mount: Option<MountKind>,
    /// (time, message), newest last.
    pub feed: Vec<(f32, String)>,
}

/// Landed combos worth this much are reported to the host (feed, ball charge).
pub const STYLE_REPORT_POINTS: u32 = 300;

/// Seconds since the mouse last moved (lets the mouse override lock-on camera pull).
#[derive(Resource, Default)]
pub struct MouseAim {
    pub since_move: f32,
    pub moved: bool,
}

/// The local player's parkour state.
#[derive(Resource, Default)]
pub struct LocalParkour(pub Parkour);

/// Throw charge in progress, for the HUD.
#[derive(Resource, Default)]
pub struct Charge {
    pub started: Option<f32>,
}

/// Where the local player wants to go this frame (from keyboard or autopilot).
#[derive(Resource, Default)]
pub struct MoveIntent {
    pub dir: Vec3,
    pub jump: bool,
    /// Q (left) or E (right) pressed for an air trick.
    pub trick: Option<Side>,
    /// Shift held.
    pub sprint: bool,
    /// Ctrl pressed.
    pub dash: bool,
    /// Charging or catching: the body turns back to face where the camera looks.
    pub face_camera: bool,
    /// F pressed (not while charging): take / leave cover.
    pub cover: bool,
    /// Charging a throw (stands up in cover).
    pub aiming: bool,
    /// 1-6 pressed: ride that mount (or hop off it); X: hop off.
    pub mount: Option<MountKind>,
    pub dismount: bool,
    /// Riding: raw WASD + Shift for picking tricks, and the trick buttons.
    pub trick_keys: TrickInput,
    pub trick_flip: bool,
    pub trick_grab_start: bool,
    pub trick_grab: bool,
    pub trick_grind: bool,
    pub trick_air_flip: bool,
}

/// How far the body is turned away from the camera's yaw (a spin turns the body only).
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct LocalFacing(pub f32);

impl LocalFacing {
    /// The yaw the body faces, given the camera's.
    pub fn body_yaw(&self, cam_yaw: f32) -> f32 {
        cam_yaw + self.0
    }

    /// Turn back toward the camera's facing (the short way round).
    pub fn settle(&mut self, dt: f32) {
        let mut d = self.0 % std::f32::consts::TAU;
        if d > std::f32::consts::PI {
            d -= std::f32::consts::TAU;
        } else if d < -std::f32::consts::PI {
            d += std::f32::consts::TAU;
        }
        d *= (-FACING_SETTLE_RATE * dt).exp();
        self.0 = if d.abs() < 0.01 { 0.0 } else { d };
    }
}

/// How quickly a spun body turns back to the camera once you move, charge or catch.
const FACING_SETTLE_RATE: f32 = 10.0;

/// The local player's dashes: two charges, a short gap between them, and both come back
/// `DASH_RECHARGE` seconds after the last dash.
#[derive(Resource, Debug, Clone)]
pub struct LocalDash {
    pub charges: u8,
    pub since_last: f32,
    /// Seconds left in the current dash (0 = not dashing).
    pub active: f32,
    pub velocity: Vec3,
    /// Numbers each dash so the host tackles each player once per dash.
    pub seq: u32,
}

impl Default for LocalDash {
    fn default() -> Self {
        LocalDash { charges: DASH_CHARGES, since_last: DASH_RECHARGE, active: 0.0, velocity: Vec3::ZERO, seq: 0 }
    }
}

impl LocalDash {
    /// Start a dash in `dir` if a charge is ready. Sprinting dashes go farther.
    pub fn try_start(&mut self, dir: Vec3, sprint: bool) -> bool {
        if self.charges == 0 || self.since_last < DASH_GAP || self.active > 0.0 {
            return false;
        }
        let dist = if sprint { DASH_DIST * SPRINT_JUMP_MULT } else { DASH_DIST };
        let flat = Vec3::new(dir.x, 0.0, dir.z).normalize_or(Vec3::NEG_Z);
        self.velocity = flat * (dist / DASH_SECS);
        self.charges -= 1;
        self.since_last = 0.0;
        self.active = DASH_SECS;
        self.seq = self.seq.wrapping_add(1);
        true
    }

    pub fn tick(&mut self, dt: f32) {
        self.active = (self.active - dt).max(0.0);
        self.since_last += dt;
        if self.since_last >= DASH_RECHARGE {
            self.charges = DASH_CHARGES;
        }
    }

    pub fn dashing(&self) -> bool {
        self.active > 0.0
    }

    /// Both charges back, ready at once (touching a wall during parkour).
    pub fn refill(&mut self) {
        self.charges = DASH_CHARGES;
        self.since_last = self.since_last.max(DASH_GAP);
    }

    /// What the host needs: the dash number while mid-dash.
    pub fn report(&self) -> Option<u32> {
        self.dashing().then_some(self.seq)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Left,
    Right,
}

/// `--autopilot`: the local player is driven by bot logic (for automated play-tests).
#[derive(Resource, Default)]
pub struct Autopilot(pub bool);

/// The enemy the local player is locked on to.
#[derive(Resource, Default)]
pub struct LockOn(pub Option<PlayerId>);

impl Charge {
    pub fn amount(&self, now: f32) -> Option<f32> {
        self.started.map(|t| ((now - t) / FULL_CHARGE_SECS).clamp(0.0, 1.0))
    }
}

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Charge>()
            .init_resource::<MoveIntent>()
            .init_resource::<Autopilot>()
            .init_resource::<LockOn>()
            .init_resource::<LocalDash>()
            .init_resource::<LocalFacing>()
            .init_resource::<LocalParkour>()
            .init_resource::<LocalCover>()
            .init_resource::<LocalArena>()
            .init_resource::<MouseAim>()
            .init_resource::<LocalRide>()
            .add_systems(Startup, spawn_local_body)
            .add_systems(
                Update,
                (
                    track_arena,
                    obey_teleports,
                    lock_input,
                    keyboard_intent,
                    crate::autopilot::drive,
                    lock_camera,
                    move_local_body,
                    combat_input,
                )
                    .chain()
                    .after(crate::camera::look)
                    .before(crate::session::SessionSet),
            );
    }
}

fn spawn_local_body(mut commands: Commands) {
    let half_segment = (BODY_HEIGHT - 2.0 * BODY_RADIUS) / 2.0;
    // No mesh: the humanoid rig is attached as a child by the renderer.
    commands.spawn((
        LocalBody::default(),
        Visibility::default(),
        Transform::from_translation(layout::spawn_point(ArenaId::Classic, true, 0) + Vec3::Y * FEET_OFFSET),
        Collider::capsule_y(half_segment, BODY_RADIUS),
        KinematicCharacterController {
            offset: CharacterLength::Absolute(0.02),
            snap_to_ground: Some(CharacterLength::Absolute(0.2)),
            autostep: None,
            ..default()
        },
    ));
}

/// Flattened camera yaw turned into a movement direction from WASD.
pub fn input_direction(keys: &ButtonInput<KeyCode>, yaw: f32) -> Vec3 {
    let mut local = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        local.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) {
        local.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) {
        local.x += 1.0;
    }
    if keys.pressed(KeyCode::KeyA) {
        local.x -= 1.0;
    }
    let local = local.normalize_or_zero();
    let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
    let right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
    forward * local.y + right * local.x
}

/// Jump rules: from the ground always; in the air while air jumps remain.
/// Returns the new vertical speed and remaining air jumps, or `None` if no jump happens.
pub fn try_jump(grounded: bool, air_jumps: u8) -> Option<(f32, u8)> {
    if grounded {
        Some((JUMP_SPEED, AIR_JUMPS))
    } else if air_jumps > 0 {
        Some((DOUBLE_JUMP_SPEED, air_jumps - 1))
    } else {
        None
    }
}

/// Which flip a double jump does: back flip when moving backward, front flip otherwise.
/// `forward` is how much the movement points along the camera's facing (-1..1).
pub fn double_jump_flip(forward: f32) -> TrickKind {
    if forward < -FLIP_INPUT {
        TrickKind::BackFlip
    } else {
        TrickKind::FrontFlip
    }
}

/// Which trick Q/E does in the air: W/S held picks a flip, otherwise a 180 spin that way.
pub fn air_trick(forward: f32, side: Side) -> TrickKind {
    if forward > FLIP_INPUT {
        TrickKind::FrontFlip
    } else if forward < -FLIP_INPUT {
        TrickKind::BackFlip
    } else if side == Side::Left {
        TrickKind::SpinLeft
    } else {
        TrickKind::SpinRight
    }
}

/// The closest living enemy within lock range (used when charging starts).
pub fn nearest_enemy(enemies: &[(PlayerId, Vec3)], from: Vec3) -> Option<PlayerId> {
    enemies
        .iter()
        .map(|&(id, pos)| (pos.distance(from), id))
        .filter(|(d, _)| *d <= LOCK_RANGE)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id)
}

/// Tab while charging: the next enemy after `current` (by closeness to the crosshair),
/// wrapping around.
pub fn next_target(enemies: &[(PlayerId, Vec3)], current: Option<PlayerId>, from: Vec3, forward: Vec3) -> Option<PlayerId> {
    let mut by_angle: Vec<(f32, PlayerId)> =
        enemies.iter().map(|&(id, pos)| (forward.angle_between(pos - from), id)).collect();
    by_angle.sort_by(|a, b| a.0.total_cmp(&b.0));
    let ids: Vec<PlayerId> = by_angle.into_iter().map(|(_, id)| id).collect();
    match current.and_then(|c| ids.iter().position(|&id| id == c)) {
        Some(i) => ids.get((i + 1) % ids.len()).copied(),
        None => ids.first().copied(),
    }
}

/// The enemy nearest the crosshair: within `LOCK_RANGE` and `LOCK_CONE` of `forward`.
pub fn pick_lock_target(enemies: &[(PlayerId, Vec3)], from: Vec3, forward: Vec3) -> Option<PlayerId> {
    enemies
        .iter()
        .filter_map(|&(id, pos)| {
            let to = pos - from;
            let angle = forward.angle_between(to);
            (to.length() <= LOCK_RANGE && angle <= LOCK_CONE).then_some((angle, id))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id)
}

/// Moving the mouse while charging: the enemy now clearly closer to the crosshair than
/// `current` (by `LOCK_SWITCH_MARGIN`), inside the lock cone, or `None` to keep the lock.
pub fn mouse_retarget(
    enemies: &[(PlayerId, Vec3)],
    current: Option<PlayerId>,
    from: Vec3,
    forward: Vec3,
) -> Option<PlayerId> {
    let angle = |p: Vec3| forward.angle_between(p - from);
    let cur = current.and_then(|c| enemies.iter().find(|(id, _)| *id == c)).map_or(f32::MAX, |(_, p)| angle(*p));
    enemies
        .iter()
        .filter(|(id, pos)| Some(*id) != current && pos.distance(from) <= LOCK_RANGE)
        .map(|&(id, pos)| (angle(pos), id))
        .filter(|(a, _)| *a <= LOCK_CONE && *a + LOCK_SWITCH_MARGIN < cur)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id)
}

/// Starting a charge: the enemy nearest the crosshair if one is in the cone, else the
/// nearest enemy in range.
pub fn charge_target(enemies: &[(PlayerId, Vec3)], eye: Vec3, forward: Vec3, feet: Vec3) -> Option<PlayerId> {
    pick_lock_target(enemies, eye, forward).or_else(|| nearest_enemy(enemies, feet))
}

/// Shortest-way blend between two angles.
fn lerp_angle(from: f32, to: f32, t: f32) -> f32 {
    let mut d = (to - from) % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    } else if d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    from + d * t
}

/// Snap to the host's position whenever it respawns us.
fn obey_teleports(
    view: Res<View>,
    mut parkour: ResMut<LocalParkour>,
    mut cover: ResMut<LocalCover>,
    body: Single<(&mut LocalBody, &mut Transform)>,
) {
    let Some(me) = view.me() else { return };
    let (mut b, mut tf) = body.into_inner();
    if b.teleports != Some(me.teleports) {
        b.teleports = Some(me.teleports);
        tf.translation = Vec3::from_array(me.pos) + Vec3::Y * FEET_OFFSET;
        b.vel_y = 0.0;
        b.push = Vec3::ZERO;
        b.recoil = Vec3::ZERO;
        parkour.0.reset();
        cover.cover = None;
    }
}

/// Keep the local copy of the arena's boxes in step with the host's arena (or the sewer's,
/// once we are down the hole).
fn track_arena(
    view: Res<View>,
    mut local: ResMut<LocalArena>,
    mut parkour: ResMut<LocalParkour>,
    mut cover: ResMut<LocalCover>,
    body: Single<&Transform, With<LocalBody>>,
) {
    let arena = view.snap.as_ref().map_or(ArenaId::Classic, |s| s.arena);
    local.hole = view.snap.as_ref().is_some_and(|s| matches!(s.phase, crate::sim::Phase::Race { .. }));
    let below = sewer::is_below(body.translation - Vec3::Y * FEET_OFFSET);
    if local.arena != Some(arena) || local.below != below {
        local.arena = Some(arena);
        local.below = below;
        (local.solids, local.rails) = if below {
            (sewer::solids(), sewer::rails())
        } else {
            (layout::solids(arena), layout::rails(arena))
        };
        // Wall indices refer to the old list.
        parkour.0.reset();
        cover.cover = None;
    }
}

/// Do we own the golden string ball, out of our hands right now?
pub fn can_pull(view: &View) -> bool {
    let (Some(s), Some(me)) = (view.snap.as_ref(), view.local) else { return false };
    let team = s.players.iter().find(|p| p.id == me).map(|p| p.team);
    let mine = |by: PlayerId| s.players.iter().any(|p| p.id == by && Some(p.team) == team);
    s.golden == Some(me)
        && s.balls.iter().any(|b| {
            b.kind == crate::sim::ball::BallKind::Golden
                && !matches!(b.state, crate::sim::BallState::Held { by } if mine(by))
                && !matches!(b.state, crate::sim::BallState::Returning { .. })
        })
}

/// Tab / middle mouse toggles lock-on. Drops the lock when the target is gone or dead.
#[allow(clippy::too_many_arguments)]
fn lock_input(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    locked: Res<CursorLocked>,
    auto: Res<Autopilot>,
    view: Res<View>,
    charge: Res<Charge>,
    mut aim: ResMut<MouseAim>,
    mut lock: ResMut<LockOn>,
    mut actions: ResMut<LocalActions>,
    cam: Single<&Transform, With<FollowCamera>>,
) {
    aim.moved = locked.0 && !auto.0 && motion.delta.length() > MOUSE_AIM_PIXELS;
    aim.since_move = if aim.moved { 0.0 } else { aim.since_move + time.delta_secs() };
    let (Some(snap), Some(me)) = (view.snap.as_ref(), view.me()) else { return };
    let enemies: Vec<(PlayerId, Vec3)> = snap
        .players
        .iter()
        .filter(|p| p.team != me.team && p.alive())
        .map(|p| (p.id, Vec3::from_array(p.pos) + Vec3::Y * 1.0))
        .collect();
    if let Some(t) = lock.0 {
        if !me.alive() || !enemies.iter().any(|(id, _)| *id == t) {
            lock.0 = None;
            actions.0.push(Action::Lock { target: None });
        }
    }
    if auto.0 || !locked.0 || !me.alive() || charge.started.is_none() {
        return;
    }
    // Swinging the mouse toward another enemy while charging moves the lock to them.
    if aim.moved {
        if let Some(t) = mouse_retarget(&enemies, lock.0, cam.translation, *cam.forward()) {
            lock.0 = Some(t);
            actions.0.push(Action::Lock { target: Some(t) });
            return;
        }
    }
    // Tab / middle mouse while charging cycles to the next enemy.
    let pressed = keys.just_pressed(KeyCode::Tab) || mouse.just_pressed(MouseButton::Middle);
    if !pressed {
        return;
    }
    let target = next_target(&enemies, lock.0, cam.translation, *cam.forward());
    if target != lock.0 {
        lock.0 = target;
        actions.0.push(Action::Lock { target });
    }
}

/// While locked on, the camera turns toward the target, except right after the mouse moved
/// (so you can swing over to another enemy).
fn lock_camera(
    time: Res<Time>,
    view: Res<View>,
    lock: Res<LockOn>,
    aim: Res<MouseAim>,
    body: Single<&Transform, With<LocalBody>>,
    mut cam: Single<&mut FollowCamera>,
) {
    if aim.since_move < LOCK_MOUSE_GRACE {
        return;
    }
    let (Some(snap), Some(t)) = (view.snap.as_ref(), lock.0) else { return };
    let Some(target) = snap.players.iter().find(|p| p.id == t) else { return };
    let chest = Vec3::from_array(target.pos) + Vec3::Y * 1.0;
    let eye = body.translation + Vec3::Y * 0.7;
    let to = chest - eye;
    let flat = Vec3::new(to.x, 0.0, to.z);
    if flat.length() < 0.5 {
        return;
    }
    let k = 1.0 - (-LOCK_CAMERA_RATE * time.delta_secs()).exp();
    cam.yaw = lerp_angle(cam.yaw, yaw_toward(flat), k);
    let pitch = to.y.atan2(flat.length()).clamp(-1.0, 0.6);
    cam.pitch += (pitch - cam.pitch) * k;
}

fn keyboard_intent(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    auto: Res<Autopilot>,
    cam: Single<&FollowCamera>,
    mut intent: ResMut<MoveIntent>,
) {
    if auto.0 {
        return;
    }
    intent.dir = input_direction(&keys, cam.yaw);
    intent.jump = keys.just_pressed(KeyCode::Space);
    intent.sprint = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    intent.dash = keys.just_pressed(KeyCode::ControlLeft) || keys.just_pressed(KeyCode::ControlRight);
    intent.face_camera = mouse.pressed(MouseButton::Left) || mouse.pressed(MouseButton::Right);
    // While charging a throw, Q/E curve the ball instead of doing tricks, and F lobs.
    let charging = mouse.pressed(MouseButton::Left);
    intent.aiming = charging;
    intent.cover = keys.just_pressed(KeyCode::KeyF) && !charging;
    // Mount keys and riding tricks (Q/E/F belong to the throw while charging).
    let digits = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6];
    intent.mount = digits.iter().position(|k| keys.just_pressed(*k)).map(|i| MountKind::ALL[i]);
    intent.dismount = keys.just_pressed(KeyCode::KeyX);
    intent.trick_keys = TrickInput {
        dir: Dir::from_keys(
            keys.pressed(KeyCode::KeyW),
            keys.pressed(KeyCode::KeyS),
            keys.pressed(KeyCode::KeyA),
            keys.pressed(KeyCode::KeyD),
        ),
        shift: intent.sprint,
    };
    intent.trick_flip = keys.just_pressed(KeyCode::KeyQ) && !charging;
    intent.trick_grab_start = keys.just_pressed(KeyCode::KeyE) && !charging;
    intent.trick_grab = keys.pressed(KeyCode::KeyE) && !charging;
    intent.trick_grind = keys.pressed(KeyCode::KeyF) && !charging;
    intent.trick_air_flip = keys.just_pressed(KeyCode::KeyR);
    intent.trick = match (keys.just_pressed(KeyCode::KeyQ), keys.just_pressed(KeyCode::KeyE)) {
        (true, false) if !charging => Some(Side::Left),
        (false, true) if !charging => Some(Side::Right),
        _ => None,
    };
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn move_local_body(
    time: Res<Time>,
    intent: Res<MoveIntent>,
    view: Res<View>,
    mut trick: ResMut<LocalTrick>,
    mut dash: ResMut<LocalDash>,
    mut facing: ResMut<LocalFacing>,
    mut parkour: ResMut<LocalParkour>,
    mut cover: ResMut<LocalCover>,
    mut shake: ResMut<CameraShake>,
    arena: Res<LocalArena>,
    mut ride: ResMut<LocalRide>,
    lib: Res<MountLibrary>,
    mut actions: ResMut<LocalActions>,
    cam: Single<&FollowCamera>,
    body: Single<(
        &mut LocalBody,
        &mut Transform,
        &mut KinematicCharacterController,
        Option<&KinematicCharacterControllerOutput>,
    )>,
) {
    let dt = time.delta_secs().min(0.05);
    let (mut b, mut tf, mut controller, output) = body.into_inner();
    let me = view.me();
    tf.rotation = Quat::from_rotation_y(facing.body_yaw(cam.yaw));
    dash.tick(dt);
    let solids = &arena.solids;
    cover.toggled = false;
    cover.crouched = false;
    cover.pose = crate::sim::CoverPose::None;
    // Catching a ball shoves us back along its flight.
    for e in &view.events {
        if let GameEvent::Caught { catcher, push, perfect, .. } = e {
            if Some(*catcher) == view.local {
                b.recoil = Vec3::from_array(*push);
                shake.add(if *perfect { 0.55 } else { 0.4 });
            }
        }
    }
    if !me.is_none_or(|m| m.alive()) {
        controller.translation = None;
        trick.0 = None;
        dash.active = 0.0;
        parkour.0.reset();
        cover.cover = None;
        ride.ride = None;
        return;
    }
    // Slimed, knocked flat or launched: the host owns our position.
    if let Some(m) = me.filter(|m| m.slimed > 0.0 || m.stunned > 0.0 || m.knocked) {
        trick.0 = None;
        dash.active = 0.0;
        if !parkour.0.is_free() {
            info!("parkour: knocked off the wall");
        }
        parkour.0.reset();
        cover.cover = None;
        if ride.ride.take().is_some() {
            info!("mount: knocked off");
        }
        b.push = Vec3::ZERO;
        b.recoil = Vec3::ZERO;
        let target = Vec3::from_array(m.pos) + Vec3::Y * FEET_OFFSET;
        tf.translation = tf.translation.lerp(target, 1.0 - (-20.0 * dt).exp());
        controller.translation = None;
        b.vel_y = 0.0;
        return;
    }
    // Only count as grounded when not rising, so the frame after a jump isn't "ground".
    let grounded = output.is_some_and(|o| o.grounded) && b.vel_y <= 0.0;
    if grounded {
        b.vel_y = 0.0;
        b.air_jumps = AIR_JUMPS;
        b.sprint_jump = false;
        b.air_time = 0.0;
    } else {
        b.air_time += dt;
    }
    b.jump_buffer = if intent.jump { JUMP_BUFFER_SECS } else { (b.jump_buffer - dt).max(0.0) };
    let recoil = b.recoil;
    b.recoil *= (-RECOIL_DECAY * dt).exp();
    if b.recoil.length() < 0.2 {
        b.recoil = Vec3::ZERO;
    }
    let feet = tf.translation - Vec3::Y * FEET_OFFSET;

    // 1-6: hop on that mount (the same key again, or X, hops off).
    let now = time.elapsed_secs();
    // Falling down the hole into the sewer: back on our last mount before we land.
    let mut mount = intent.mount;
    if arena.hole && ride.ride.is_none() && feet.y < sewer::BELOW && !grounded && mount.is_none() {
        let kind = ride.last_mount.unwrap_or(MountKind::Skateboard);
        info!("sewer: dropping in, mounting the {}", lib.get(kind).name);
        mount = Some(kind);
    }
    if let Some(kind) = mount {
        ride.last_mount = Some(kind);
        if ride.ride.as_ref().is_some_and(|r| r.mount == kind) {
            ride.ride = None;
            info!("mount: hopped off");
        } else {
            let heading = facing.body_yaw(cam.yaw);
            let speed = ride.ride.as_ref().map_or(0.0, |r| r.speed);
            ride.ride = Some(Ride::new(kind, heading, speed));
            cover.cover = None;
            parkour.0.reset();
            trick.0 = None;
            let name = lib.get(kind).name.clone();
            info!("mount: riding the {name}");
            ride.feed.push((now, format!("Riding the {name}")));
        }
    }
    // X: tug the golden ball's string if it's ours and out of our hands, else hop off.
    if intent.dismount {
        if can_pull(&view) {
            info!("golden: pulled the string");
            actions.0.push(Action::Pull);
        } else if ride.ride.take().is_some() {
            info!("mount: hopped off");
        }
    }
    if ride.ride.is_some() {
        ride_frame(&intent, &lib, &arena, output, feet, grounded, dt, now, recoil, &mut ride, &mut dash, &mut b, &mut actions);
        if let Some(r) = ride.ride.as_ref() {
            // The body faces the way it rolls.
            let want = r.heading - cam.yaw;
            facing.0 = lerp_angle(facing.0, want, 1.0 - (-20.0 * dt).exp());
            tf.rotation = Quat::from_rotation_y(facing.body_yaw(cam.yaw));
        }
        match b.ride_place.take() {
            Some(p) => {
                tf.translation = p + Vec3::Y * FEET_OFFSET;
                controller.translation = None;
            }
            None => controller.translation = Some(b.ride_vel * dt),
        }
        return;
    }

    // F: take cover behind the wall we're next to, or leave it.
    if intent.cover {
        if cover.cover.take().is_some() {
            cover.toggled = true;
            info!("cover: left (F)");
        } else if grounded && parkour.0.is_free() {
            if let Some(c) = Cover::find(feet, solids) {
                info!("cover: taken (wall {} m high)", (c.top - feet.y).round());
                cover.cover = Some(c);
                cover.toggled = true;
                trick.0 = None;
            }
        }
    }
    if cover.cover.is_some() {
        let leave = if intent.sprint && intent.dir.length_squared() > 0.01 {
            Some("sprint")
        } else if intent.jump {
            Some("jump")
        } else if intent.dash {
            Some("dash")
        } else if !grounded {
            Some("fell")
        } else {
            None
        };
        if let Some(why) = leave {
            info!("cover: left ({why})");
            cover.cover = None;
        }
    }
    if let Some(c) = cover.cover.as_mut() {
        match c.slide(feet, intent.dir.normalize_or_zero(), dt, solids) {
            Some(p) => {
                let crouched = c.crouched(intent.aiming);
                let wall_yaw = yaw_toward(-c.normal) - cam.yaw;
                cover.crouched = crouched;
                cover.pose = if crouched { crate::sim::CoverPose::Crouched } else { crate::sim::CoverPose::Up };
                // Facing the wall (peeking over it), or the camera while aiming.
                let want = if intent.face_camera { 0.0 } else { wall_yaw };
                facing.0 = lerp_angle(facing.0, want, 1.0 - (-WALL_FACE_RATE * dt).exp());
                tf.rotation = Quat::from_rotation_y(facing.body_yaw(cam.yaw));
                b.vel_y = 0.0;
                b.push = Vec3::ZERO;
                controller.translation = Some(p - feet + recoil * dt * 0.3 + Vec3::NEG_Y * 0.05);
                return;
            }
            None => cover.cover = None,
        }
    }
    // Ctrl: a quick burst in the movement direction (or straight ahead), ground or air.
    // (While hanging from a ledge Ctrl lets go instead.)
    let hanging = matches!(parkour.0.state, parkour::State::Hang { .. } | parkour::State::Mantle { .. });
    if intent.dash && !hanging {
        let fwd = Vec3::new(-cam.yaw.sin(), 0.0, -cam.yaw.cos());
        let dir = if intent.dir.length_squared() > 0.01 { intent.dir } else { fwd };
        if dash.try_start(dir, intent.sprint) {
            trick.0 = None;
            parkour.0.reset();
            b.push = Vec3::ZERO;
        }
    }
    if dash.dashing() {
        b.vel_y = 0.0;
        b.was_dashing = true;
        controller.translation = Some((dash.velocity + recoil) * dt);
        return;
    }
    // An air dash flows on into the fall instead of stopping dead.
    if std::mem::take(&mut b.was_dashing) && !grounded {
        b.push = dash.velocity * DASH_CARRY;
    }
    let forward = Vec3::new(-cam.yaw.sin(), 0.0, -cam.yaw.cos());
    let fwd_amount = intent.dir.normalize_or_zero().dot(forward);

    // Parkour: wall run, climb, hang, vault / climb up.
    let mut motion = parkour::Motion { vel_y: b.vel_y, push: b.push, air_jumps: b.air_jumps };
    let input = parkour::Input {
        dir: intent.dir.normalize_or_zero(),
        forward,
        jump: intent.jump,
        drop: intent.dash,
        grounded,
        // Holding a ball while hanging: pull up and look over the ledge.
        peek: me.is_some_and(|m| m.holding),
    };
    let out = parkour.0.update(&mut motion, &input, feet, dt, solids);
    (b.vel_y, b.push, b.air_jumps) = (motion.vel_y, motion.push, motion.air_jumps);
    if let Some(e) = out.event {
        info!("parkour: {e}");
    }
    // Touching a wall gives back the double jump and both dashes, so moves chain.
    if out.refresh() {
        b.air_jumps = AIR_JUMPS;
        dash.refill();
    }
    if out.used_jump {
        b.jump_buffer = 0.0;
    }
    // On a wall the body turns to face along it / into it, unless aiming (throw while hanging).
    if let Some(face) = parkour.0.facing() {
        trick.0 = None;
        let want = if intent.face_camera { 0.0 } else { yaw_toward(face) - cam.yaw };
        facing.0 = lerp_angle(facing.0, want, 1.0 - (-WALL_FACE_RATE * dt).exp());
        tf.rotation = Quat::from_rotation_y(facing.body_yaw(cam.yaw));
    }
    match out.drive {
        Drive::Translate(d) => {
            controller.translation = Some(d);
            return;
        }
        Drive::Place(p) => {
            tf.translation = p + Vec3::Y * FEET_OFFSET;
            controller.translation = None;
            return;
        }
        Drive::Normal => {}
    }

    // A jump pressed just before landing fires on landing; just after walking off a ledge
    // it still counts as a ground jump.
    let wants_jump = (intent.jump || (grounded && b.jump_buffer > 0.0)) && !out.used_jump;
    let on_ground = grounded || (b.air_time < COYOTE_SECS && b.vel_y <= 0.0 && b.air_jumps == AIR_JUMPS);
    if wants_jump {
        if let Some((vy, left)) = try_jump(on_ground, b.air_jumps) {
            b.vel_y = vy;
            b.air_jumps = left;
            b.jump_buffer = 0.0;
            b.air_time = COYOTE_SECS;
            if on_ground {
                b.sprint_jump = intent.sprint;
            }
            if !on_ground {
                // The second jump flips: forward or backward with the movement.
                trick.0 = Some(TrickState { kind: double_jump_flip(fwd_amount), progress: 0.0 });
            }
        }
    }
    if let Some(side) = intent.trick {
        if !grounded && trick.0.is_none() {
            trick.0 = Some(TrickState { kind: air_trick(fwd_amount, side), progress: 0.0 });
        }
    }
    // Spins turn only the body; it turns back once you move, charge or catch.
    advance_trick(&mut trick.0, &mut facing.0, grounded, dt);
    if trick.0.is_none() && (intent.face_camera || (grounded && intent.dir.length_squared() > 0.01)) {
        facing.settle(dt);
    }

    b.vel_y -= GRAVITY * dt;
    let speed = MOVE_SPEED * move_multiplier(intent.sprint, !grounded && b.sprint_jump);
    // Wall-jump momentum carries you across, fading in the air; a catch shoves you back.
    let flat = intent.dir.normalize_or_zero() * speed + b.push + recoil;
    b.push = if grounded { Vec3::ZERO } else { b.push * (-parkour::PUSH_DECAY * dt).exp() };
    controller.translation = Some(Vec3::new(flat.x, b.vel_y, flat.z) * dt);
}

/// One frame on a mount: dashes still work; otherwise the ride logic moves us. Leaves the
/// result in `b.ride_vel` / `b.ride_place`.
#[allow(clippy::too_many_arguments)]
fn ride_frame(
    intent: &MoveIntent,
    lib: &MountLibrary,
    arena: &LocalArena,
    output: Option<&KinematicCharacterControllerOutput>,
    feet: Vec3,
    grounded: bool,
    dt: f32,
    now: f32,
    recoil: Vec3,
    ride: &mut LocalRide,
    dash: &mut LocalDash,
    b: &mut LocalBody,
    actions: &mut LocalActions,
) {
    let Some(r) = ride.ride.as_mut() else { return };
    b.ride_place = None;
    if intent.dash {
        let dir = if intent.dir.length_squared() > 0.01 { intent.dir } else { crate::ride::heading_dir(r.heading) };
        if dash.try_start(dir, intent.sprint) {
            r.trick = None;
            r.grind = None;
        }
    }
    if dash.dashing() {
        b.vel_y = 0.0;
        b.ride_vel = dash.velocity + recoil;
        return;
    }
    // Ran into something: lose the speed we couldn't make.
    if let Some(o) = output {
        let want = o.desired_translation.with_y(0.0).length();
        if want > 1e-3 && r.grind.is_none() {
            let eff = o.effective_translation.with_y(0.0);
            let got = eff.length();
            if got < want * 0.7 {
                r.blocked(r.speed * got / want);
                // Glance off along the wall instead of grinding to a halt against it.
                if got > want * 0.15 {
                    r.heading = yaw_toward(eff);
                }
            }
        }
    }
    let def = lib.get(r.mount);
    let input = RideInput {
        dir: intent.dir.normalize_or_zero(),
        keys: intent.trick_keys,
        sprint: intent.sprint,
        jump: intent.jump,
        flip: intent.trick_flip,
        grab_start: intent.trick_grab_start,
        grab: intent.trick_grab,
        grind: intent.trick_grind,
        air_flip: intent.trick_air_flip,
        grounded,
    };
    let out = r.update(def, &input, &mut b.vel_y, feet, dt, &arena.rails);
    b.ride_vel = out.vel + recoil;
    b.ride_place = out.place;
    for e in out.events {
        let msg = match e {
            RideEvent::Trick(n) => n,
            RideEvent::Grind(n) => format!("{n} (grind)"),
            RideEvent::Landed { names, points } => {
                if points >= STYLE_REPORT_POINTS {
                    actions.0.push(Action::Style { points });
                }
                format!("LANDED {} = {points}", names.join(" + "))
            }
            RideEvent::Bail(why) => format!("BAIL! {why}"),
        };
        info!("ride: {msg}");
        ride.feed.push((now, msg));
    }
    ride.feed.retain(|(t, _)| now - t < 4.0);
    let len = ride.feed.len();
    if len > 5 {
        ride.feed.drain(..len - 5);
    }
}

/// Speed multiplier: sprinting runs 1.4x; a jump taken while sprinting carries 1.24x farther.
pub fn move_multiplier(sprint: bool, sprint_jump: bool) -> f32 {
    match (sprint, sprint_jump) {
        (true, true) => SPRINT_MULT * SPRINT_JUMP_MULT,
        (true, false) => SPRINT_MULT,
        _ => 1.0,
    }
}

/// Move a trick along. Spins turn `yaw` (the body's offset from the camera) 180 degrees as
/// they go. Flips end early on landing.
pub fn advance_trick(trick: &mut Option<TrickState>, yaw: &mut f32, grounded: bool, dt: f32) {
    let Some(t) = trick.as_mut() else { return };
    let before = t.progress;
    t.progress = (t.progress + dt / TRICK_SECS).min(1.0);
    let turned = (t.progress - before) * std::f32::consts::PI;
    match t.kind {
        TrickKind::SpinLeft => *yaw += turned,
        TrickKind::SpinRight => *yaw -= turned,
        TrickKind::FrontFlip | TrickKind::BackFlip => {}
    }
    let flip = matches!(t.kind, TrickKind::FrontFlip | TrickKind::BackFlip);
    if t.progress >= 1.0 || (flip && grounded && t.progress > 0.3) {
        *trick = None;
    }
}

/// Throw, lob and catch. Only queues actions that could succeed right now.
#[allow(clippy::too_many_arguments)]
fn combat_input(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    locked: Res<CursorLocked>,
    auto: Res<Autopilot>,
    view: Res<View>,
    mut lock: ResMut<LockOn>,
    mut charge: ResMut<Charge>,
    mut actions: ResMut<LocalActions>,
    mut cover: ResMut<LocalCover>,
    mut shake: ResMut<CameraShake>,
    (arena, ride): (Res<LocalArena>, Res<LocalRide>),
    body: Single<&Transform, With<LocalBody>>,
    cam: Single<&Transform, (With<FollowCamera>, Without<LocalBody>)>,
    follow: Single<&FollowCamera>,
) {
    if auto.0 {
        return; // the autopilot drives catching and throwing
    }
    let now = time.elapsed_secs();
    let Some(me) = view.me() else { return };
    let helpless = me.stunned > 0.0 || me.slimed > 0.0 || me.knocked;
    if !locked.0 || !me.alive() || helpless {
        charge.started = None;
        release_lock(&mut lock, &mut actions);
        return;
    }

    // Right mouse opens a short catch window: press it just before the ball arrives.
    if mouse.just_pressed(MouseButton::Right) && !me.holding && me.catch_cooldown <= 0.0 {
        actions.0.push(Action::Catch);
    }
    if !me.holding {
        charge.started = None;
        release_lock(&mut lock, &mut actions);
        return;
    }

    // Aim at the locked target, otherwise where the crosshair points (40 m out).
    let feet = body.translation - Vec3::Y * FEET_OFFSET;
    let forward = Vec3::new(-follow.yaw.sin(), 0.0, -follow.yaw.cos());
    // Risen up out of cover the ball comes from over the wall.
    let hold_h = if cover.cover.is_some() { crate::sim::COVER_HOLD_HEIGHT } else { HOLD_HEIGHT };
    let hold = feet + Vec3::Y * hold_h + forward * 0.45;
    let locked_target = lock
        .0
        .and_then(|t| view.snap.as_ref()?.players.iter().find(|p| p.id == t))
        .map(|p| Vec3::from_array(p.pos) + Vec3::Y * 1.0);
    let target = locked_target.unwrap_or(cam.translation + cam.forward() * 40.0);
    let mut curve = match (keys.pressed(KeyCode::KeyQ), keys.pressed(KeyCode::KeyE)) {
        (true, false) => -1.0,
        (false, true) => 1.0,
        _ => 0.0,
    };
    let (aim_v, cover_curve) = cover_throw(cover.cover.as_ref(), &arena.solids, feet, hold, target);
    let aim = aim_v.normalize_or(forward).to_array();
    if curve == 0.0 {
        curve = cover_curve;
    }

    // F lobs (while charging, or when it wasn't used to take / leave cover).
    // (Riding, F is the grind button: it only lobs while charging.)
    if keys.just_pressed(KeyCode::KeyF) && (charge.started.is_some() || (!cover.toggled && ride.ride.is_none())) {
        actions.0.push(Action::Throw { aim, charge: 0.0, curve: 0.0, lob: true });
        charge.started = None;
        release_lock(&mut lock, &mut actions);
        return;
    }
    // Lock-on lasts only while charging: just holding the ball lets go of it.
    if charge.started.is_none() && !mouse.just_pressed(MouseButton::Left) {
        release_lock(&mut lock, &mut actions);
    }
    // Already charged (perfect catch / boomerang): the click throws at full power right away.
    if me.charged && mouse.just_pressed(MouseButton::Left) {
        lock_nearest(&view, me, feet, &mut lock, &mut actions);
        let aim = lock
            .0
            .and_then(|t| view.snap.as_ref()?.players.iter().find(|p| p.id == t))
            .map(|p| (Vec3::from_array(p.pos) + Vec3::Y * 1.0 - hold).normalize_or(forward).to_array())
            .unwrap_or(aim);
        actions.0.push(Action::Throw { aim, charge: 1.0, curve, lob: false });
        threw(&mut cover, &mut shake, 1.0);
        charge.started = None;
        release_lock(&mut lock, &mut actions);
        return;
    }
    if mouse.just_pressed(MouseButton::Left) {
        charge.started = Some(now);
        // Starting a charge locks on to the enemy under the crosshair (else the nearest).
        lock_for_charge(&view, me, cam.translation, *cam.forward(), feet, &mut lock, &mut actions);
    }
    if mouse.just_released(MouseButton::Left) {
        let amount = charge.amount(now).unwrap_or(0.0);
        actions.0.push(Action::Throw { aim, charge: amount, curve, lob: false });
        threw(&mut cover, &mut shake, amount);
        charge.started = None;
        release_lock(&mut lock, &mut actions);
    }
}

/// After a throw: stay up in cover a moment, and kick the camera with the throw's power.
fn threw(cover: &mut LocalCover, shake: &mut CameraShake, power: f32) {
    if let Some(c) = cover.cover.as_mut() {
        c.popped_up();
        // Report risen up this frame already, so the host throws from over the wall.
        cover.crouched = false;
        cover.pose = crate::sim::CoverPose::Up;
    }
    shake.add(0.12 + 0.2 * power);
}

/// Aim (unnormalised) and curve for a throw at `target`, adjusted when throwing from cover:
/// over a low wall the aim is tilted up just enough to clear it; from behind a tall wall the
/// ball curves around its nearer end (curved throws go round cover to their target).
pub fn cover_throw(cover: Option<&Cover>, solids: &[layout::Aabb], feet: Vec3, hold: Vec3, target: Vec3) -> (Vec3, f32) {
    let straight = target - hold;
    let Some(c) = cover else { return (straight, 0.0) };
    let Some(wall) = solids.get(c.wall) else { return (straight, 0.0) };
    if !cover::blocked(hold, target, std::slice::from_ref(wall)) {
        return (straight, 0.0);
    }
    if c.is_low(feet) {
        (cover::clear_aim(hold, target, wall), 0.0)
    } else {
        (straight, c.curve_around(feet, straight))
    }
}

/// Lock on to the nearest living enemy, if one is in range.
fn lock_nearest(view: &View, me: &crate::sim::PlayerView, feet: Vec3, lock: &mut LockOn, actions: &mut LocalActions) {
    let enemies = enemy_chests(view, me);
    if let Some(t) = nearest_enemy(&enemies, feet) {
        lock.0 = Some(t);
        actions.0.push(Action::Lock { target: Some(t) });
    }
}

/// Living enemies' chest positions.
fn enemy_chests(view: &View, me: &crate::sim::PlayerView) -> Vec<(PlayerId, Vec3)> {
    view.snap
        .iter()
        .flat_map(|s| s.players.iter())
        .filter(|p| p.team != me.team && p.alive())
        .map(|p| (p.id, Vec3::from_array(p.pos) + Vec3::Y * 1.0))
        .collect()
}

/// Starting a charge: lock the enemy under the crosshair, else the nearest one.
fn lock_for_charge(
    view: &View,
    me: &crate::sim::PlayerView,
    eye: Vec3,
    forward: Vec3,
    feet: Vec3,
    lock: &mut LockOn,
    actions: &mut LocalActions,
) {
    let enemies = enemy_chests(view, me);
    if let Some(t) = charge_target(&enemies, eye, forward, feet + Vec3::Y * 1.0) {
        lock.0 = Some(t);
        actions.0.push(Action::Lock { target: Some(t) });
    }
}

fn release_lock(lock: &mut LockOn, actions: &mut LocalActions) {
    if lock.0.take().is_some() {
        actions.0.push(Action::Lock { target: None });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_follows_camera_yaw() {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyW);
        // Camera yaw 0 looks down -Z.
        assert!(input_direction(&keys, 0.0).abs_diff_eq(Vec3::NEG_Z, 1e-5));
        // Camera turned 90 degrees left looks down -X.
        let d = input_direction(&keys, std::f32::consts::FRAC_PI_2);
        assert!(d.abs_diff_eq(Vec3::NEG_X, 1e-5));
    }

    #[test]
    fn diagonal_is_not_faster() {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyW);
        keys.press(KeyCode::KeyD);
        assert!((input_direction(&keys, 0.0).length() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn charge_fills_over_one_second() {
        let c = Charge { started: Some(2.0) };
        assert_eq!(c.amount(2.0), Some(0.0));
        assert!((c.amount(2.5).unwrap() - 0.5).abs() < 1e-6);
        assert_eq!(c.amount(9.0), Some(1.0));
        assert_eq!(Charge::default().amount(1.0), None);
    }

    #[test]
    fn double_jump_once_in_air() {
        let (vy, left) = try_jump(true, 0).unwrap();
        assert_eq!((vy, left), (JUMP_SPEED, AIR_JUMPS));
        let (vy2, left2) = try_jump(false, left).unwrap();
        assert_eq!((vy2, left2), (DOUBLE_JUMP_SPEED, 0));
        assert!(try_jump(false, left2).is_none(), "no triple jump");
    }

    #[test]
    fn lock_picks_enemy_nearest_crosshair() {
        let from = Vec3::ZERO;
        let fwd = Vec3::NEG_Z;
        let enemies = [(1, Vec3::new(3.0, 0.0, -10.0)), (2, Vec3::new(0.5, 0.0, -20.0)), (3, Vec3::new(0.0, 0.0, 10.0))];
        assert_eq!(pick_lock_target(&enemies, from, fwd), Some(2));
        // Behind us or too far: nothing.
        assert_eq!(pick_lock_target(&[(3, Vec3::new(0.0, 0.0, 10.0))], from, fwd), None);
        assert_eq!(pick_lock_target(&[(4, Vec3::new(0.0, 0.0, -LOCK_RANGE - 1.0))], from, fwd), None);
    }

    #[test]
    fn double_jump_flips_with_movement() {
        assert_eq!(double_jump_flip(1.0), TrickKind::FrontFlip);
        assert_eq!(double_jump_flip(-1.0), TrickKind::BackFlip);
        assert_eq!(double_jump_flip(0.0), TrickKind::FrontFlip, "standing still flips forward");
    }

    #[test]
    fn q_and_e_pick_air_tricks() {
        assert_eq!(air_trick(0.0, Side::Left), TrickKind::SpinLeft);
        assert_eq!(air_trick(0.0, Side::Right), TrickKind::SpinRight);
        assert_eq!(air_trick(1.0, Side::Left), TrickKind::FrontFlip);
        assert_eq!(air_trick(-1.0, Side::Right), TrickKind::BackFlip);
    }

    #[test]
    fn spins_turn_exactly_half_a_circle() {
        let mut t = Some(TrickState { kind: TrickKind::SpinLeft, progress: 0.0 });
        let mut yaw = 0.0;
        for _ in 0..100 {
            advance_trick(&mut t, &mut yaw, true, 1.0 / 60.0);
        }
        assert!(t.is_none());
        assert!((yaw - std::f32::consts::PI).abs() < 1e-4, "{yaw}");
        let mut t = Some(TrickState { kind: TrickKind::SpinRight, progress: 0.0 });
        let mut yaw = 0.0;
        for _ in 0..100 {
            advance_trick(&mut t, &mut yaw, false, 1.0 / 60.0);
        }
        assert!((yaw + std::f32::consts::PI).abs() < 1e-4);
    }

    #[test]
    fn spun_body_settles_back_the_short_way() {
        let mut f = LocalFacing(std::f32::consts::PI * 3.0 - 0.2); // spun one and a half times
        for _ in 0..120 {
            f.settle(1.0 / 60.0);
        }
        assert_eq!(f.0, 0.0);
        assert!((LocalFacing(1.0).body_yaw(0.5) - 1.5).abs() < 1e-6);
    }

    #[test]
    fn flip_ends_on_landing() {
        let mut t = Some(TrickState { kind: TrickKind::FrontFlip, progress: 0.5 });
        let mut yaw = 0.0;
        advance_trick(&mut t, &mut yaw, true, 0.01);
        assert!(t.is_none());
        assert_eq!(yaw, 0.0, "flips don't turn you");
    }

    #[test]
    fn charge_locks_nearest_enemy_in_range() {
        let enemies = [(1, Vec3::new(0.0, 0.0, 30.0)), (2, Vec3::new(5.0, 0.0, 0.0))];
        assert_eq!(nearest_enemy(&enemies, Vec3::ZERO), Some(2), "nearest, even behind us");
        assert_eq!(nearest_enemy(&[(3, Vec3::new(0.0, 0.0, LOCK_RANGE + 5.0))], Vec3::ZERO), None);
    }

    #[test]
    fn two_dashes_then_cooldown() {
        let mut d = LocalDash::default();
        assert!(d.try_start(Vec3::X, false));
        assert!(!d.try_start(Vec3::X, false), "still mid-dash");
        d.tick(DASH_SECS);
        assert!(!d.try_start(Vec3::X, false), "0.2 s gap between dashes");
        d.tick(DASH_GAP);
        assert!(d.try_start(Vec3::X, false), "second dash");
        d.tick(DASH_SECS + DASH_GAP);
        assert!(!d.try_start(Vec3::X, false), "both used");
        d.tick(DASH_RECHARGE);
        assert_eq!(d.charges, DASH_CHARGES, "both back 1.5 s after the last dash");
        assert!(d.try_start(Vec3::X, false));
    }

    #[test]
    fn dash_covers_its_distance_and_sprint_adds_24_percent() {
        let mut d = LocalDash::default();
        d.try_start(Vec3::new(0.0, 0.0, -2.0), false);
        assert!((d.velocity.length() * DASH_SECS - DASH_DIST).abs() < 1e-3);
        let mut s = LocalDash::default();
        s.try_start(Vec3::NEG_Z, true);
        assert!((s.velocity.length() / d.velocity.length() - SPRINT_JUMP_MULT).abs() < 1e-4);
        assert_eq!(d.report(), Some(1));
        d.tick(1.0);
        assert_eq!(d.report(), None);
    }

    #[test]
    fn sprint_speeds() {
        assert_eq!(move_multiplier(false, false), 1.0);
        assert!((move_multiplier(true, false) - 1.4).abs() < 1e-6);
        assert!((move_multiplier(true, true) - 1.4 * 1.24).abs() < 1e-6);
        assert_eq!(move_multiplier(false, true), 1.0, "letting go of shift ends the boost");
    }

    #[test]
    fn tab_cycles_targets() {
        let e = [(1, Vec3::new(0.0, 0.0, -10.0)), (2, Vec3::new(5.0, 0.0, -10.0)), (3, Vec3::new(-9.0, 0.0, -3.0))];
        let (from, fwd) = (Vec3::ZERO, Vec3::NEG_Z);
        assert_eq!(next_target(&e, None, from, fwd), Some(1));
        assert_eq!(next_target(&e, Some(1), from, fwd), Some(2));
        assert_eq!(next_target(&e, Some(3), from, fwd), Some(1), "wraps around");
    }

    #[test]
    fn swinging_the_mouse_moves_the_lock() {
        let e = [(1, Vec3::new(0.0, 0.0, -10.0)), (2, Vec3::new(6.0, 0.0, -10.0))];
        let from = Vec3::ZERO;
        // Still looking at 1: keep it.
        assert_eq!(mouse_retarget(&e, Some(1), from, Vec3::NEG_Z), None);
        // Looking at 2 now: switch.
        let at2 = Vec3::new(6.0, 0.0, -10.0).normalize();
        assert_eq!(mouse_retarget(&e, Some(1), from, at2), Some(2));
        // Halfway between: not clearly closer, keep the lock (no flicker).
        let mid = Vec3::new(3.0, 0.0, -10.0).normalize();
        assert_eq!(mouse_retarget(&e, Some(1), from, mid), None);
        // Way off to the side of everyone: nothing in the cone, keep it.
        assert_eq!(mouse_retarget(&e, Some(1), from, Vec3::X), None);
    }

    #[test]
    fn charge_locks_the_crosshair_target_first() {
        let e = [(1, Vec3::new(0.0, 0.0, -30.0)), (2, Vec3::new(5.0, 0.0, 0.0))];
        assert_eq!(charge_target(&e, Vec3::ZERO, Vec3::NEG_Z, Vec3::ZERO), Some(1), "under the crosshair");
        assert_eq!(charge_target(&e, Vec3::ZERO, Vec3::Z, Vec3::ZERO), Some(2), "else the nearest");
    }

    #[test]
    fn wall_moves_give_both_dashes_back() {
        let mut d = LocalDash::default();
        d.try_start(Vec3::X, false);
        d.tick(DASH_SECS + DASH_GAP);
        d.try_start(Vec3::X, false);
        d.tick(DASH_SECS);
        assert_eq!(d.charges, 0);
        d.refill();
        assert_eq!(d.charges, DASH_CHARGES);
        assert!(d.try_start(Vec3::X, false), "ready at once");
    }

    #[test]
    fn cover_throws_go_over_low_walls_and_round_tall_ones() {
        let solids = layout::solids(ArenaId::Classic);
        // Behind Red's low wall (z 6.6..7.4, 1.2 m), throwing at someone across it.
        let feet = Vec3::new(0.0, 0.0, 7.95);
        let c = Cover::find(feet, &solids).expect("cover");
        let hold = feet + Vec3::Y * crate::sim::COVER_HOLD_HEIGHT + Vec3::NEG_Z * 0.45;
        let target = Vec3::new(0.0, 0.3, 5.0);
        let (aim, curve) = cover_throw(Some(&c), &solids, feet, hold, target);
        assert_eq!(curve, 0.0);
        assert!(aim.normalize().y > (target - hold).normalize().y, "tilted up over the wall");
        // Out of cover: untouched.
        assert_eq!(cover_throw(None, &solids, feet, hold, target), (target - hold, 0.0));
        // Behind the 3 m pillar at (9, 0): curves around it.
        let feet = Vec3::new(9.0, 0.0, 1.0 + BODY_RADIUS + 0.06);
        let c = Cover::find(feet, &solids).expect("pillar cover");
        assert!(!c.is_low(feet));
        let hold = feet + Vec3::Y * HOLD_HEIGHT;
        let (_, curve) = cover_throw(Some(&c), &solids, feet, hold, Vec3::new(9.0, 1.0, -10.0));
        assert!(curve != 0.0);
    }

    #[test]
    fn angle_blend_takes_short_way() {
        let a = lerp_angle(3.0, -3.0, 0.5);
        // 3.0 and -3.0 are 0.28 rad apart across +/-PI; halfway is near PI, not 0.
        assert!((a.abs() - std::f32::consts::PI).abs() < 0.05, "{a}");
    }
}
