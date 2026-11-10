//! The local player's body (Rapier character controller) and combat input.
//!
//! Controls: WASD move, Space jump (again in the air to double jump), hold Left Mouse to
//! charge and release to throw, hold Q / E while throwing to curve, F to lob,
//! hold Right Mouse to catch, Tab or Middle Mouse to lock on.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::camera::{CursorLocked, FollowCamera};
use crate::session::{CatchHold, LocalActions, View};
use crate::sim::bot::yaw_toward;
use crate::sim::{layout, Action, PlayerId, BODY_HEIGHT, BODY_RADIUS, HOLD_HEIGHT, LOCK_RANGE};

const MOVE_SPEED: f32 = 7.0;
const JUMP_SPEED: f32 = 8.0;
const DOUBLE_JUMP_SPEED: f32 = 7.5;
/// Extra jumps allowed in the air.
pub const AIR_JUMPS: u8 = 1;
const GRAVITY: f32 = 22.0;
/// Movement speed multiplier while holding the catch button.
pub const CATCH_MOVE_FACTOR: f32 = 0.6;
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

fn spawn_local_body(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let half_segment = (BODY_HEIGHT - 2.0 * BODY_RADIUS) / 2.0;
    commands.spawn((
        LocalBody::default(),
        Mesh3d(meshes.add(Capsule3d::new(BODY_RADIUS, half_segment * 2.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.8, 0.8, 0.8))),
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
    let pressed = keys.just_pressed(KeyCode::Tab) || mouse.just_pressed(MouseButton::Middle);
    if auto.0 || !locked.0 || !pressed || !me.alive() {
        return;
    }
    let target = match lock.0 {
        Some(_) => None,
        None => pick_lock_target(&enemies, cam.translation, *cam.forward()),
    };
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
    auto: Res<Autopilot>,
    cam: Single<&FollowCamera>,
    mut intent: ResMut<MoveIntent>,
) {
    if auto.0 {
        return;
    }
    intent.dir = input_direction(&keys, cam.yaw);
    intent.jump = keys.just_pressed(KeyCode::Space);
}

#[allow(clippy::type_complexity)]
pub fn move_local_body(
    time: Res<Time>,
    intent: Res<MoveIntent>,
    view: Res<View>,
    catch_hold: Res<CatchHold>,
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
    tf.rotation = Quat::from_rotation_y(cam.yaw);
    if !me.is_none_or(|m| m.alive()) {
        controller.translation = None;
        return;
    }
    // Slimed: the host drags us to the wall and holds us there.
    if let Some(m) = me.filter(|m| m.slimed > 0.0) {
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
    }
    if intent.jump {
        if let Some((vy, left)) = try_jump(grounded, b.air_jumps) {
            b.vel_y = vy;
            b.air_jumps = left;
        }
    }
    b.vel_y -= GRAVITY * dt;
    let speed = if catch_hold.0 { MOVE_SPEED * CATCH_MOVE_FACTOR } else { MOVE_SPEED };
    let flat = intent.dir.normalize_or_zero() * speed;
    controller.translation = Some(Vec3::new(flat.x, b.vel_y, flat.z) * dt);
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
    lock: Res<LockOn>,
    mut charge: ResMut<Charge>,
    mut actions: ResMut<LocalActions>,
    mut catch_hold: ResMut<CatchHold>,
    body: Single<&Transform, With<LocalBody>>,
    cam: Single<&Transform, (With<FollowCamera>, Without<LocalBody>)>,
    follow: Single<&FollowCamera>,
) {
    if auto.0 {
        return; // the autopilot drives catching and throwing
    }
    let now = time.elapsed_secs();
    let Some(me) = view.me() else { return };
    if !locked.0 || !me.alive() {
        charge.started = None;
        catch_hold.0 = false;
        return;
    }

    // Hold right mouse: the catch fires the moment a ball arrives.
    catch_hold.0 = mouse.pressed(MouseButton::Right) && !me.holding;
    if !me.holding {
        charge.started = None;
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
        return;
    }
    if mouse.just_pressed(MouseButton::Left) {
        charge.started = Some(now);
    }
    if mouse.just_released(MouseButton::Left) {
        let amount = charge.amount(now).unwrap_or(0.0);
        actions.0.push(Action::Throw { aim, charge: amount, curve, lob: false });
        charge.started = None;
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
    fn angle_blend_takes_short_way() {
        let a = lerp_angle(3.0, -3.0, 0.5);
        // 3.0 and -3.0 are 0.28 rad apart across +/-PI; halfway is near PI, not 0.
        assert!((a.abs() - std::f32::consts::PI).abs() < 0.05, "{a}");
    }
}
