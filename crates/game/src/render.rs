//! Draws other players (as humanoid rigs) and the balls from the latest `View` snapshot.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::camera::FollowCamera;
use crate::player::{LocalBody, LockOn, FEET_OFFSET};
use crate::rig::{animate_rigs, make_rig_assets, spawn_rig, RigAssets};
use crate::session::{SessionSet, View};
use crate::sim::{ball::{BallKind, BALL_RADIUS}, BallState, PlayerId, Team, BODY_HEIGHT, HOLD_HEIGHT};

/// Higher = snappier smoothing of remote positions.
const SMOOTHING: f32 = 18.0;

#[derive(Resource)]
struct Palette {
    ball_free: Handle<StandardMaterial>,
    ball_team: [Handle<StandardMaterial>; 2],
    ball_powered: Handle<StandardMaterial>,
    /// Boomerang, slime and brick: [resting, flying].
    ball_boomerang: [Handle<StandardMaterial>; 2],
    ball_slime: [Handle<StandardMaterial>; 2],
    ball_brick: [Handle<StandardMaterial>; 2],
    ball: Handle<Mesh>,
    brick: Handle<Mesh>,
}

/// Another player: the root sits at their feet and turns with their facing.
#[derive(Component)]
struct Avatar {
    id: PlayerId,
}

#[derive(Component)]
struct BallVis(usize);

/// Red ring floating over the enemy we are locked on to.
#[derive(Component)]
struct LockMarker;

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<crate::rig::LocalRigInput>()
            .add_systems(Startup, (make_palette, make_rig_assets, spawn_lock_marker))
            // PostStartup: the local body is spawned during Startup.
            .add_systems(PostStartup, attach_local_rig)
            .add_systems(
                Update,
                ((sync_avatars, sync_lock_marker).chain(), sync_local_body, sync_balls, animate_rigs).after(SessionSet),
            );
    }
}

fn team_index(t: Team) -> usize {
    t.index()
}

fn make_palette(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let emissive = |m: &mut Assets<StandardMaterial>, c: Color, glow: LinearRgba| {
        m.add(StandardMaterial { base_color: c, emissive: glow, ..default() })
    };
    commands.insert_resource(Palette {
        ball_free: materials.add(Color::srgb(0.95, 0.95, 0.92)),
        ball_team: [
            emissive(&mut materials, Color::srgb(1.0, 0.3, 0.25), LinearRgba::rgb(3.0, 0.4, 0.3)),
            emissive(&mut materials, Color::srgb(0.3, 0.5, 1.0), LinearRgba::rgb(0.4, 0.8, 3.0)),
        ],
        ball_powered: emissive(&mut materials, Color::srgb(1.0, 0.8, 0.2), LinearRgba::rgb(5.0, 3.5, 0.5)),
        ball_boomerang: [
            materials.add(Color::srgb(0.62, 0.35, 0.9)),
            emissive(&mut materials, Color::srgb(0.75, 0.45, 1.0), LinearRgba::rgb(2.5, 0.8, 4.0)),
        ],
        ball_slime: [
            materials.add(StandardMaterial { base_color: Color::srgb(0.35, 0.85, 0.3), perceptual_roughness: 0.15, ..default() }),
            emissive(&mut materials, Color::srgb(0.4, 1.0, 0.35), LinearRgba::rgb(0.8, 4.0, 0.6)),
        ],
        ball_brick: [
            materials.add(StandardMaterial { base_color: Color::srgb(0.75, 0.12, 0.08), perceptual_roughness: 0.9, ..default() }),
            emissive(&mut materials, Color::srgb(0.85, 0.15, 0.1), LinearRgba::rgb(2.5, 0.2, 0.1)),
        ],
        ball: meshes.add(Sphere::new(BALL_RADIUS)),
        brick: meshes.add(Cuboid::new(0.62, 0.3, 0.34)),
    });
}

fn attach_local_rig(mut commands: Commands, assets: Res<RigAssets>, body: Single<Entity, With<LocalBody>>) {
    spawn_rig(&mut commands, &assets, *body, Vec3::NEG_Y * FEET_OFFSET, None);
}

fn smooth(dt: f32) -> f32 {
    1.0 - (-SMOOTHING * dt).exp()
}

fn sync_avatars(
    mut commands: Commands,
    time: Res<Time>,
    view: Res<View>,
    assets: Res<RigAssets>,
    mut avatars: Query<(Entity, &Avatar, &mut Transform, &mut Visibility)>,
) {
    let Some(snap) = view.snap.as_ref() else { return };
    let mut seen: HashMap<PlayerId, ()> = HashMap::new();
    let k = smooth(time.delta_secs());

    for (entity, av, mut tf, mut vis) in &mut avatars {
        let Some(p) = snap.players.iter().find(|p| p.id == av.id && Some(p.id) != view.local) else {
            commands.entity(entity).despawn();
            continue;
        };
        seen.insert(p.id, ());
        let target = Vec3::from_array(p.pos);
        // Snap on big jumps (respawns), smooth otherwise.
        if tf.translation.distance(target) > 4.0 {
            tf.translation = target;
        } else {
            tf.translation = tf.translation.lerp(target, k);
        }
        tf.rotation = tf.rotation.slerp(Quat::from_rotation_y(p.yaw), k);
        *vis = if p.alive() { Visibility::Inherited } else { Visibility::Hidden };
    }

    for p in &snap.players {
        if Some(p.id) == view.local || seen.contains_key(&p.id) {
            continue;
        }
        let root = commands
            .spawn((
                Avatar { id: p.id },
                Transform::from_translation(Vec3::from_array(p.pos)).with_rotation(Quat::from_rotation_y(p.yaw)),
                Visibility::default(),
            ))
            .id();
        spawn_rig(&mut commands, &assets, root, Vec3::ZERO, Some(p.id));
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_local_body(
    view: Res<View>,
    charge: Res<crate::player::Charge>,
    trick: Res<crate::session::LocalTrick>,
    dash: Res<crate::player::LocalDash>,
    parkour: Res<crate::player::LocalParkour>,
    time: Res<Time>,
    mut rig_input: ResMut<crate::rig::LocalRigInput>,
    body: Single<&mut Visibility, With<LocalBody>>,
) {
    rig_input.charge = charge.amount(time.elapsed_secs());
    rig_input.trick = trick.0;
    rig_input.dashing = dash.dashing();
    rig_input.parkour = parkour.0.pose();
    let Some(me) = view.me() else { return };
    *body.into_inner() = if me.alive() { Visibility::Inherited } else { Visibility::Hidden };
}

#[allow(clippy::too_many_arguments)]
fn sync_balls(
    mut commands: Commands,
    time: Res<Time>,
    view: Res<View>,
    palette: Res<Palette>,
    body: Single<&Transform, (With<LocalBody>, Without<BallVis>)>,
    cam: Single<&FollowCamera>,
    facing: Res<crate::player::LocalFacing>,
    mut balls: Query<(Entity, &BallVis, &mut Transform, &mut MeshMaterial3d<StandardMaterial>)>,
    mut spin: Local<f32>,
) {
    let Some(snap) = view.snap.as_ref() else { return };
    *spin += time.delta_secs() * 4.0;
    let k = smooth(time.delta_secs() * 1.5);
    let mut have = vec![false; snap.balls.len()];

    for (entity, vis, mut tf, mut mat) in &mut balls {
        let Some(b) = snap.balls.get(vis.0) else {
            commands.entity(entity).despawn();
            continue;
        };
        have[vis.0] = true;
        let target = Vec3::from_array(b.pos);
        match b.state {
            // Our own held ball follows our body without network lag.
            BallState::Held { by } if Some(by) == view.local => {
                let yaw = facing.body_yaw(cam.yaw);
                let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
                tf.translation = body.translation - Vec3::Y * FEET_OFFSET + Vec3::Y * HOLD_HEIGHT + forward * 0.45;
            }
            _ if view.is_host || tf.translation.distance(target) > 3.0 => tf.translation = target,
            _ => tf.translation = tf.translation.lerp(target, k),
        }
        let powered_holder =
            |by: PlayerId| snap.players.iter().any(|p| p.id == by && p.powered && p.holding);
        let flying = matches!(b.state, BallState::Live { .. } | BallState::Returning { .. });
        let powered = match b.state {
            BallState::Held { by } => powered_holder(by),
            BallState::Live { powered, .. } => powered,
            _ => false,
        };
        mat.0 = match (b.kind, b.state) {
            _ if powered => palette.ball_powered.clone(),
            (BallKind::Normal, BallState::Live { team, .. }) => palette.ball_team[team_index(team)].clone(),
            (BallKind::Normal, _) => palette.ball_free.clone(),
            (BallKind::Boomerang, _) => palette.ball_boomerang[flying as usize].clone(),
            (BallKind::Slime, _) => palette.ball_slime[flying as usize].clone(),
            (BallKind::Brick, _) => palette.ball_brick[flying as usize].clone(),
        };
        // Shape cues: the boomerang is a spinning disc, slime is a wobbly blob.
        match b.kind {
            BallKind::Normal => tf.scale = Vec3::ONE,
            BallKind::Boomerang => {
                tf.scale = Vec3::new(1.35, 0.55, 1.35);
                tf.rotation = Quat::from_rotation_y(*spin * 3.0) * Quat::from_rotation_x(0.3);
            }
            BallKind::Slime => {
                let w = 1.0 + 0.08 * (*spin * 9.0 + vis.0 as f32).sin();
                tf.scale = Vec3::new(1.15 * w, 1.15 / w, 1.15 * w);
            }
            // The brick tumbles end over end in flight and lies flat otherwise.
            BallKind::Brick => {
                tf.rotation = if flying { Quat::from_rotation_x(*spin * 2.5) } else { Quat::IDENTITY };
            }
        }
    }

    for (i, b) in snap.balls.iter().enumerate() {
        if !have[i] {
            let mesh = if b.kind == BallKind::Brick { palette.brick.clone() } else { palette.ball.clone() };
            commands.spawn((
                BallVis(i),
                Mesh3d(mesh),
                MeshMaterial3d(palette.ball_free.clone()),
                Transform::from_translation(Vec3::from_array(b.pos)),
            ));
        }
    }
}

fn spawn_lock_marker(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.spawn((
        LockMarker,
        Mesh3d(meshes.add(Torus::new(0.3, 0.42))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.15, 0.1),
            emissive: LinearRgba::rgb(6.0, 0.5, 0.3),
            unlit: true,
            ..default()
        })),
        Transform::default(),
        Visibility::Hidden,
    ));
}

/// Float the lock ring over the target's head and spin it.
fn sync_lock_marker(
    time: Res<Time>,
    lock: Res<LockOn>,
    avatars: Query<(&Avatar, &Transform), Without<LockMarker>>,
    marker: Single<(&mut Transform, &mut Visibility), With<LockMarker>>,
) {
    let (mut tf, mut vis) = marker.into_inner();
    let target = lock.0.and_then(|t| avatars.iter().find(|(a, _)| a.id == t));
    match target {
        Some((_, at)) => {
            *vis = Visibility::Visible;
            tf.translation = at.translation + Vec3::Y * (BODY_HEIGHT + 0.6);
            tf.rotation = Quat::from_rotation_y(time.elapsed_secs() * 3.0);
        }
        None => *vis = Visibility::Hidden,
    }
}
