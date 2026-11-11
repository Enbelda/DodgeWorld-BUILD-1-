//! The local player's body (Rapier character controller) and combat input.
//!
//! Controls: WASD move, Shift sprint, Ctrl dash (tackles), Space jump (again in the air to
//! double jump with a flip), hold Left Mouse to charge (locks the nearest enemy while charging)
//! and release to throw, hold Q / E while throwing to curve, F to lob, Right Mouse to catch
//! (time it), Tab / Middle Mouse while charging to switch target.
//! In the air: Q / E spin 180 left / right; W+Q/E front flip, S+Q/E back flip.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::camera::{CursorLocked, FollowCamera};
use crate::session::{LocalActions, LocalTrick, View};
use crate::sim::bot::yaw_toward;
use crate::sim::{
    layout, Action, PlayerId, TrickKind, TrickState, BODY_HEIGHT, BODY_RADIUS, HOLD_HEIGHT, LOCK_RANGE,
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
const LOCK_CAMERA_RATE: f32 = 8.0;
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
}

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
}

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
            .add_systems(Startup, spawn_local_body)
            .add_systems(
                Update,
                (
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
        Transform::from_translation(layout::spawn_point(true, 0) + Vec3::Y * FEET_OFFSET),
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
fn obey_teleports(view: Res<View>, body: Single<(&mut LocalBody, &mut Transform)>) {
    let Some(me) = view.me() else { return };
    let (mut b, mut tf) = body.into_inner();
    if b.teleports != Some(me.teleports) {
        b.teleports = Some(me.teleports);
        tf.translation = Vec3::from_array(me.pos) + Vec3::Y * FEET_OFFSET;
        b.vel_y = 0.0;
    }
}

/// Tab / middle mouse toggles lock-on. Drops the lock when the target is gone or dead.
#[allow(clippy::too_many_arguments)]
fn lock_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    locked: Res<CursorLocked>,
    auto: Res<Autopilot>,
    view: Res<View>,
    charge: Res<Charge>,
    mut lock: ResMut<LockOn>,
    mut actions: ResMut<LocalActions>,
    cam: Single<&Transform, With<FollowCamera>>,
) {
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
    // Tab / middle mouse while charging switches to another enemy.
    let pressed = keys.just_pressed(KeyCode::Tab) || mouse.just_pressed(MouseButton::Middle);
    if auto.0 || !locked.0 || !pressed || !me.alive() || charge.started.is_none() {
        return;
    }
    let target = next_target(&enemies, lock.0, cam.translation, *cam.forward());
    if target != lock.0 {
        lock.0 = target;
        actions.0.push(Action::Lock { target });
    }
}

/// While locked on, the camera turns toward the target (the mouse still nudges it).
fn lock_camera(
    time: Res<Time>,
    view: Res<View>,
    lock: Res<LockOn>,
    body: Single<&Transform, With<LocalBody>>,
    mut cam: Single<&mut FollowCamera>,
) {
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
    // While charging a throw, Q/E curve the ball instead of doing tricks.
    let charging = mouse.pressed(MouseButton::Left);
    intent.trick = match (keys.just_pressed(KeyCode::KeyQ), keys.just_pressed(KeyCode::KeyE)) {
        (true, false) if !charging => Some(Side::Left),
        (false, true) if !charging => Some(Side::Right),
        _ => None,
    };
}

#[allow(clippy::type_complexity)]
pub fn move_local_body(
    time: Res<Time>,
    intent: Res<MoveIntent>,
    view: Res<View>,
    mut trick: ResMut<LocalTrick>,
    mut dash: ResMut<LocalDash>,
    mut cam: Single<&mut FollowCamera>,
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
    tf.rotation = Quat::from_rotation_y(cam.yaw);
    dash.tick(dt);
    if !me.is_none_or(|m| m.alive()) {
        controller.translation = None;
        trick.0 = None;
        dash.active = 0.0;
        return;
    }
    // Slimed, knocked flat or launched: the host owns our position.
    if let Some(m) = me.filter(|m| m.slimed > 0.0 || m.stunned > 0.0 || m.knocked) {
        trick.0 = None;
        dash.active = 0.0;
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
    }
    // Ctrl: a quick burst in the movement direction (or straight ahead), ground or air.
    if intent.dash {
        let fwd = Vec3::new(-cam.yaw.sin(), 0.0, -cam.yaw.cos());
        let dir = if intent.dir.length_squared() > 0.01 { intent.dir } else { fwd };
        if dash.try_start(dir, intent.sprint) {
            trick.0 = None;
        }
    }
    if dash.dashing() {
        b.vel_y = 0.0;
        controller.translation = Some(dash.velocity * dt);
        return;
    }
    let forward = Vec3::new(-cam.yaw.sin(), 0.0, -cam.yaw.cos());
    let fwd_amount = intent.dir.normalize_or_zero().dot(forward);
    if intent.jump {
        if let Some((vy, left)) = try_jump(grounded, b.air_jumps) {
            b.vel_y = vy;
            b.air_jumps = left;
            if grounded {
                b.sprint_jump = intent.sprint;
            }
            if !grounded {
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
    advance_trick(&mut trick.0, &mut cam.yaw, grounded, dt);

    b.vel_y -= GRAVITY * dt;
    let speed = MOVE_SPEED * move_multiplier(intent.sprint, !grounded && b.sprint_jump);
    let flat = intent.dir.normalize_or_zero() * speed;
    controller.translation = Some(Vec3::new(flat.x, b.vel_y, flat.z) * dt);
}

/// Speed multiplier: sprinting runs 1.4x; a jump taken while sprinting carries 1.24x farther.
pub fn move_multiplier(sprint: bool, sprint_jump: bool) -> f32 {
    match (sprint, sprint_jump) {
        (true, true) => SPRINT_MULT * SPRINT_JUMP_MULT,
        (true, false) => SPRINT_MULT,
        _ => 1.0,
    }
}

/// Move a trick along. Spins turn the player (and camera) 180 degrees as they go.
/// Flips end early on landing.
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
    let hold = feet + Vec3::Y * HOLD_HEIGHT + forward * 0.45;
    let locked_target = lock
        .0
        .and_then(|t| view.snap.as_ref()?.players.iter().find(|p| p.id == t))
        .map(|p| Vec3::from_array(p.pos) + Vec3::Y * 1.0);
    let target = locked_target.unwrap_or(cam.translation + cam.forward() * 40.0);
    let aim = (target - hold).normalize_or(forward).to_array();
    let curve = match (keys.pressed(KeyCode::KeyQ), keys.pressed(KeyCode::KeyE)) {
        (true, false) => -1.0,
        (false, true) => 1.0,
        _ => 0.0,
    };

    if keys.just_pressed(KeyCode::KeyF) {
        actions.0.push(Action::Throw { aim, charge: 0.0, curve: 0.0, lob: true });
        charge.started = None;
        release_lock(&mut lock, &mut actions);
        return;
    }
    // Lock-on lasts only while charging: just holding the ball lets go of it.
    if charge.started.is_none() && !mouse.just_pressed(MouseButton::Left) {
        release_lock(&mut lock, &mut actions);
    }
    if mouse.just_pressed(MouseButton::Left) {
        charge.started = Some(now);
        // Starting a charge locks on to the nearest enemy.
        {
            let enemies: Vec<(PlayerId, Vec3)> = view
                .snap
                .iter()
                .flat_map(|s| s.players.iter())
                .filter(|p| p.team != me.team && p.alive())
                .map(|p| (p.id, Vec3::from_array(p.pos)))
                .collect();
            if let Some(t) = nearest_enemy(&enemies, feet) {
                lock.0 = Some(t);
                actions.0.push(Action::Lock { target: Some(t) });
            }
        }
    }
    if mouse.just_released(MouseButton::Left) {
        let amount = charge.amount(now).unwrap_or(0.0);
        actions.0.push(Action::Throw { aim, charge: amount, curve, lob: false });
        charge.started = None;
        release_lock(&mut lock, &mut actions);
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
    fn angle_blend_takes_short_way() {
        let a = lerp_angle(3.0, -3.0, 0.5);
        // 3.0 and -3.0 are 0.28 rad apart across +/-PI; halfway is near PI, not 0.
        assert!((a.abs() - std::f32::consts::PI).abs() < 0.05, "{a}");
    }
}
