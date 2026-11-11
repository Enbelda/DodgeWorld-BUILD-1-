//! Arena visuals and colliders, built from the same layout the simulation uses.
//! Two looks share the geometry: the day arena, and a night arena with stars, a moon
//! and street lights. The host picks one in the lobby; everyone follows the snapshot.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::sim::ball::BallKind;
use crate::session::View;
use crate::sim::layout::{self, ARENA_HALF, WALL_HEIGHT, WALL_THICKNESS};
use crate::sim::MapId;

const DAY_SKY: Color = Color::srgb(0.53, 0.75, 0.92);
const NIGHT_SKY: Color = Color::srgb(0.015, 0.02, 0.06);

/// Shown only on the day map.
#[derive(Component)]
struct DayOnly;

/// Shown only on the night map.
#[derive(Component)]
struct NightOnly;

pub struct ArenaPlugin;

impl Plugin for ArenaPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (spawn_arena, spawn_night_props))
            .add_systems(Update, apply_map.after(crate::session::SessionSet));
    }
}

fn spawn_arena(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let red_half = materials.add(Color::srgb(0.42, 0.33, 0.33));
    let blue_half = materials.add(Color::srgb(0.31, 0.35, 0.44));
    let wall = materials.add(StandardMaterial {
        base_color: Color::srgba(0.85, 0.50, 0.25, 0.35),
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let cover = materials.add(Color::srgb(0.85, 0.50, 0.25));
    let line = materials.add(Color::srgb(0.95, 0.95, 0.95));
    let pad_colors = [
        (BallKind::Normal, materials.add(Color::srgb(0.75, 0.75, 0.70))),
        (BallKind::Boomerang, materials.add(Color::srgb(0.62, 0.35, 0.9))),
        (BallKind::Slime, materials.add(Color::srgb(0.35, 0.85, 0.3))),
    ];

    // Floor: one collider, two tinted halves (Red at +Z, Blue at -Z).
    commands.spawn((Transform::from_xyz(0.0, -0.1, 0.0), Collider::cuboid(ARENA_HALF + 1.0, 0.1, ARENA_HALF + 1.0)));
    let half = meshes.add(Plane3d::default().mesh().size(ARENA_HALF * 2.0, ARENA_HALF));
    for (mat, z) in [(red_half, ARENA_HALF / 2.0), (blue_half, -ARENA_HALF / 2.0)] {
        commands.spawn((Mesh3d(half.clone()), MeshMaterial3d(mat), Transform::from_xyz(0.0, 0.0, z)));
    }
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(ARENA_HALF * 2.0, 0.02, 0.2))),
        MeshMaterial3d(line),
        Transform::from_xyz(0.0, 0.01, 0.0),
    ));
    let pad_mesh = meshes.add(Cylinder::new(0.6, 0.02));
    for (p, kind) in layout::ball_pads() {
        let mat = pad_colors.iter().find(|(k, _)| *k == kind).map(|(_, m)| m.clone()).unwrap_or_default();
        commands.spawn((Mesh3d(pad_mesh.clone()), MeshMaterial3d(mat), Transform::from_xyz(p.x, 0.01, p.z)));
    }

    let covers = layout::cover();
    for s in layout::solids() {
        let h = s.half();
        let is_cover = covers.contains(&s);
        let mat = if is_cover { cover.clone() } else { wall.clone() };
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(h.x * 2.0, h.y * 2.0, h.z * 2.0))),
            MeshMaterial3d(mat),
            Transform::from_translation(s.center()),
            Collider::cuboid(h.x, h.y, h.z),
        ));
    }
    // Invisible ceiling-high extension so players can't jump out over the glass walls.
    for s in layout::solids().iter().take(4) {
        let h = s.half();
        commands.spawn((
            Transform::from_translation(s.center() + Vec3::Y * WALL_HEIGHT),
            Collider::cuboid(h.x, h.y, h.z),
        ));
    }

    commands.spawn((
        DayOnly,
        DirectionalLight { illuminance: 10_000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_xyz(10.0, 20.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// Where the street lights stand: inside the glass walls, arms reaching over the field.
/// Returns (pole base, direction the arm points).
pub fn street_light_spots() -> Vec<(Vec3, Vec3)> {
    let edge = ARENA_HALF + WALL_THICKNESS / 2.0;
    let mut v = Vec::new();
    for z in [-14.0, 0.0, 14.0] {
        v.push((Vec3::new(edge, 0.0, z), Vec3::NEG_X));
        v.push((Vec3::new(-edge, 0.0, z), Vec3::X));
    }
    for x in [-10.0, 10.0] {
        v.push((Vec3::new(x, 0.0, edge), Vec3::NEG_Z));
        v.push((Vec3::new(x, 0.0, -edge), Vec3::Z));
    }
    v
}

/// Star directions on the upper sky: a fixed pseudo-random spread so every client sees
/// the same sky.
pub fn star_directions(count: usize) -> Vec<Vec3> {
    let mut seed: u32 = 0x1234_5678;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32
    };
    (0..count)
        .map(|_| {
            let yaw = next() * std::f32::consts::TAU;
            // Keep stars above the walls' horizon.
            let elevation = 0.12 + next() * 1.4;
            Vec3::new(yaw.cos() * elevation.cos(), elevation.sin(), yaw.sin() * elevation.cos())
        })
        .collect()
}

fn spawn_night_props(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Moonlight: dim, cool, still casts shadows.
    commands.spawn((
        NightOnly,
        DirectionalLight {
            illuminance: 350.0,
            color: Color::srgb(0.6, 0.7, 1.0),
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-12.0, 25.0, -6.0).looking_at(Vec3::ZERO, Vec3::Y),
        Visibility::Hidden,
    ));
    let glow = |m: &mut Assets<StandardMaterial>, c: Color, e: LinearRgba| {
        m.add(StandardMaterial { base_color: c, emissive: e, unlit: true, ..default() })
    };
    // The moon and stars, far out on the sky.
    commands.spawn((
        NightOnly,
        Mesh3d(meshes.add(Sphere::new(14.0))),
        MeshMaterial3d(glow(&mut materials, Color::srgb(0.95, 0.95, 0.85), LinearRgba::rgb(3.0, 3.0, 2.6))),
        Transform::from_translation(Vec3::new(-0.45, 0.6, -0.65).normalize() * 400.0),
        Visibility::Hidden,
    ));
    let star = meshes.add(Sphere::new(1.0).mesh().ico(1).unwrap_or_else(|_| Sphere::new(1.0).into()));
    let star_mats = [
        glow(&mut materials, Color::WHITE, LinearRgba::rgb(4.0, 4.0, 4.0)),
        glow(&mut materials, Color::srgb(0.8, 0.85, 1.0), LinearRgba::rgb(2.0, 2.4, 3.5)),
        glow(&mut materials, Color::srgb(1.0, 0.9, 0.75), LinearRgba::rgb(3.0, 2.5, 1.8)),
    ];
    for (i, dir) in star_directions(450).into_iter().enumerate() {
        let size = 0.6 + (i % 5) as f32 * 0.25;
        commands.spawn((
            NightOnly,
            Mesh3d(star.clone()),
            MeshMaterial3d(star_mats[i % star_mats.len()].clone()),
            Transform::from_translation(dir * 450.0).with_scale(Vec3::splat(size)),
            Visibility::Hidden,
        ));
    }

    // Street lights: a dark pole, an arm over the field, a glowing lamp with a warm light.
    let metal = materials.add(StandardMaterial { base_color: Color::srgb(0.12, 0.12, 0.14), metallic: 0.6, ..default() });
    let lamp = glow(&mut materials, Color::srgb(1.0, 0.85, 0.55), LinearRgba::rgb(12.0, 9.0, 5.0));
    let pole_h = WALL_HEIGHT + 1.0;
    let pole = meshes.add(Cylinder::new(0.12, pole_h));
    let arm = meshes.add(Cuboid::new(0.1, 0.1, 2.2));
    let head = meshes.add(Cuboid::new(0.45, 0.18, 0.6));
    for (base, inward) in street_light_spots() {
        let yaw = crate::sim::bot::yaw_toward(inward);
        let root = commands
            .spawn((
                NightOnly,
                Transform::from_translation(base).with_rotation(Quat::from_rotation_y(yaw)),
                Visibility::Hidden,
            ))
            .id();
        commands.spawn((Mesh3d(pole.clone()), MeshMaterial3d(metal.clone()), Transform::from_xyz(0.0, pole_h / 2.0, 0.0), ChildOf(root)));
        commands.spawn((Mesh3d(arm.clone()), MeshMaterial3d(metal.clone()), Transform::from_xyz(0.0, pole_h, -1.1), ChildOf(root)));
        commands.spawn((Mesh3d(head.clone()), MeshMaterial3d(lamp.clone()), Transform::from_xyz(0.0, pole_h - 0.12, -2.1), ChildOf(root)));
        commands.spawn((
            PointLight {
                color: Color::srgb(1.0, 0.82, 0.55),
                intensity: 900_000.0,
                range: 26.0,
                radius: 0.2,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_xyz(0.0, pole_h - 0.4, -2.1),
            ChildOf(root),
        ));
    }
}

/// Switch between the day and night looks when the host changes the map.
fn apply_map(
    view: Res<View>,
    mut current: Local<Option<MapId>>,
    mut clear: ResMut<ClearColor>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut day: Query<&mut Visibility, (With<DayOnly>, Without<NightOnly>)>,
    mut night: Query<&mut Visibility, (With<NightOnly>, Without<DayOnly>)>,
) {
    let map = view.snap.as_ref().map_or(MapId::Day, |s| s.map);
    if *current == Some(map) {
        return;
    }
    *current = Some(map);
    let is_night = map == MapId::Night;
    let (on, off) = (Visibility::Inherited, Visibility::Hidden);
    for mut v in &mut day {
        *v = if is_night { off } else { on };
    }
    for mut v in &mut night {
        *v = if is_night { on } else { off };
    }
    clear.0 = if is_night { NIGHT_SKY } else { DAY_SKY };
    if is_night {
        ambient.color = Color::srgb(0.5, 0.6, 1.0);
        ambient.brightness = 25.0;
    } else {
        *ambient = GlobalAmbientLight::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn street_lights_stand_inside_the_walls_and_point_inward() {
        for (base, inward) in street_light_spots() {
            // In the wall's thickness, not on the playing field.
            assert!(base.x.abs() > ARENA_HALF || base.z.abs() > ARENA_HALF, "{base}");
            // The arm points back toward the middle.
            assert!(inward.dot(-base) > 0.0);
        }
        assert_eq!(street_light_spots().len(), 10);
    }

    #[test]
    fn stars_are_above_the_horizon_and_stable() {
        let a = star_directions(100);
        assert!(a.iter().all(|d| d.y > 0.1 && (d.length() - 1.0).abs() < 1e-4));
        assert_eq!(a, star_directions(100), "same sky on every client");
    }
}
