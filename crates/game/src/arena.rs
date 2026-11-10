//! Arena visuals and colliders, built from the same layout the simulation uses.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::sim::layout::{self, ARENA_HALF, WALL_HEIGHT};

pub struct ArenaPlugin;

impl Plugin for ArenaPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_arena);
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
    let pad = materials.add(Color::srgb(0.75, 0.75, 0.70));

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
    for p in layout::ball_pads() {
        commands.spawn((Mesh3d(pad_mesh.clone()), MeshMaterial3d(pad.clone()), Transform::from_xyz(p.x, 0.01, p.z)));
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
        DirectionalLight { illuminance: 10_000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_xyz(10.0, 20.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}
