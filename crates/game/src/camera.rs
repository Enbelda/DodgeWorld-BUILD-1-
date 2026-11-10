//! Over-the-shoulder camera with mouse look. The cursor locks on start;
//! Esc releases it and a click locks it again.

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use bevy_rapier3d::plugin::PhysicsSet;

use crate::player::LocalBody;
use crate::sim::layout::ARENA_HALF;

const SENSITIVITY: f32 = 0.0025;
const MIN_PITCH: f32 = -1.2;
const MAX_PITCH: f32 = 0.9;
/// Camera offset in look space: right of the shoulder, slightly up, behind.
const OFFSET: Vec3 = Vec3::new(1.1, 0.55, 5.0);

#[derive(Component)]
pub struct FollowCamera {
    pub yaw: f32,
    pub pitch: f32,
}

#[derive(Resource, Default)]
pub struct CursorLocked(pub bool);

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CursorLocked>()
            .add_systems(Startup, spawn_camera)
            .add_systems(Update, (cursor_lock, look).chain())
            // Place the camera after physics moved the body, before transforms propagate.
            .add_systems(
                PostUpdate,
                place_camera.after(PhysicsSet::Writeback).before(TransformSystems::Propagate),
            );
    }
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        FollowCamera { yaw: 0.0, pitch: -0.2 },
        Transform::from_xyz(0.0, 4.0, 20.0),
    ));
}

fn cursor_lock(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut locked: ResMut<CursorLocked>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
    mut first: Local<bool>,
    auto: Res<crate::player::Autopilot>,
) {
    if auto.0 {
        // Never grab the mouse during automated play-tests.
        return;
    }
    let want = if !*first {
        *first = true;
        Some(true)
    } else if keys.just_pressed(KeyCode::Escape) {
        Some(false)
    } else if !locked.0 && mouse.just_pressed(MouseButton::Left) {
        Some(true)
    } else {
        None
    };
    if let Some(lock) = want {
        // The click that re-locks must not also count as a throw: lock takes effect next frame.
        locked.0 = lock && !mouse.just_pressed(MouseButton::Left);
        cursor.grab_mode = if lock { CursorGrabMode::Locked } else { CursorGrabMode::None };
        cursor.visible = !lock;
    } else if cursor.grab_mode == CursorGrabMode::Locked {
        locked.0 = true;
    }
}

pub fn look(mouse: Res<AccumulatedMouseMotion>, locked: Res<CursorLocked>, mut cam: Single<&mut FollowCamera>) {
    if !locked.0 {
        return;
    }
    cam.yaw -= mouse.delta.x * SENSITIVITY;
    cam.pitch = (cam.pitch - mouse.delta.y * SENSITIVITY).clamp(MIN_PITCH, MAX_PITCH);
}

/// Camera position for a body centre and look angles, kept inside the arena walls.
pub fn camera_position(body: Vec3, yaw: f32, pitch: f32) -> Vec3 {
    let rot = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
    let pivot = body + Vec3::Y * 0.7;
    let mut p = pivot + rot * OFFSET;
    let lim = ARENA_HALF - 0.3;
    p.x = p.x.clamp(-lim, lim);
    p.z = p.z.clamp(-lim, lim);
    p.y = p.y.max(0.3);
    p
}

fn place_camera(
    body: Single<&Transform, (With<LocalBody>, Without<FollowCamera>)>,
    cam: Single<(&FollowCamera, &mut Transform)>,
) {
    let (cam, mut tf) = cam.into_inner();
    tf.translation = camera_position(body.translation, cam.yaw, cam.pitch);
    tf.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_sits_behind_and_right() {
        let p = camera_position(Vec3::ZERO, 0.0, 0.0);
        assert!(p.z > 3.0, "behind the player when looking down -Z");
        assert!(p.x > 0.0, "over the right shoulder");
    }

    #[test]
    fn camera_stays_inside_walls() {
        let p = camera_position(Vec3::new(0.0, 1.0, ARENA_HALF - 0.5), 0.0, 0.0);
        assert!(p.z <= ARENA_HALF - 0.3);
    }
}
