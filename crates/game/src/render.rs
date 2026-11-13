//! Draws other players (as humanoid rigs) and the balls from the latest `View` snapshot.
//!
//! Balls (all but the brick) are smooth, textured spheres with a pebbled grip and seams.
//! In flight they turn fluid: stretched along their path and jiggling like jelly, spinning
//! about their flight line, with swirling wind streaks and a fading trail behind them. At
//! rest or in hand they are round again. A held ball sits in the holder's palm (or cradled
//! between both hands just after a catch). Throws and catches set off a shockwave ring.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::transform::TransformSystems;

use crate::player::{LocalBody, LockOn, FEET_OFFSET};
use crate::rig::{animate_rigs, make_rig_assets, spawn_rig, Rig, RigAssets};
use crate::session::{SessionSet, View};
use crate::sim::ball::{BallKind, BALL_RADIUS};
use crate::sim::{BallState, GameEvent, PlayerId, Team, BODY_HEIGHT};
use crate::textures::Textures;

/// Higher = snappier smoothing of remote positions.
const SMOOTHING: f32 = 18.0;
/// Wind streaks and trail ghosts per ball.
const STREAKS: usize = 6;
const GHOSTS: usize = 5;
/// Flight speed (m/s) at which the wind effects are fully on.
const WIND_FULL_SPEED: f32 = 30.0;

#[derive(Resource)]
struct Palette {
    ball_free: Handle<StandardMaterial>,
    ball_team: [Handle<StandardMaterial>; 2],
    ball_powered: Handle<StandardMaterial>,
    /// Boomerang, slime and brick: [resting, flying].
    ball_boomerang: [Handle<StandardMaterial>; 2],
    ball_slime: [Handle<StandardMaterial>; 2],
    ball_brick: [Handle<StandardMaterial>; 2],
    ball_golden: [Handle<StandardMaterial>; 2],
    string: Handle<StandardMaterial>,
    string_mesh: Handle<Mesh>,
    ring: Handle<StandardMaterial>,
    ball: Handle<Mesh>,
    brick: Handle<Mesh>,
    torus: Handle<Mesh>,
    streak: Handle<Mesh>,
    wind: Handle<StandardMaterial>,
    ghosts: Vec<Handle<StandardMaterial>>,
    shock: Handle<Mesh>,
}

/// Another player: the root sits at their feet and turns with their facing.
#[derive(Component)]
struct Avatar {
    id: PlayerId,
}

#[derive(Component)]
struct BallVis {
    index: usize,
    /// What kind it was spawned as (the mesh and ring depend on it).
    kind: BallKind,
    /// Flying last frame (to start the jiggle and the throw burst).
    was_flying: bool,
    jiggle: f32,
    phase: f32,
    spin: f32,
    /// Recent positions, newest first (for the trail).
    trail: Vec<Vec3>,
}

/// The spinning band round the boomerang ball.
#[derive(Component)]
struct BoomerangRing;

/// The golden ball's string.
#[derive(Component)]
struct GoldString;

/// A wind streak (`k`) or trail ghost behind ball `ball`.
#[derive(Component)]
struct WindFx {
    ball: usize,
    k: usize,
    ghost: bool,
}

/// An expanding, fading shockwave ring.
#[derive(Component)]
struct Burst {
    t: f32,
    life: f32,
    size: f32,
    mat: Handle<StandardMaterial>,
    alpha: f32,
}

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
                (
                    (sync_avatars, sync_lock_marker).chain(),
                    sync_local_body,
                    (sync_balls, sync_wind, catch_bursts, animate_bursts).chain(),
                    animate_rigs,
                )
                    .after(SessionSet),
            )
            // Once the rigs' joints have their final world positions, put held balls in hand.
            .add_systems(PostUpdate, place_held_balls.after(TransformSystems::Propagate));
    }
}

fn team_index(t: Team) -> usize {
    t.index()
}

fn make_palette(
    mut commands: Commands,
    textures: Res<Textures>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let grip = &textures.ball;
    // Rubber balls: pebbled grip texture with a normal map, a soft sheen.
    let rubber = |m: &mut Assets<StandardMaterial>, c: Color, glow: LinearRgba, rough: f32| {
        m.add(StandardMaterial {
            base_color: c,
            emissive: glow,
            base_color_texture: Some(grip.color.clone()),
            normal_map_texture: Some(grip.normal.clone()),
            perceptual_roughness: rough,
            clearcoat: 0.3,
            clearcoat_perceptual_roughness: 0.4,
            ..default()
        })
    };
    let goo = |m: &mut Assets<StandardMaterial>, c: Color, glow: LinearRgba| {
        m.add(StandardMaterial {
            base_color: c,
            emissive: glow,
            perceptual_roughness: 0.08,
            clearcoat: 1.0,
            clearcoat_perceptual_roughness: 0.05,
            ..default()
        })
    };
    let additive = |m: &mut Assets<StandardMaterial>, c: Color| {
        m.add(StandardMaterial { base_color: c, unlit: true, alpha_mode: AlphaMode::Add, ..default() })
    };
    let ball_mesh = Sphere::new(BALL_RADIUS).mesh().uv(48, 28);
    let ball_mesh = ball_mesh.clone().with_generated_tangents().unwrap_or(ball_mesh);
    let ghosts = (0..GHOSTS)
        .map(|k| additive(&mut materials, Color::srgba(0.8, 0.9, 1.0, 0.22 * (1.0 - k as f32 / GHOSTS as f32))))
        .collect();
    commands.insert_resource(Palette {
        ball_free: rubber(&mut materials, Color::srgb(0.95, 0.95, 0.92), LinearRgba::BLACK, 0.55),
        ball_team: [
            rubber(&mut materials, Color::srgb(1.0, 0.3, 0.25), LinearRgba::rgb(3.0, 0.4, 0.3), 0.5),
            rubber(&mut materials, Color::srgb(0.3, 0.5, 1.0), LinearRgba::rgb(0.4, 0.8, 3.0), 0.5),
        ],
        ball_powered: rubber(&mut materials, Color::srgb(1.0, 0.8, 0.2), LinearRgba::rgb(5.0, 3.5, 0.5), 0.4),
        ball_boomerang: [
            rubber(&mut materials, Color::srgb(0.62, 0.35, 0.9), LinearRgba::BLACK, 0.5),
            rubber(&mut materials, Color::srgb(0.75, 0.45, 1.0), LinearRgba::rgb(2.5, 0.8, 4.0), 0.5),
        ],
        ball_slime: [
            goo(&mut materials, Color::srgb(0.35, 0.85, 0.3), LinearRgba::BLACK),
            goo(&mut materials, Color::srgb(0.4, 1.0, 0.35), LinearRgba::rgb(0.8, 4.0, 0.6)),
        ],
        ball_brick: [
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.75, 0.12, 0.08),
                base_color_texture: Some(textures.concrete.color.clone()),
                normal_map_texture: Some(textures.concrete.normal.clone()),
                perceptual_roughness: 0.9,
                ..default()
            }),
            materials.add(StandardMaterial {
                base_color: Color::srgb(0.85, 0.15, 0.1),
                emissive: LinearRgba::rgb(2.5, 0.2, 0.1),
                base_color_texture: Some(textures.concrete.color.clone()),
                perceptual_roughness: 0.9,
                ..default()
            }),
        ],
        ball_golden: [
            materials.add(StandardMaterial {
                base_color: Color::srgb(1.0, 0.78, 0.25),
                metallic: 0.95,
                perceptual_roughness: 0.18,
                emissive: LinearRgba::rgb(0.6, 0.4, 0.05),
                clearcoat: 1.0,
                ..default()
            }),
            materials.add(StandardMaterial {
                base_color: Color::srgb(1.0, 0.85, 0.3),
                metallic: 0.9,
                perceptual_roughness: 0.15,
                emissive: LinearRgba::rgb(4.0, 2.8, 0.4),
                ..default()
            }),
        ],
        string: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.9, 0.55),
            emissive: LinearRgba::rgb(1.6, 1.2, 0.3),
            unlit: true,
            ..default()
        }),
        string_mesh: meshes.add(Cylinder::new(0.025, 1.0)),
        ring: materials.add(StandardMaterial {
            base_color: Color::srgb(0.95, 0.85, 1.0),
            emissive: LinearRgba::rgb(1.5, 0.8, 2.5),
            metallic: 0.8,
            perceptual_roughness: 0.25,
            ..default()
        }),
        ball: meshes.add(ball_mesh),
        brick: meshes.add(Cuboid::new(0.62, 0.3, 0.34)),
        torus: meshes.add(Torus::new(BALL_RADIUS * 1.02, BALL_RADIUS * 1.22).mesh().minor_resolution(12).major_resolution(36)),
        streak: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        wind: additive(&mut materials, Color::srgba(0.9, 0.95, 1.0, 0.35)),
        ghosts,
        shock: meshes.add(Torus::new(0.85, 1.0).mesh().minor_resolution(8).major_resolution(40)),
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
    cover: Res<crate::player::LocalCover>,
    time: Res<Time>,
    mut rig_input: ResMut<crate::rig::LocalRigInput>,
    body: Single<&mut Visibility, With<LocalBody>>,
) {
    rig_input.charge = charge.amount(time.elapsed_secs());
    rig_input.trick = trick.0;
    rig_input.dashing = dash.dashing();
    rig_input.parkour = parkour.0.pose();
    rig_input.crouch = cover.crouched;
    let Some(me) = view.me() else { return };
    *body.into_inner() = if me.alive() { Visibility::Inherited } else { Visibility::Hidden };
}

/// Fluid shape of a flying ball: stretched along the flight by speed, jiggling with
/// `jiggle` (0..1) at `phase`. Returns the local scale (Z = along the flight).
pub fn fluid_scale(speed: f32, jiggle: f32, phase: f32) -> Vec3 {
    let stretch = 1.0 + (speed * 0.008).min(0.28);
    let wob = 0.14 * jiggle * phase.sin();
    let side = 1.0 / stretch.sqrt();
    Vec3::new(side * (1.0 + wob), side * (1.0 - wob), stretch * (1.0 + 0.5 * wob * (phase * 1.7).cos()))
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn sync_balls(
    mut commands: Commands,
    time: Res<Time>,
    view: Res<View>,
    palette: Res<Palette>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut balls: Query<(Entity, &mut BallVis, &mut Transform, &mut MeshMaterial3d<StandardMaterial>), (Without<BoomerangRing>, Without<GoldString>)>,
    mut rings: Query<(&ChildOf, &mut Transform), (With<BoomerangRing>, Without<GoldString>)>,
    fx: Query<(Entity, &WindFx)>,
    mut string: Query<(&mut Transform, &mut Visibility), With<GoldString>>,
    mut spin: Local<f32>,
) {
    let Some(snap) = view.snap.as_ref() else { return };
    let dt = time.delta_secs();
    *spin += dt * 4.0;
    let k = smooth(dt * 1.5);
    let mut have = vec![false; snap.balls.len()];

    for (entity, mut vis, mut tf, mut mat) in &mut balls {
        let Some(b) = snap.balls.get(vis.index) else {
            commands.entity(entity).despawn();
            continue;
        };
        if b.kind != vis.kind {
            // The map changed and this slot is a different ball now: build it afresh.
            commands.entity(entity).despawn();
            for (e, w) in &fx {
                if w.ball == vis.index {
                    commands.entity(e).despawn();
                }
            }
            continue;
        }
        have[vis.index] = true;
        let target = Vec3::from_array(b.pos);
        match b.state {
            // Held balls are put in the holder's hand after transforms propagate.
            BallState::Held { .. } => tf.translation = target,
            _ if view.is_host || tf.translation.distance(target) > 3.0 => tf.translation = target,
            _ => tf.translation = tf.translation.lerp(target, k),
        }
        let powered_holder = |by: PlayerId| snap.players.iter().any(|p| p.id == by && p.powered && p.holding);
        let flying = matches!(b.state, BallState::Live { .. } | BallState::Returning { .. } | BallState::Ricochet { .. });
        let powered = match b.state {
            BallState::Held { by } => powered_holder(by),
            BallState::Live { powered, .. } => powered,
            _ => false,
        };
        let want = match (b.kind, b.state) {
            _ if powered && b.kind != BallKind::Brick => &palette.ball_powered,
            (BallKind::Normal, BallState::Live { team, .. }) => &palette.ball_team[team_index(team)],
            (BallKind::Normal, _) => &palette.ball_free,
            (BallKind::Boomerang, _) => &palette.ball_boomerang[flying as usize],
            (BallKind::Slime, _) => &palette.ball_slime[flying as usize],
            (BallKind::Brick, _) => &palette.ball_brick[flying as usize],
            (BallKind::Golden, _) => &palette.ball_golden[flying as usize],
        };
        if mat.0 != *want {
            mat.0 = want.clone();
        }

        // Thrown: a burst at the hand and a fresh jiggle.
        let vel = Vec3::from_array(b.vel);
        let speed = vel.length();
        if flying && !vis.was_flying {
            vis.jiggle = 1.0;
            vis.trail.clear();
            if matches!(b.state, BallState::Live { .. }) {
                let c = Color::srgba(0.9, 0.95, 1.0, 0.45);
                spawn_burst(&mut commands, &palette.shock, &mut materials, tf.translation, vel, 0.7, c, 0.25);
            }
        }
        vis.was_flying = flying;
        vis.trail.insert(0, tf.translation);
        vis.trail.truncate(GHOSTS * 2 + 1);

        if b.kind == BallKind::Brick {
            // The brick tumbles end over end in flight and lies flat otherwise.
            tf.scale = Vec3::ONE;
            tf.rotation = if flying { Quat::from_rotation_x(*spin * 2.5) } else { Quat::IDENTITY };
            continue;
        }
        if flying && speed > 1.0 {
            // Fluid in flight: stretched along the path, jiggling, spinning about it.
            let goo = if b.kind == BallKind::Slime { 1.6 } else { 1.0 };
            vis.jiggle = (vis.jiggle - dt * 1.5).max(0.35);
            vis.phase += dt * (22.0 + speed * 0.3);
            vis.spin += dt * speed / BALL_RADIUS * 0.35;
            let dir = vel / speed;
            tf.rotation = Quat::from_rotation_arc(Vec3::Z, dir) * Quat::from_rotation_z(vis.spin);
            tf.scale = fluid_scale(speed, (vis.jiggle * goo).min(1.0), vis.phase);
        } else {
            // Round again at rest and in hand (slime still breathes a little).
            vis.jiggle = 0.0;
            tf.scale = if b.kind == BallKind::Slime {
                let w = 1.0 + 0.05 * (*spin * 9.0 + vis.index as f32).sin();
                Vec3::new(w, 1.0 / w, w)
            } else {
                Vec3::ONE
            };
            if b.kind == BallKind::Boomerang {
                tf.rotation = Quat::from_rotation_y(*spin * 0.5);
            } else if !flying {
                tf.rotation = Quat::IDENTITY;
            }
        }
    }
    // The golden ball's string, from its owner's hand to the ball.
    if let Ok((mut tf, mut vis)) = string.single_mut() {
        let line = snap.golden.and_then(|owner| {
            let o = snap.players.iter().find(|p| p.id == owner && p.alive())?;
            let b = snap.balls.iter().find(|b| b.kind == BallKind::Golden)?;
            if b.state == (BallState::Held { by: owner }) {
                return None;
            }
            let fwd = Vec3::new(-o.yaw.sin(), 0.0, -o.yaw.cos());
            Some((Vec3::from_array(o.pos) + Vec3::Y * 1.3 + fwd * 0.4, Vec3::from_array(b.pos)))
        });
        match line {
            Some((a, b)) if a.distance(b) > 0.3 => {
                let d = b - a;
                tf.translation = (a + b) * 0.5;
                tf.rotation = Quat::from_rotation_arc(Vec3::Y, d.normalize());
                tf.scale = Vec3::new(1.0, d.length(), 1.0);
                *vis = Visibility::Visible;
            }
            _ => *vis = Visibility::Hidden,
        }
    } else {
        commands.spawn((GoldString, Mesh3d(palette.string_mesh.clone()), MeshMaterial3d(palette.string.clone()), Transform::default(), Visibility::Hidden));
    }
    // The boomerang's band spins fast round it, faster in flight.
    for (parent, mut tf) in &mut rings {
        let flying = balls
            .get(parent.parent())
            .ok()
            .and_then(|(_, v, _, _)| snap.balls.get(v.index))
            .is_some_and(|b| matches!(b.state, BallState::Live { .. } | BallState::Returning { .. }));
        let rate = if flying { 18.0 } else { 3.0 };
        tf.rotation = Quat::from_rotation_y(*spin * rate) * Quat::from_rotation_x(0.35);
    }

    for (i, b) in snap.balls.iter().enumerate() {
        if have[i] {
            continue;
        }
        let mesh = if b.kind == BallKind::Brick { palette.brick.clone() } else { palette.ball.clone() };
        let ball = commands
            .spawn((
                BallVis { index: i, kind: b.kind, was_flying: false, jiggle: 0.0, phase: i as f32, spin: 0.0, trail: Vec::new() },
                Mesh3d(mesh),
                MeshMaterial3d(palette.ball_free.clone()),
                Transform::from_translation(Vec3::from_array(b.pos)),
            ))
            .id();
        if b.kind == BallKind::Boomerang {
            commands.spawn((BoomerangRing, Mesh3d(palette.torus.clone()), MeshMaterial3d(palette.ring.clone()), Transform::default(), ChildOf(ball)));
        }
        if b.kind != BallKind::Brick {
            for k in 0..STREAKS {
                commands.spawn((WindFx { ball: i, k, ghost: false }, Mesh3d(palette.streak.clone()), MeshMaterial3d(palette.wind.clone()), Transform::default(), Visibility::Hidden));
            }
            for k in 0..GHOSTS {
                commands.spawn((
                    WindFx { ball: i, k, ghost: true },
                    Mesh3d(palette.ball.clone()),
                    MeshMaterial3d(palette.ghosts[k].clone()),
                    Transform::default(),
                    Visibility::Hidden,
                ));
            }
        }
    }
}

/// Wind streaks swirl round a flying ball and stream off behind it; ghost copies fade out
/// along its recent path.
fn sync_wind(
    time: Res<Time>,
    view: Res<View>,
    balls: Query<(&BallVis, &Transform), Without<WindFx>>,
    mut fx: Query<(&WindFx, &mut Transform, &mut Visibility)>,
) {
    let Some(snap) = view.snap.as_ref() else { return };
    let t = time.elapsed_secs();
    let by_index: HashMap<usize, (&BallVis, &Transform)> = balls.iter().map(|(v, tf)| (v.index, (v, tf))).collect();
    for (w, mut tf, mut vis) in &mut fx {
        let (Some(b), Some((bv, btf))) = (snap.balls.get(w.ball), by_index.get(&w.ball)) else {
            *vis = Visibility::Hidden;
            continue;
        };
        let vel = Vec3::from_array(b.vel);
        let speed = vel.length();
        let flying = matches!(b.state, BallState::Live { .. } | BallState::Returning { .. } | BallState::Ricochet { .. });
        let amount = ((speed - 6.0) / (WIND_FULL_SPEED - 6.0)).clamp(0.0, 1.0);
        if !flying || amount <= 0.0 {
            *vis = Visibility::Hidden;
            continue;
        }
        *vis = Visibility::Inherited;
        let dir = vel / speed;
        let rot = Quat::from_rotation_arc(Vec3::Z, dir);
        if w.ghost {
            let Some(p) = bv.trail.get(w.k * 2 + 2) else {
                *vis = Visibility::Hidden;
                continue;
            };
            tf.translation = *p;
            tf.rotation = rot;
            tf.scale = Vec3::splat((1.0 - w.k as f32 * 0.15) * (0.6 + 0.4 * amount));
        } else {
            let seed = w.k as f32 * 1.73 + w.ball as f32 * 0.37;
            let angle = w.k as f32 * std::f32::consts::TAU / STREAKS as f32 + t * 9.0;
            let flicker = 0.6 + 0.4 * (t * 23.0 + seed * 5.0).sin();
            let len = (0.3 + 1.3 * amount) * flicker;
            let radial = BALL_RADIUS * (1.0 + 0.2 * (seed * 3.1).sin().abs());
            let around = rot * Vec3::new(angle.cos(), angle.sin(), 0.0) * radial;
            tf.translation = btf.translation + around - dir * (BALL_RADIUS * 0.4 + len * 0.5);
            tf.rotation = rot;
            tf.scale = Vec3::new(0.014, 0.014, len);
        }
    }
}

/// An expanding shockwave ring at `at` whose axis points along `dir` (the throw's or
/// catch's direction), fading over `life` seconds.
#[allow(clippy::too_many_arguments)]
fn spawn_burst(
    commands: &mut Commands,
    mesh: &Handle<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    at: Vec3,
    dir: Vec3,
    size: f32,
    color: Color,
    life: f32,
) {
    let mat = materials.add(StandardMaterial { base_color: color, unlit: true, alpha_mode: AlphaMode::Add, ..default() });
    commands.spawn((
        Burst { t: 0.0, life, size, mat: mat.clone(), alpha: color.alpha() },
        Mesh3d(mesh.clone()),
        MeshMaterial3d(mat),
        Transform::from_translation(at)
            .with_rotation(Quat::from_rotation_arc(Vec3::Y, dir.normalize_or(Vec3::Y)))
            .with_scale(Vec3::splat(size * 0.2)),
    ));
}

/// Catches set off a bigger burst at the catcher's chest (gold for a perfect catch).
fn catch_bursts(
    mut commands: Commands,
    view: Res<View>,
    palette: Res<Palette>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(snap) = view.snap.as_ref() else { return };
    for e in &view.events {
        if let GameEvent::Caught { catcher, push, perfect, .. } = e {
            let Some(p) = snap.players.iter().find(|p| p.id == *catcher) else { continue };
            let at = Vec3::from_array(p.pos) + Vec3::Y * 1.2;
            let (color, size) =
                if *perfect { (Color::srgba(1.0, 0.85, 0.3, 0.7), 1.6) } else { (Color::srgba(1.0, 1.0, 1.0, 0.5), 1.1) };
            spawn_burst(&mut commands, &palette.shock, &mut materials, at, Vec3::from_array(*push), size, color, 0.35);
        }
    }
}

fn animate_bursts(
    mut commands: Commands,
    time: Res<Time>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut bursts: Query<(Entity, &mut Burst, &mut Transform)>,
) {
    for (e, mut b, mut tf) in &mut bursts {
        b.t += time.delta_secs();
        let k = b.t / b.life;
        if k >= 1.0 {
            commands.entity(e).despawn();
            continue;
        }
        let grow = 1.0 - (1.0 - k).powi(3);
        tf.scale = Vec3::new(b.size * (0.2 + grow), b.size * (0.2 + grow) * 0.35, b.size * (0.2 + grow));
        if let Some(mut m) = materials.get_mut(&b.mat) {
            m.base_color.set_alpha(b.alpha * (1.0 - k));
        }
    }
}

/// Where a ball held in `hand` (0 left, 1 right) sits, from the wrist's world transform:
/// in the palm, which faces the body's centre line.
pub fn palm_ball(wrist: &GlobalTransform, hand: usize) -> Vec3 {
    let inward = if hand == 0 { 1.0 } else { -1.0 };
    wrist.transform_point(Vec3::new(inward * BALL_RADIUS * 0.85, -0.09, 0.0))
}

/// Put every held ball in its holder's hand (both hands while cradling a catch).
fn place_held_balls(
    view: Res<View>,
    rigs: Query<&Rig>,
    joints: Query<&GlobalTransform, Without<BallVis>>,
    mut balls: Query<(&BallVis, &mut GlobalTransform)>,
) {
    let Some(snap) = view.snap.as_ref() else { return };
    for (vis, mut gt) in &mut balls {
        let Some(b) = snap.balls.get(vis.index) else { continue };
        let BallState::Held { by } = b.state else { continue };
        let owner = if Some(by) == view.local { None } else { Some(by) };
        let Some(rig) = rigs.iter().find(|r| r.player == owner) else { continue };
        let (Ok(l), Ok(r)) = (joints.get(rig.wrist(0)), joints.get(rig.wrist(1))) else { continue };
        let at = if rig.cradling() {
            (l.translation() + r.translation()) * 0.5
        } else {
            palm_ball(r, 1)
        };
        let (scale, rot, _) = gt.to_scale_rotation_translation();
        *gt = GlobalTransform::from(Transform { translation: at, rotation: rot, scale });
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flying_balls_stretch_along_the_path_and_keep_their_volume() {
        let s = fluid_scale(40.0, 0.0, 0.0);
        assert!(s.z > 1.2 && s.x < 1.0 && (s.x - s.y).abs() < 1e-6);
        assert!((s.x * s.y * s.z - 1.0).abs() < 1e-4, "volume kept");
        assert_eq!(fluid_scale(0.0, 0.0, 0.0), Vec3::ONE, "round when slow and still");
        // Jiggle squashes one way then the other.
        let a = fluid_scale(20.0, 1.0, std::f32::consts::FRAC_PI_2);
        let b = fluid_scale(20.0, 1.0, -std::f32::consts::FRAC_PI_2);
        assert!(a.x > a.y && b.x < b.y);
        assert!(fluid_scale(1000.0, 0.0, 0.0).z <= 1.28 + 1e-5, "stretch is capped");
    }

    #[test]
    fn palm_ball_sits_inside_each_hand() {
        let wrist = GlobalTransform::from(Transform::from_xyz(1.0, 1.0, 0.0));
        let right = palm_ball(&wrist, 1);
        let left = palm_ball(&wrist, 0);
        assert!(right.x < 1.0 && left.x > 1.0, "toward the body's centre");
        assert!(right.y < 1.0);
    }
}
