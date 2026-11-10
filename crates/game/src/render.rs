//! Draws other players and the balls from the latest `View` snapshot.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::camera::FollowCamera;
use crate::player::{LocalBody, FEET_OFFSET};
use crate::session::{SessionSet, View};
use crate::sim::{ball::BALL_RADIUS, BallState, PlayerId, Team, BODY_HEIGHT, BODY_RADIUS, HOLD_HEIGHT};

/// Higher = snappier smoothing of remote positions.
const SMOOTHING: f32 = 18.0;

#[derive(Resource)]
struct Palette {
    body: [Handle<StandardMaterial>; 2],
    visor: Handle<StandardMaterial>,
    visor_catch: Handle<StandardMaterial>,
    ball_free: Handle<StandardMaterial>,
    ball_team: [Handle<StandardMaterial>; 2],
    ball_powered: Handle<StandardMaterial>,
    capsule: Handle<Mesh>,
    visor_mesh: Handle<Mesh>,
    ball: Handle<Mesh>,
}

#[derive(Component)]
struct Avatar {
    id: PlayerId,
    body: Entity,
    visor: Entity,
}

#[derive(Component)]
struct BallVis(usize);

/// Visor on the local body, so others' catch state reads the same way as ours.
#[derive(Component)]
struct LocalVisor;

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, make_palette)
            // PostStartup: the local body is spawned during Startup.
            .add_systems(PostStartup, add_local_visor)
            .add_systems(Update, (sync_avatars, sync_local_body, sync_balls).after(SessionSet));
    }
}

fn team_index(t: Team) -> usize {
    t.index()
}

fn make_palette(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let emissive = |m: &mut Assets<StandardMaterial>, c: Color, glow: LinearRgba| {
        m.add(StandardMaterial { base_color: c, emissive: glow, ..default() })
    };
    let half_segment = BODY_HEIGHT - 2.0 * BODY_RADIUS;
    commands.insert_resource(Palette {
        body: [materials.add(Color::srgb(0.90, 0.25, 0.22)), materials.add(Color::srgb(0.22, 0.45, 0.95))],
        visor: materials.add(Color::srgb(0.1, 0.1, 0.12)),
        visor_catch: emissive(&mut materials, Color::srgb(1.0, 1.0, 0.6), LinearRgba::rgb(4.0, 4.0, 1.5)),
        ball_free: materials.add(Color::srgb(0.95, 0.95, 0.92)),
        ball_team: [
            emissive(&mut materials, Color::srgb(1.0, 0.3, 0.25), LinearRgba::rgb(3.0, 0.4, 0.3)),
            emissive(&mut materials, Color::srgb(0.3, 0.5, 1.0), LinearRgba::rgb(0.4, 0.8, 3.0)),
        ],
        ball_powered: emissive(&mut materials, Color::srgb(1.0, 0.8, 0.2), LinearRgba::rgb(5.0, 3.5, 0.5)),
        capsule: meshes.add(Capsule3d::new(BODY_RADIUS, half_segment)),
        visor_mesh: meshes.add(Cuboid::new(0.55, 0.18, 0.12)),
        ball: meshes.add(Sphere::new(BALL_RADIUS)),
    });
}

fn add_local_visor(mut commands: Commands, palette: Res<Palette>, body: Single<Entity, With<LocalBody>>) {
    commands.spawn((
        LocalVisor,
        Mesh3d(palette.visor_mesh.clone()),
        MeshMaterial3d(palette.visor.clone()),
        Transform::from_xyz(0.0, 0.55, -0.4),
        ChildOf(*body),
    ));
}

fn smooth(dt: f32) -> f32 {
    1.0 - (-SMOOTHING * dt).exp()
}

fn sync_avatars(
    mut commands: Commands,
    time: Res<Time>,
    view: Res<View>,
    palette: Res<Palette>,
    mut avatars: Query<(Entity, &Avatar, &mut Transform, &mut Visibility)>,
    mut mats: Query<&mut MeshMaterial3d<StandardMaterial>>,
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
        if let Ok(mut m) = mats.get_mut(av.body) {
            m.0 = palette.body[team_index(p.team)].clone();
        }
        if let Ok(mut m) = mats.get_mut(av.visor) {
            m.0 = if p.catching { palette.visor_catch.clone() } else { palette.visor.clone() };
        }
    }

    for p in &snap.players {
        if Some(p.id) == view.local || seen.contains_key(&p.id) {
            continue;
        }
        let root = commands
            .spawn((Transform::from_translation(Vec3::from_array(p.pos)), Visibility::default()))
            .id();
        let body = commands
            .spawn((
                Mesh3d(palette.capsule.clone()),
                MeshMaterial3d(palette.body[team_index(p.team)].clone()),
                Transform::from_xyz(0.0, FEET_OFFSET, 0.0),
                ChildOf(root),
            ))
            .id();
        let visor = commands
            .spawn((
                Mesh3d(palette.visor_mesh.clone()),
                MeshMaterial3d(palette.visor.clone()),
                Transform::from_xyz(0.0, FEET_OFFSET + 0.55, -0.4),
                ChildOf(root),
            ))
            .id();
        commands.entity(root).insert(Avatar { id: p.id, body, visor });
    }
}

fn sync_local_body(
    view: Res<View>,
    palette: Res<Palette>,
    body: Single<(&mut MeshMaterial3d<StandardMaterial>, &mut Visibility), With<LocalBody>>,
    visor: Single<&mut MeshMaterial3d<StandardMaterial>, (With<LocalVisor>, Without<LocalBody>)>,
) {
    let Some(me) = view.me() else { return };
    let (mut mat, mut vis) = body.into_inner();
    mat.0 = palette.body[team_index(me.team)].clone();
    *vis = if me.alive() { Visibility::Inherited } else { Visibility::Hidden };
    visor.into_inner().0 = if me.catching { palette.visor_catch.clone() } else { palette.visor.clone() };
}

fn sync_balls(
    mut commands: Commands,
    time: Res<Time>,
    view: Res<View>,
    palette: Res<Palette>,
    body: Single<&Transform, (With<LocalBody>, Without<BallVis>)>,
    cam: Single<&FollowCamera>,
    mut balls: Query<(Entity, &BallVis, &mut Transform, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let Some(snap) = view.snap.as_ref() else { return };
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
                let forward = Vec3::new(-cam.yaw.sin(), 0.0, -cam.yaw.cos());
                tf.translation = body.translation - Vec3::Y * FEET_OFFSET + Vec3::Y * HOLD_HEIGHT + forward * 0.45;
            }
            _ if view.is_host || tf.translation.distance(target) > 3.0 => tf.translation = target,
            _ => tf.translation = tf.translation.lerp(target, k),
        }
        let powered_holder =
            |by: PlayerId| snap.players.iter().any(|p| p.id == by && p.powered && p.holding);
        mat.0 = match b.state {
            BallState::Free => palette.ball_free.clone(),
            BallState::Held { by } if powered_holder(by) => palette.ball_powered.clone(),
            BallState::Held { .. } => palette.ball_free.clone(),
            BallState::Live { powered: true, .. } => palette.ball_powered.clone(),
            BallState::Live { team, .. } => palette.ball_team[team_index(team)].clone(),
        };
    }

    for (i, b) in snap.balls.iter().enumerate() {
        if !have[i] {
            commands.spawn((
                BallVis(i),
                Mesh3d(palette.ball.clone()),
                MeshMaterial3d(palette.ball_free.clone()),
                Transform::from_translation(Vec3::from_array(b.pos)),
            ));
        }
    }
}
