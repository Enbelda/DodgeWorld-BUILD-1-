//! The sewer under the arena (its walls, rails, water, lights and finish line), the arena
//! floor's collider, and the hole that opens in the middle of the floor for the race.
//!
//! The sewer is always there, 30 m down; only the hole changes. The floor's collider is
//! one slab normally and four slabs round a square gap while the hole is open, with a
//! shaft below it so a falling player can't drift onto the sewer's ceiling.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::player::LocalArena;
use crate::sim::layout::{Aabb, RAIL_HALF};
use crate::sim::sewer::{self, HOLE_HALF, SEWER_CEILING, SEWER_FLOOR};
use crate::textures::{tiled_box, Tex, Textures};

pub struct SewerPlugin;

impl Plugin for SewerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_sewer)
            .add_systems(Update, floor_and_hole.after(crate::session::SessionSet));
    }
}

/// The arena floor's collider and the hole's look (rebuilt when either changes).
#[derive(Component)]
struct FloorPiece;

/// The beam of light over the open hole (pulses).
#[derive(Component)]
struct HoleBeam;

fn textured(m: &mut Assets<StandardMaterial>, color: Color, t: &Tex, rough: f32) -> Handle<StandardMaterial> {
    m.add(StandardMaterial {
        base_color: color,
        base_color_texture: Some(t.color.clone()),
        normal_map_texture: Some(t.normal.clone()),
        perceptual_roughness: rough,
        ..default()
    })
}

fn collider(commands: &mut Commands, b: &Aabb) {
    commands.spawn((Transform::from_translation(b.center()), Collider::cuboid(b.half().x, b.half().y, b.half().z)));
}

fn spawn_sewer(
    mut commands: Commands,
    textures: Res<Textures>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let t = &*textures;
    let m = &mut *materials;
    let floor_mat = textured(m, Color::srgb(0.32, 0.36, 0.33), &t.concrete, 0.8);
    let wall_mat = textured(m, Color::srgb(0.42, 0.47, 0.40), &t.brick, 0.85);
    let block_mat = textured(m, Color::srgb(0.45, 0.46, 0.44), &t.concrete, 0.8);
    let ceiling_mat = textured(m, Color::srgb(0.22, 0.25, 0.22), &t.concrete, 0.9);
    let chrome = m.add(StandardMaterial { base_color: Color::srgb(0.8, 0.82, 0.85), metallic: 1.0, perceptual_roughness: 0.2, ..default() });
    let pipe_mat = m.add(StandardMaterial { base_color: Color::srgb(0.45, 0.3, 0.18), metallic: 0.6, perceptual_roughness: 0.45, ..default() });
    let water = m.add(StandardMaterial {
        base_color: Color::srgba(0.25, 0.55, 0.25, 0.8),
        emissive: LinearRgba::rgb(0.15, 0.5, 0.12),
        perceptual_roughness: 0.05,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let arrow = m.add(StandardMaterial { base_color: Color::srgb(1.0, 0.8, 0.1), emissive: LinearRgba::rgb(3.0, 2.2, 0.2), unlit: true, ..default() });
    let bulb = m.add(StandardMaterial { base_color: Color::srgb(1.0, 0.95, 0.7), emissive: LinearRgba::rgb(8.0, 7.0, 3.5), unlit: true, ..default() });
    let check = [
        m.add(StandardMaterial { base_color: Color::WHITE, emissive: LinearRgba::rgb(1.2, 1.2, 1.2), ..default() }),
        m.add(StandardMaterial { base_color: Color::srgb(0.05, 0.05, 0.05), ..default() }),
    ];

    // Floor slab.
    let f = sewer::floor();
    collider(&mut commands, &f);
    commands.spawn((Mesh3d(meshes.add(tiled_box(f.half() * 2.0, 3.0))), MeshMaterial3d(floor_mat), Transform::from_translation(f.center())));

    // Walls, obstacles and rail bars.
    for b in sewer::solids() {
        collider(&mut commands, &b);
        let size = b.half() * 2.0;
        if b.half().x.min(b.half().z) <= RAIL_HALF + 1e-4 {
            // A chrome bar on posts.
            let long_x = size.x > size.z;
            let len = size.x.max(size.z);
            let bar = if long_x { Vec3::new(len, 0.08, 0.08) } else { Vec3::new(0.08, 0.08, len) };
            commands.spawn((Mesh3d(meshes.add(Cuboid::from_size(bar))), MeshMaterial3d(chrome.clone()), Transform::from_translation(b.center().with_y(b.max.y - 0.04))));
            let posts = (len / 2.0).round().max(2.0) as i32;
            for k in 0..=posts {
                let off = -len / 2.0 + len * k as f32 / posts as f32;
                let at = b.center() + if long_x { Vec3::X * off } else { Vec3::Z * off };
                commands.spawn((
                    Mesh3d(meshes.add(Cuboid::new(0.06, size.y, 0.06))),
                    MeshMaterial3d(chrome.clone()),
                    Transform::from_translation(at.with_y(b.center().y)),
                ));
            }
            continue;
        }
        let tall = b.max.y > SEWER_FLOOR + 3.0;
        let mat = if tall { wall_mat.clone() } else { block_mat.clone() };
        commands.spawn((Mesh3d(meshes.add(tiled_box(size, if tall { 1.6 } else { 1.0 }))), MeshMaterial3d(mat), Transform::from_translation(b.center())));
    }

    // Ceiling: four slabs round the shaft (no colliders; nothing reaches it but the shaft).
    let (x0, x1, z0, z1) = (sewer::X_MIN - 1.0, sewer::X_MAX + 1.0, sewer::Z_MIN - 1.0, sewer::Z_MAX + 1.0);
    let g = HOLE_HALF + 0.3;
    let slabs = [
        (x0, -g, z0, z1),
        (g, x1, z0, z1),
        (-g, g, z0, -g),
        (-g, g, g, z1),
    ];
    for (a, b, c, d) in slabs {
        let size = Vec3::new(b - a, 0.6, d - c);
        let at = Vec3::new((a + b) / 2.0, SEWER_CEILING + 0.3, (c + d) / 2.0);
        commands.spawn((Mesh3d(meshes.add(tiled_box(size, 3.0))), MeshMaterial3d(ceiling_mat.clone()), Transform::from_translation(at)));
    }
    // The shaft from the arena floor down to the ceiling: four walls, solid.
    let shaft_h = -SEWER_CEILING;
    let y = SEWER_CEILING / 2.0;
    for (cx, cz, hx, hz) in [(g + 0.2, 0.0, 0.2, g + 0.4), (-g - 0.2, 0.0, 0.2, g + 0.4), (0.0, g + 0.2, g, 0.2), (0.0, -g - 0.2, g, 0.2)] {
        let b = Aabb::from_center(Vec3::new(cx, y - 0.2, cz), Vec3::new(hx, shaft_h / 2.0 - 0.2, hz));
        collider(&mut commands, &b);
        commands.spawn((Mesh3d(meshes.add(tiled_box(b.half() * 2.0, 1.6))), MeshMaterial3d(wall_mat.clone()), Transform::from_translation(b.center())));
    }

    // Water channels along each lane's south edge, and pipes along the walls.
    let lane_len = sewer::X_MAX - sewer::X_MIN;
    let mid_x = (sewer::X_MIN + sewer::X_MAX) / 2.0;
    for k in 0..sewer::LANES {
        let z = k as f32 * sewer::LANE_W;
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(lane_len, 0.02, 0.9))),
            MeshMaterial3d(water.clone()),
            Transform::from_xyz(mid_x, SEWER_FLOOR + 0.012, z - 5.4),
        ));
        commands.spawn((
            Mesh3d(meshes.add(Cylinder::new(0.3, lane_len).mesh().resolution(16))),
            MeshMaterial3d(pipe_mat.clone()),
            Transform::from_xyz(mid_x, SEWER_CEILING - 1.6, z + 5.5).with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
        ));
        // Chevrons on the floor pointing the way to go.
        let dir = if k % 2 == 0 { 1.0 } else { -1.0 };
        let mut x = 6.0;
        while x < sewer::X_MAX - 6.0 {
            for side in [-1.0_f32, 1.0] {
                commands.spawn((
                    Mesh3d(meshes.add(Cuboid::new(1.4, 0.02, 0.18))),
                    MeshMaterial3d(arrow.clone()),
                    Transform::from_xyz(x, SEWER_FLOOR + 0.015, z + side * 0.45)
                        .with_rotation(Quat::from_rotation_y(-side * dir * 0.7)),
                ));
            }
            x += 10.0;
        }
    }

    // Lamps: a bulb on the ceiling and a point light under it.
    for p in sewer::lights() {
        commands.spawn((Mesh3d(meshes.add(Sphere::new(0.25))), MeshMaterial3d(bulb.clone()), Transform::from_translation(p + Vec3::Y * 0.6)));
        commands.spawn((
            PointLight { intensity: 900_000.0, range: 24.0, color: Color::srgb(1.0, 0.92, 0.7), shadow_maps_enabled: false, ..default() },
            Transform::from_translation(p),
        ));
    }
    // A light down the shaft, so the landing is lit.
    commands.spawn((
        PointLight { intensity: 600_000.0, range: 30.0, color: Color::srgb(0.7, 1.0, 0.7), shadow_maps_enabled: false, ..default() },
        Transform::from_xyz(0.0, SEWER_CEILING - 1.0, 0.0),
    ));

    // Finish line: a chequered strip across the last lane and a banner over it.
    let last_z = (sewer::LANES - 1) as f32 * sewer::LANE_W;
    let cells = 12;
    let w = sewer::LANE_W / cells as f32;
    for i in 0..cells {
        for j in 0..2 {
            let z = last_z - sewer::LANE_W / 2.0 + w * (i as f32 + 0.5);
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(w, 0.02, w))),
                MeshMaterial3d(check[(i + j) % 2].clone()),
                Transform::from_xyz(sewer::FINISH_X + w * (j as f32 + 0.5), SEWER_FLOOR + 0.014, z),
            ));
        }
    }
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.3, 1.2, sewer::LANE_W - 1.0))),
        MeshMaterial3d(check[0].clone()),
        Transform::from_xyz(sewer::FINISH_X, SEWER_CEILING - 1.0, last_z),
    ));
}

/// Floor collider (with the hole while it's open) and the hole's look.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn floor_and_hole(
    mut commands: Commands,
    time: Res<Time>,
    arena: Res<LocalArena>,
    mut built: Local<Option<(u32, bool)>>,
    old: Query<Entity, With<FloorPiece>>,
    mut beam: Query<(&mut Transform, &MeshMaterial3d<StandardMaterial>), With<HoleBeam>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let t = time.elapsed_secs();
    for (mut tf, mat) in &mut beam {
        tf.scale = Vec3::new(1.0 + 0.08 * (t * 4.0).sin(), 1.0, 1.0 + 0.08 * (t * 4.0).sin());
        if let Some(mut m) = materials.get_mut(&mat.0) {
            m.base_color = Color::srgba(0.4, 1.0, 0.4, 0.12 + 0.05 * (t * 3.0).sin());
        }
    }
    let Some(id) = arena.arena else { return };
    let half = id.half();
    let key = (half.to_bits(), arena.hole);
    if *built == Some(key) {
        return;
    }
    *built = Some(key);
    for e in &old {
        commands.entity(e).despawn();
    }
    let e = half + 1.0;
    let slab = |commands: &mut Commands, x0: f32, x1: f32, z0: f32, z1: f32| {
        commands.spawn((
            FloorPiece,
            Transform::from_xyz((x0 + x1) / 2.0, -0.1, (z0 + z1) / 2.0),
            Collider::cuboid((x1 - x0) / 2.0, 0.1, (z1 - z0) / 2.0),
        ));
    };
    if !arena.hole {
        slab(&mut commands, -e, e, -e, e);
        return;
    }
    let h = HOLE_HALF;
    slab(&mut commands, -e, -h, -e, e);
    slab(&mut commands, h, e, -e, e);
    slab(&mut commands, -h, h, -e, -h);
    slab(&mut commands, -h, h, h, e);
    // The open manhole: black inside, an iron rim, the lid slid aside, and a beam of light.
    let black = materials.add(StandardMaterial { base_color: Color::BLACK, unlit: true, ..default() });
    let iron = materials.add(StandardMaterial { base_color: Color::srgb(0.22, 0.22, 0.24), metallic: 0.9, perceptual_roughness: 0.5, ..default() });
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    commands.spawn((FloorPiece, Mesh3d(meshes.add(Circle::new(h * 1.42).mesh().resolution(48))), MeshMaterial3d(black), Transform::from_xyz(0.0, 0.015, 0.0).with_rotation(flat)));
    commands.spawn((
        FloorPiece,
        Mesh3d(meshes.add(Annulus::new(h * 1.42, h * 1.42 + 0.35).mesh().resolution(48))),
        MeshMaterial3d(iron.clone()),
        Transform::from_xyz(0.0, 0.02, 0.0).with_rotation(flat),
    ));
    commands.spawn((
        FloorPiece,
        Mesh3d(meshes.add(Cylinder::new(1.1, 0.08).mesh().resolution(32))),
        MeshMaterial3d(iron),
        Transform::from_xyz(h * 1.42 + 1.6, 0.04, 0.6),
    ));
    let glow = materials.add(StandardMaterial {
        base_color: Color::srgba(0.4, 1.0, 0.4, 0.14),
        emissive: LinearRgba::rgb(0.6, 2.5, 0.6),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        ..default()
    });
    commands.spawn((
        FloorPiece,
        HoleBeam,
        Mesh3d(meshes.add(Cylinder::new(h * 1.3, 14.0).mesh().resolution(32))),
        MeshMaterial3d(glow),
        Transform::from_xyz(0.0, 7.0, 0.0),
        bevy::light::NotShadowCaster,
    ));
}
