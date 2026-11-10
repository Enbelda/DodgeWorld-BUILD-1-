//! The local player's body (Rapier character controller) and combat input.
//!
//! Controls: WASD move, Space jump, hold Left Mouse to charge a throw and release to throw,
//! hold Q / E while throwing to curve left / right, F to lob, Right Mouse to catch.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::camera::{CursorLocked, FollowCamera};
use crate::session::{LocalActions, View};
use crate::sim::{layout, Action, BODY_HEIGHT, BODY_RADIUS, HOLD_HEIGHT};

const MOVE_SPEED: f32 = 7.0;
const JUMP_SPEED: f32 = 8.0;
const GRAVITY: f32 = 22.0;
/// Seconds of holding the throw button for a full-power throw.
pub const FULL_CHARGE_SECS: f32 = 1.0;
/// The body's origin sits at the capsule centre, this far above the feet.
pub const FEET_OFFSET: f32 = BODY_HEIGHT / 2.0;

#[derive(Component, Default)]
pub struct LocalBody {
    pub vel_y: f32,
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
            .add_systems(Startup, spawn_local_body)
            .add_systems(
                Update,
                (obey_teleports, keyboard_intent, crate::autopilot::drive, move_local_body, combat_input)
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

pub fn move_local_body(
    time: Res<Time>,
    intent: Res<MoveIntent>,
    view: Res<View>,
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
    let alive = view.me().is_none_or(|m| m.alive());
    tf.rotation = Quat::from_rotation_y(cam.yaw);
    if !alive {
        controller.translation = None;
        return;
    }
    let grounded = output.is_some_and(|o| o.grounded);
    if grounded && b.vel_y < 0.0 {
        b.vel_y = 0.0;
    }
    if grounded && intent.jump {
        b.vel_y = JUMP_SPEED;
    }
    b.vel_y -= GRAVITY * dt;
    let flat = intent.dir.normalize_or_zero() * MOVE_SPEED;
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
    mut charge: ResMut<Charge>,
    mut actions: ResMut<LocalActions>,
    body: Single<&Transform, With<LocalBody>>,
    cam: Single<&Transform, (With<FollowCamera>, Without<LocalBody>)>,
    follow: Single<&FollowCamera>,
) {
    let now = time.elapsed_secs();
    let Some(me) = view.me() else { return };
    if auto.0 || !locked.0 || !me.alive() {
        charge.started = None;
        return;
    }

    if mouse.just_pressed(MouseButton::Right) && !me.holding && me.catch_cooldown <= 0.0 {
        actions.0.push(Action::Catch);
    }
    if !me.holding {
        charge.started = None;
        return;
    }

    // Aim where the crosshair points, 40 m out, from where the ball is held.
    let feet = body.translation - Vec3::Y * FEET_OFFSET;
    let forward = Vec3::new(-follow.yaw.sin(), 0.0, -follow.yaw.cos());
    let hold = feet + Vec3::Y * HOLD_HEIGHT + forward * 0.45;
    let target = cam.translation + cam.forward() * 40.0;
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
}
