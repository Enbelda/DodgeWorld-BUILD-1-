//! Arena visuals and colliders, built from the same layout the simulation uses.
//! Two arenas (classic and the twice-as-big grand arena) and two looks (day, and night with
//! stars, a moon and street lights). The host picks in the lobby; everyone follows the
//! snapshot, and the arena is rebuilt when it changes.
//!
//! Detail: a wooden court floor with painted lines and pad rings, brick low walls with
//! concrete caps, painted-metal parkour panels, concrete pillars and blocks (all textured
//! with normal maps), and glass outer walls in a metal frame.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::player::LocalArena;
use crate::session::View;
use crate::sim::ball::BallKind;
use crate::sim::layout::{self, Aabb, ArenaId, WALL_THICKNESS};
use crate::sim::MapId;
use crate::textures::{tiled_box, Tex, Textures};

const DAY_SKY: Color = Color::srgb(0.53, 0.75, 0.92);
const NIGHT_SKY: Color = Color::srgb(0.015, 0.02, 0.06);

/// Shown only on the day map.
#[derive(Component)]
struct DayOnly;

/// Shown only on the night map.
#[derive(Component)]
struct NightOnly;

/// Part of the current arena's geometry (despawned when the arena changes).
#[derive(Component)]
struct ArenaGeo;

pub struct ArenaPlugin;

impl Plugin for ArenaPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (spawn_lights, spawn_sky))
            .add_systems(Update, (build_arena, apply_map).chain().after(crate::session::SessionSet));
    }
}

/// What a box is, for its look.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BoxKind {
    /// One of the four glass boundary walls.
    Boundary,
    /// Low wall (up to 2 m, long and thin): brick with a concrete cap.
    LowWall,
    /// Tall thin panel (parkour): painted metal.
    Panel,
    /// Pillars, blocks, crates and bunkers: concrete.
    Block,
    /// A free-standing grind rail: a metal bar on posts.
    Rail,
}

/// Classify the n-th solid of an arena.
pub fn box_kind(index: usize, b: &Aabb) -> BoxKind {
    let h = b.half();
    let thin = h.x.min(h.z) <= 0.5;
    let long = h.x.max(h.z) >= 2.0;
    if index < 4 {
        BoxKind::Boundary
    } else if h.x.min(h.z) <= layout::RAIL_HALF + 1e-4 {
        BoxKind::Rail
    } else if thin && long && b.max.y <= 2.0 {
        BoxKind::LowWall
    } else if thin && b.max.y > 3.5 {
        BoxKind::Panel
    } else {
        BoxKind::Block
    }
}

fn textured(m: &mut Assets<StandardMaterial>, color: Color, t: &Tex, rough: f32) -> Handle<StandardMaterial> {
    m.add(StandardMaterial {
        base_color: color,
        base_color_texture: Some(t.color.clone()),
        normal_map_texture: Some(t.normal.clone()),
        perceptual_roughness: rough,
        ..default()
    })
}

fn spawn_lights(mut commands: Commands) {
    commands.spawn((
        DayOnly,
        DirectionalLight { illuminance: 10_000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_xyz(10.0, 20.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
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
}

/// Rebuild floor, walls, cover, pads and street lights when the arena changes.
#[allow(clippy::too_many_arguments)]
fn build_arena(
    mut commands: Commands,
    arena: Res<LocalArena>,
    textures: Res<Textures>,
    mut built: Local<Option<ArenaId>>,
    old: Query<Entity, With<ArenaGeo>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(id) = arena.arena else { return };
    if *built == Some(id) {
        return;
    }
    *built = Some(id);
    for e in &old {
        commands.entity(e).despawn();
    }
    let half = id.half();
    let wall_h = id.wall_height();
    let t = &*textures;
    let m = &mut *materials;

    // Floor: one collider, a wooden court tinted per team half, painted lines on top.
    let floor_red = textured(m, Color::srgb(0.86, 0.62, 0.50), &t.court, 0.55);
    let floor_blue = textured(m, Color::srgb(0.58, 0.66, 0.84), &t.court, 0.55);
    let paint = m.add(StandardMaterial { base_color: Color::srgb(0.96, 0.96, 0.94), perceptual_roughness: 0.4, ..default() });
    commands.spawn((ArenaGeo, Transform::from_xyz(0.0, -0.1, 0.0), Collider::cuboid(half + 1.0, 0.1, half + 1.0)));
    let board = meshes.add(tiled_box(Vec3::new(half * 2.0, 0.1, half), 4.0));
    for (mat, z) in [(floor_red, half / 2.0), (floor_blue, -half / 2.0)] {
        commands.spawn((ArenaGeo, Mesh3d(board.clone()), MeshMaterial3d(mat), Transform::from_xyz(0.0, -0.05, z)));
    }
    let line = |w: f32, d: f32| Cuboid::new(w, 0.012, d);
    commands.spawn((ArenaGeo, Mesh3d(meshes.add(line(half * 2.0, 0.2))), MeshMaterial3d(paint.clone()), Transform::from_xyz(0.0, 0.006, 0.0)));
    // Boundary line inset from the walls, and attack lines at a third of each half.
    for z in [half - 0.6, -(half - 0.6), half / 3.0, -half / 3.0] {
        commands.spawn((ArenaGeo, Mesh3d(meshes.add(line(half * 2.0 - 1.2, 0.1))), MeshMaterial3d(paint.clone()), Transform::from_xyz(0.0, 0.006, z)));
    }
    for x in [half - 0.6, -(half - 0.6)] {
        commands.spawn((ArenaGeo, Mesh3d(meshes.add(line(0.1, half * 2.0 - 1.2))), MeshMaterial3d(paint.clone()), Transform::from_xyz(x, 0.006, 0.0)));
    }
    // Centre circle.
    commands.spawn((
        ArenaGeo,
        Mesh3d(meshes.add(Annulus::new(half * 0.15 - 0.1, half * 0.15).mesh().resolution(64))),
        MeshMaterial3d(paint.clone()),
        Transform::from_xyz(0.0, 0.007, 0.0).with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
    ));

    // Ball pads: a coloured disc with a white ring.
    let pad_colors = [
        (BallKind::Normal, Color::srgb(0.75, 0.75, 0.70)),
        (BallKind::Boomerang, Color::srgb(0.62, 0.35, 0.9)),
        (BallKind::Slime, Color::srgb(0.35, 0.85, 0.3)),
        (BallKind::Brick, Color::srgb(0.75, 0.18, 0.12)),
    ];
    let pad_mesh = meshes.add(Cylinder::new(0.6, 0.02).mesh().resolution(32));
    let ring_mesh = meshes.add(Annulus::new(0.62, 0.72).mesh().resolution(32));
    for (p, kind) in layout::ball_pads(id) {
        let c = pad_colors.iter().find(|(k, _)| *k == kind).map_or(Color::WHITE, |(_, c)| *c);
        let mat = m.add(StandardMaterial { base_color: c, perceptual_roughness: 0.5, ..default() });
        commands.spawn((ArenaGeo, Mesh3d(pad_mesh.clone()), MeshMaterial3d(mat), Transform::from_xyz(p.x, 0.012, p.z)));
        commands.spawn((
            ArenaGeo,
            Mesh3d(ring_mesh.clone()),
            MeshMaterial3d(paint.clone()),
            Transform::from_xyz(p.x, 0.014, p.z).with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        ));
    }

    // Walls and cover.
    let glass = m.add(StandardMaterial {
        base_color: Color::srgba(0.75, 0.88, 0.95, 0.18),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.05,
        reflectance: 0.9,
        ..default()
    });
    let frame = textured(m, Color::srgb(0.22, 0.24, 0.28), &t.panel, 0.35);
    let brick = textured(m, Color::srgb(0.78, 0.42, 0.28), &t.brick, 0.85);
    let cap = textured(m, Color::srgb(0.62, 0.62, 0.6), &t.concrete, 0.8);
    let concrete = textured(m, Color::srgb(0.86, 0.80, 0.70), &t.concrete, 0.9);
    let panel = [
        textured(m, Color::srgb(0.95, 0.55, 0.2), &t.panel, 0.45),
        textured(m, Color::srgb(0.25, 0.65, 0.7), &t.panel, 0.45),
    ];
    let stripe = m.add(StandardMaterial { base_color: Color::srgb(0.1, 0.1, 0.12), perceptual_roughness: 0.6, ..default() });
    let chrome = m.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.87, 0.9),
        metallic: 1.0,
        perceptual_roughness: 0.18,
        ..default()
    });
    let post = m.add(StandardMaterial { base_color: Color::srgb(0.95, 0.75, 0.1), metallic: 0.3, perceptual_roughness: 0.5, ..default() });
    for (i, s) in layout::solids(id).into_iter().enumerate() {
        let size = s.half() * 2.0;
        let kind = box_kind(i, &s);
        commands.spawn((ArenaGeo, Transform::from_translation(s.center()), Collider::cuboid(s.half().x, s.half().y, s.half().z)));
        match kind {
            BoxKind::Boundary => {
                commands.spawn((ArenaGeo, Mesh3d(meshes.add(Cuboid::from_size(size))), MeshMaterial3d(glass.clone()), Transform::from_translation(s.center())));
                // Frame: posts every ~5 m along the wall, a top rail and a kick plate.
                let long_x = size.x > size.z;
                let len = size.x.max(size.z);
                let posts = (len / 5.0).round().max(2.0) as i32;
                for k in 0..=posts {
                    let off = -len / 2.0 + len * k as f32 / posts as f32;
                    let at = s.center() + if long_x { Vec3::X * off } else { Vec3::Z * off };
                    commands.spawn((
                        ArenaGeo,
                        Mesh3d(meshes.add(tiled_box(Vec3::new(0.25, wall_h, 0.25), 1.0))),
                        MeshMaterial3d(frame.clone()),
                        Transform::from_translation(Vec3::new(at.x, wall_h / 2.0, at.z)),
                    ));
                }
                let rail = if long_x { Vec3::new(size.x, 0.2, size.z + 0.1) } else { Vec3::new(size.x + 0.1, 0.2, size.z) };
                commands.spawn((ArenaGeo, Mesh3d(meshes.add(tiled_box(rail, 1.0))), MeshMaterial3d(frame.clone()), Transform::from_translation(s.center().with_y(wall_h))));
                let kick = if long_x { Vec3::new(size.x, 0.4, size.z + 0.06) } else { Vec3::new(size.x + 0.06, 0.4, size.z) };
                commands.spawn((ArenaGeo, Mesh3d(meshes.add(tiled_box(kick, 1.0))), MeshMaterial3d(frame.clone()), Transform::from_translation(s.center().with_y(0.2))));
                // Invisible extension so players can't jump out over the glass.
                commands.spawn((ArenaGeo, Transform::from_translation(s.center() + Vec3::Y * wall_h), Collider::cuboid(s.half().x, s.half().y, s.half().z)));
            }
            BoxKind::LowWall => {
                let body = Vec3::new(size.x, size.y - 0.1, size.z);
                commands.spawn((
                    ArenaGeo,
                    Mesh3d(meshes.add(tiled_box(body, 1.6))),
                    MeshMaterial3d(brick.clone()),
                    Transform::from_translation(s.center() - Vec3::Y * 0.05),
                ));
                // A slightly wider concrete cap along the top.
                commands.spawn((
                    ArenaGeo,
                    Mesh3d(meshes.add(tiled_box(Vec3::new(size.x + 0.08, 0.12, size.z + 0.08), 1.5))),
                    MeshMaterial3d(cap.clone()),
                    Transform::from_translation(Vec3::new(s.center().x, s.max.y - 0.06, s.center().z)),
                ));
            }
            BoxKind::Panel => {
                let mat = panel[(s.center().x + s.center().z > 0.0) as usize].clone();
                commands.spawn((ArenaGeo, Mesh3d(meshes.add(tiled_box(size, 2.75))), MeshMaterial3d(mat), Transform::from_translation(s.center())));
                // Dark hazard stripe near the top shows where to grab.
                commands.spawn((
                    ArenaGeo,
                    Mesh3d(meshes.add(Cuboid::new(size.x + 0.02, 0.18, size.z + 0.02))),
                    MeshMaterial3d(stripe.clone()),
                    Transform::from_translation(Vec3::new(s.center().x, s.max.y - 0.15, s.center().z)),
                ));
            }
            BoxKind::Rail => {
                // Round bar along the top, yellow posts every ~2 m down to the floor.
                let long_x = size.x > size.z;
                let len = size.x.max(size.z);
                let bar = Cylinder::new(0.045, len).mesh().resolution(16);
                let rot = if long_x { Quat::from_rotation_z(std::f32::consts::FRAC_PI_2) } else { Quat::from_rotation_x(std::f32::consts::FRAC_PI_2) };
                commands.spawn((
                    ArenaGeo,
                    Mesh3d(meshes.add(bar)),
                    MeshMaterial3d(chrome.clone()),
                    Transform::from_translation(Vec3::new(s.center().x, s.max.y - 0.045, s.center().z)).with_rotation(rot),
                ));
                let posts = (len / 2.0).ceil().max(1.0) as i32;
                for k in 0..=posts {
                    let off = -len / 2.0 + len * k as f32 / posts as f32;
                    let at = s.center() + if long_x { Vec3::X * off } else { Vec3::Z * off };
                    commands.spawn((
                        ArenaGeo,
                        Mesh3d(meshes.add(Cylinder::new(0.035, s.max.y).mesh().resolution(10))),
                        MeshMaterial3d(post.clone()),
                        Transform::from_translation(Vec3::new(at.x, s.max.y / 2.0, at.z)),
                    ));
                }
            }
            BoxKind::Block => {
                commands.spawn((ArenaGeo, Mesh3d(meshes.add(tiled_box(size, 2.0))), MeshMaterial3d(concrete.clone()), Transform::from_translation(s.center())));
                // Concrete cap lip.
                commands.spawn((
                    ArenaGeo,
                    Mesh3d(meshes.add(tiled_box(Vec3::new(size.x + 0.1, 0.1, size.z + 0.1), 1.5))),
                    MeshMaterial3d(cap.clone()),
                    Transform::from_translation(Vec3::new(s.center().x, s.max.y - 0.05, s.center().z)),
                ));
            }
        }
    }

    spawn_street_lights(&mut commands, &mut meshes, m, id);
}

/// Where the street lights stand: inside the glass walls, arms reaching over the field.
/// Returns (pole base, direction the arm points).
pub fn street_light_spots(arena: ArenaId) -> Vec<(Vec3, Vec3)> {
    let h = arena.half();
    let edge = h + WALL_THICKNESS / 2.0;
    let mut v = Vec::new();
    for z in [-0.7 * h, 0.0, 0.7 * h] {
        v.push((Vec3::new(edge, 0.0, z), Vec3::NEG_X));
        v.push((Vec3::new(-edge, 0.0, z), Vec3::X));
    }
    for x in [-0.5 * h, 0.5 * h] {
        v.push((Vec3::new(x, 0.0, edge), Vec3::NEG_Z));
        v.push((Vec3::new(x, 0.0, -edge), Vec3::Z));
    }
    v
}

fn spawn_street_lights(commands: &mut Commands, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, arena: ArenaId) {
    let metal = materials.add(StandardMaterial { base_color: Color::srgb(0.12, 0.12, 0.14), metallic: 0.6, ..default() });
    let lamp = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.85, 0.55),
        emissive: LinearRgba::rgb(12.0, 9.0, 5.0),
        unlit: true,
        ..default()
    });
    let pole_h = arena.wall_height() + 1.0;
    let range = arena.half() * 1.3;
    let pole = meshes.add(Cylinder::new(0.12, pole_h));
    let arm = meshes.add(Cuboid::new(0.1, 0.1, 2.2));
    let head = meshes.add(Cuboid::new(0.45, 0.18, 0.6));
    for (base, inward) in street_light_spots(arena) {
        let yaw = crate::sim::bot::yaw_toward(inward);
        let root = commands
            .spawn((
                ArenaGeo,
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
                intensity: 900_000.0 * (range / 26.0).powi(2),
                range,
                radius: 0.2,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_xyz(0.0, pole_h - 0.4, -2.1),
            ChildOf(root),
        ));
    }
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

fn spawn_sky(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
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
}

/// Switch between the day and night looks when the host changes the map (and after the
/// arena is rebuilt, so new street lights follow the current look).
#[allow(clippy::type_complexity)]
fn apply_map(
    view: Res<View>,
    arena: Res<LocalArena>,
    mut current: Local<Option<(MapId, Option<ArenaId>)>>,
    mut clear: ResMut<ClearColor>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut day: Query<&mut Visibility, (With<DayOnly>, Without<NightOnly>)>,
    mut night: Query<&mut Visibility, (With<NightOnly>, Without<DayOnly>)>,
) {
    let map = view.snap.as_ref().map_or(MapId::Day, |s| s.map);
    let key = (map, arena.arena);
    if *current == Some(key) {
        return;
    }
    *current = Some(key);
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
        for arena in [ArenaId::Classic, ArenaId::Grand] {
            let h = arena.half();
            for (base, inward) in street_light_spots(arena) {
                // In the wall's thickness, not on the playing field.
                assert!(base.x.abs() > h || base.z.abs() > h, "{base}");
                // The arm points back toward the middle.
                assert!(inward.dot(-base) > 0.0);
            }
            assert_eq!(street_light_spots(arena).len(), 10);
        }
    }

    #[test]
    fn stars_are_above_the_horizon_and_stable() {
        let a = star_directions(100);
        assert!(a.iter().all(|d| d.y > 0.1 && (d.length() - 1.0).abs() < 1e-4));
        assert_eq!(a, star_directions(100), "same sky on every client");
    }

    #[test]
    fn boxes_get_the_right_look() {
        for arena in [ArenaId::Classic, ArenaId::Grand] {
            let s = layout::solids(arena);
            let kinds: Vec<BoxKind> = s.iter().enumerate().map(|(i, b)| box_kind(i, b)).collect();
            assert!(kinds[..4].iter().all(|k| *k == BoxKind::Boundary));
            assert!(kinds.contains(&BoxKind::LowWall) && kinds.contains(&BoxKind::Panel) && kinds.contains(&BoxKind::Block));
            assert_eq!(kinds.iter().filter(|k| **k == BoxKind::Rail).count(), layout::rail_boxes(arena).len());
            for (k, b) in kinds.iter().zip(&s) {
                if *k == BoxKind::Panel {
                    assert!(b.max.y > 3.5);
                }
            }
        }
    }
}
