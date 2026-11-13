//! Mount models (built from simple shapes) and their animation.
//!
//! Boards, the BMX and the scooter hang under the rider's hip pivot, so they go round with
//! front and back flips and spins; the trick data spins and flips them on top of that
//! (kickflip roll, shove-it spin, nose-over-tail, tailwhips round the head tube, barspins),
//! sets them on rails at the grind's angle and tilt, and lifts them to the rider's feet in
//! tucked tricks. Inline skates and soap shoes sit on the rider's ankles. Wheels roll with
//! the speed.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::prelude::*;

use crate::mounts::{Legs, MountKind, Stance, TrickCat};
use crate::player::LocalRide;
use crate::rig::Rig;
use crate::session::View;
use crate::textures::Textures;

/// Height of the hip pivot above the feet (matches the rig).
const PIVOT: f32 = 0.9;

/// A mount model attached to a rig.
pub struct MountAttach {
    pub kind: MountKind,
    /// The model's root (a child of the rig's hip pivot), or the left skate.
    pub root: Entity,
    /// Spinning handlebars (BMX, scooter).
    pub bars: Option<Entity>,
    /// Wheels and their radius, to roll them.
    pub wheels: Vec<(Entity, f32)>,
    /// The right skate (skates and soap shoes).
    pub other: Option<Entity>,
    /// Where the model turns for flips (above its ground point).
    pub center: Vec3,
    /// Tailwhip pivot (the head tube / stem), relative to the ground point.
    pub whip_pivot: Vec3,
    pub roll: f32,
}

#[derive(Resource)]
pub struct MountAssets {
    cube: Handle<Mesh>,
    cylinder: Handle<Mesh>,
    sphere: Handle<Mesh>,
    tire: Handle<Mesh>,
    wood: Handle<StandardMaterial>,
    grip: Handle<StandardMaterial>,
    metal: Handle<StandardMaterial>,
    urethane: [Handle<StandardMaterial>; 3],
    rubber: Handle<StandardMaterial>,
    frame: [Handle<StandardMaterial>; 2],
    boot: [Handle<StandardMaterial>; 2],
    plate: Handle<StandardMaterial>,
}

pub struct MountVisPlugin;

impl Plugin for MountVisPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, make_mount_assets).add_systems(
            Update,
            (
                sync_mount_models.before(crate::rig::animate_rigs),
                pose_mounts.after(crate::rig::animate_rigs),
            )
                .after(crate::session::SessionSet),
        );
    }
}

fn make_mount_assets(
    mut commands: Commands,
    textures: Res<Textures>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut mat = |c: Color, rough: f32, metal: f32| {
        materials.add(StandardMaterial { base_color: c, perceptual_roughness: rough, metallic: metal, ..default() })
    };
    let urethane = [mat(Color::srgb(0.95, 0.92, 0.8), 0.6, 0.0), mat(Color::srgb(0.95, 0.4, 0.2), 0.6, 0.0), mat(Color::srgb(0.3, 0.8, 0.9), 0.6, 0.0)];
    let metal = mat(Color::srgb(0.75, 0.76, 0.78), 0.3, 0.9);
    let rubber = mat(Color::srgb(0.08, 0.08, 0.09), 0.85, 0.0);
    let frame = [mat(Color::srgb(0.85, 0.15, 0.12), 0.35, 0.4), mat(Color::srgb(0.15, 0.35, 0.9), 0.35, 0.4)];
    let boot = [mat(Color::srgb(0.12, 0.12, 0.13), 0.5, 0.0), mat(Color::srgb(0.92, 0.92, 0.9), 0.5, 0.0)];
    let plate = mat(Color::srgb(0.35, 0.36, 0.4), 0.25, 0.2);
    let grip = mat(Color::srgb(0.06, 0.06, 0.07), 0.95, 0.0);
    let wood = materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.62, 0.38),
        base_color_texture: Some(textures.court.color.clone()),
        perceptual_roughness: 0.6,
        ..default()
    });
    commands.insert_resource(MountAssets {
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        cylinder: meshes.add(Cylinder::new(0.5, 1.0).mesh().resolution(16)),
        sphere: meshes.add(Sphere::new(0.5).mesh().uv(16, 10)),
        tire: meshes.add(Torus::new(0.86, 1.0).mesh().minor_resolution(10).major_resolution(32)),
        wood,
        grip,
        metal,
        urethane,
        rubber,
        frame,
        boot,
        plate,
    });
}

struct Build<'a, 'w, 's> {
    c: &'a mut Commands<'w, 's>,
    a: &'a MountAssets,
}

impl Build<'_, '_, '_> {
    fn part(&mut self, mesh: &Handle<Mesh>, mat: &Handle<StandardMaterial>, tf: Transform, parent: Entity) -> Entity {
        self.c.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), tf, ChildOf(parent))).id()
    }
    fn cube(&mut self, at: Vec3, size: Vec3, mat: &Handle<StandardMaterial>, parent: Entity) -> Entity {
        let m = self.a.cube.clone();
        self.part(&m, mat, Transform::from_translation(at).with_scale(size), parent)
    }
    /// A round wheel of `radius` and `width`, axle along X.
    fn wheel(&mut self, at: Vec3, radius: f32, width: f32, mat: &Handle<StandardMaterial>, parent: Entity) -> Entity {
        let m = self.a.cylinder.clone();
        let hub = self.c.spawn((Transform::from_translation(at), Visibility::default(), ChildOf(parent))).id();
        self.part(
            &m,
            mat,
            Transform::from_rotation(Quat::from_rotation_z(FRAC_PI_2)).with_scale(Vec3::new(radius * 2.0, width, radius * 2.0)),
            hub,
        );
        // A hub cap so the roll is visible.
        let cap = self.a.metal.clone();
        let c = self.a.cube.clone();
        self.part(&c, &cap, Transform::from_xyz(0.0, 0.0, 0.0).with_scale(Vec3::new(width * 1.05, radius * 1.2, radius * 0.25)), hub);
        hub
    }
    /// A tube from `a` to `b`.
    fn tube(&mut self, a: Vec3, b: Vec3, radius: f32, mat: &Handle<StandardMaterial>, parent: Entity) -> Entity {
        let m = self.a.cylinder.clone();
        let d = b - a;
        let tf = Transform::from_translation((a + b) * 0.5)
            .with_rotation(Quat::from_rotation_arc(Vec3::Y, d.normalize_or(Vec3::Y)))
            .with_scale(Vec3::new(radius * 2.0, d.length(), radius * 2.0));
        self.part(&m, mat, tf, parent)
    }
    /// A bike tyre (torus) of `radius`, axle along X.
    fn tyre(&mut self, at: Vec3, radius: f32, parent: Entity) -> Entity {
        let hub = self.c.spawn((Transform::from_translation(at), Visibility::default(), ChildOf(parent))).id();
        let tire = self.a.tire.clone();
        let rubber = self.a.rubber.clone();
        self.part(&tire, &rubber, Transform::from_rotation(Quat::from_rotation_z(FRAC_PI_2)).with_scale(Vec3::new(radius, radius * 0.6, radius)), hub);
        let metal = self.a.metal.clone();
        // Spokes.
        for k in 0..6 {
            let ang = k as f32 * TAU / 12.0;
            let dir = Vec3::new(0.0, ang.cos(), ang.sin());
            self.tube(-dir * radius * 0.85, dir * radius * 0.85, 0.006, &metal, hub);
        }
        let cyl = self.a.cylinder.clone();
        self.part(&cyl, &metal, Transform::from_rotation(Quat::from_rotation_z(FRAC_PI_2)).with_scale(Vec3::new(0.05, 0.1, 0.05)), hub);
        hub
    }
}

/// Build the model for `kind` on `rig`, team colour `team` (0 red, 1 blue).
fn spawn_model(commands: &mut Commands, a: &MountAssets, rig: &Rig, kind: MountKind, team: usize) -> MountAttach {
    let mut b = Build { c: commands, a };
    let root = b.c.spawn((Transform::default(), Visibility::default(), ChildOf(rig.flip))).id();
    let mut wheels = Vec::new();
    let mut bars = None;
    let other = None;
    let mut center = Vec3::new(0.0, 0.1, 0.0);
    let mut whip_pivot = Vec3::ZERO;
    let ure = a.urethane[match kind {
        MountKind::Longboard => 1,
        MountKind::Scooter => 2,
        _ => 0,
    }]
    .clone();
    match kind {
        MountKind::Skateboard | MountKind::Longboard => {
            let long = kind == MountKind::Longboard;
            let (len, width, y, axle, r) = if long { (1.0, 0.24, 0.13, 0.36, 0.037) } else { (0.62, 0.21, 0.1, 0.24, 0.028) };
            center = Vec3::new(0.0, y, 0.0);
            b.cube(Vec3::new(0.0, y, 0.0), Vec3::new(width, 0.02, len), &a.wood, root);
            b.cube(Vec3::new(0.0, y + 0.012, 0.0), Vec3::new(width - 0.01, 0.005, len - 0.02), &a.grip, root);
            // Kicked-up nose and tail (the longboard's are rounded off instead).
            for s in [-1.0_f32, 1.0] {
                if long {
                    let sph = a.sphere.clone();
                    b.part(&sph, &a.wood, Transform::from_xyz(0.0, y, s * len / 2.0).with_scale(Vec3::new(width, 0.02, 0.2)), root);
                } else {
                    let tf = Transform::from_xyz(0.0, y + 0.02, s * (len / 2.0 + 0.04))
                        .with_rotation(Quat::from_rotation_x(-0.4 * s))
                        .with_scale(Vec3::new(width, 0.02, 0.1));
                    let cube = a.cube.clone();
                    b.part(&cube, &a.wood, tf, root);
                }
                b.cube(Vec3::new(0.0, y - 0.03, s * axle), Vec3::new(width * 0.8, 0.035, 0.05), &a.metal, root);
                for x in [-1.0_f32, 1.0] {
                    wheels.push((b.wheel(Vec3::new(x * width * 0.42, r, s * axle), r, 0.03, &ure, root), r));
                }
            }
        }
        MountKind::Bmx => {
            let f = a.frame[team].clone();
            center = Vec3::new(0.0, 0.45, 0.0);
            whip_pivot = Vec3::new(0.0, 0.0, -0.4);
            let (front, rear) = (Vec3::new(0.0, 0.25, -0.5), Vec3::new(0.0, 0.25, 0.48));
            let crank = Vec3::new(0.0, 0.3, 0.02);
            let head_top = Vec3::new(0.0, 0.66, -0.4);
            let head_low = Vec3::new(0.0, 0.56, -0.42);
            let seat = Vec3::new(0.0, 0.66, 0.14);
            b.tube(head_top, seat, 0.02, &f, root);
            b.tube(head_low, crank, 0.022, &f, root);
            b.tube(crank, seat, 0.02, &f, root);
            for x in [-0.04_f32, 0.04] {
                b.tube(crank + Vec3::X * x, rear + Vec3::X * x, 0.012, &f, root);
                b.tube(seat + Vec3::X * x, rear + Vec3::X * x, 0.012, &f, root);
                b.tube(head_low + Vec3::X * x, front + Vec3::X * x, 0.014, &a.metal, root);
            }
            b.tube(head_low, head_top, 0.028, &f, root);
            b.cube(seat + Vec3::new(0.0, 0.05, 0.02), Vec3::new(0.09, 0.04, 0.22), &a.rubber, root);
            // Cranks, pedals and pegs.
            for x in [-1.0_f32, 1.0] {
                b.tube(crank, crank + Vec3::new(0.12 * x, -0.06 * x, 0.12 * x), 0.012, &a.metal, root);
                b.cube(crank + Vec3::new(0.16 * x, -0.06 * x, 0.12 * x), Vec3::new(0.08, 0.02, 0.07), &a.rubber, root);
                for axle in [front, rear] {
                    b.tube(axle + Vec3::X * 0.05 * x, axle + Vec3::X * 0.14 * x, 0.02, &a.metal, root);
                }
            }
            wheels.push((b.tyre(front, 0.25, root), 0.25));
            wheels.push((b.tyre(rear, 0.25, root), 0.25));
            // Bars spin about the stem.
            let bar_root = b.c.spawn((Transform::from_translation(head_top), Visibility::default(), ChildOf(root))).id();
            b.tube(Vec3::ZERO, Vec3::new(0.0, 0.14, -0.03), 0.016, &a.metal, bar_root);
            b.tube(Vec3::new(-0.3, 0.16, -0.03), Vec3::new(0.3, 0.16, -0.03), 0.014, &a.metal, bar_root);
            b.tube(Vec3::new(-0.12, 0.06, -0.03), Vec3::new(0.12, 0.06, -0.03), 0.01, &a.metal, bar_root);
            for x in [-0.27_f32, 0.27] {
                b.tube(Vec3::new(x, 0.16, -0.03), Vec3::new(x * 1.1, 0.16, -0.03), 0.02, &a.rubber, bar_root);
            }
            bars = Some(bar_root);
        }
        MountKind::Scooter => {
            center = Vec3::new(0.0, 0.25, 0.0);
            whip_pivot = Vec3::new(0.0, 0.0, -0.27);
            let f = a.frame[team].clone();
            b.cube(Vec3::new(0.0, 0.08, 0.0), Vec3::new(0.12, 0.03, 0.5), &f, root);
            b.cube(Vec3::new(0.0, 0.097, 0.0), Vec3::new(0.11, 0.005, 0.46), &a.grip, root);
            for z in [-0.27_f32, 0.27] {
                wheels.push((b.wheel(Vec3::new(0.0, 0.05, z), 0.05, 0.025, &ure, root), 0.05));
                b.tube(Vec3::new(-0.02, 0.05, z), Vec3::new(0.02, 0.05, z), 0.012, &a.metal, root);
            }
            let bar_root = b.c.spawn((Transform::from_xyz(0.0, 0.08, -0.27), Visibility::default(), ChildOf(root))).id();
            b.tube(Vec3::ZERO, Vec3::new(0.0, 0.8, -0.02), 0.018, &a.metal, bar_root);
            b.tube(Vec3::new(-0.26, 0.8, -0.02), Vec3::new(0.26, 0.8, -0.02), 0.014, &a.metal, bar_root);
            for x in [-0.24_f32, 0.24] {
                b.tube(Vec3::new(x, 0.8, -0.02), Vec3::new(x * 1.12, 0.8, -0.02), 0.02, &a.rubber, bar_root);
            }
            bars = Some(bar_root);
        }
        MountKind::InlineSkates | MountKind::SoapShoes => {
            // One per foot, on the ankle joints.
            let inline = kind == MountKind::InlineSkates;
            let mut skate = |b: &mut Build, ankle: Entity| -> Entity {
                let s = b.c.spawn((Transform::default(), Visibility::default(), ChildOf(ankle))).id();
                if inline {
                    b.cube(Vec3::new(0.0, 0.02, 0.0), Vec3::new(0.13, 0.12, 0.14), &a.boot[0], s);
                    b.cube(Vec3::new(0.0, 0.085, 0.0), Vec3::new(0.135, 0.02, 0.145), &a.boot[1], s);
                    b.cube(Vec3::new(0.0, -0.125, -0.06), Vec3::new(0.035, 0.045, 0.28), &a.plate, s);
                    for z in [-0.17_f32, -0.085, 0.0, 0.085] {
                        wheels.push((b.wheel(Vec3::new(0.0, -0.165, z), 0.035, 0.024, &ure, s), 0.035));
                    }
                } else {
                    // Soap shoes: a hard plastic plate set into the sole.
                    b.cube(Vec3::new(0.0, -0.1, -0.06), Vec3::new(0.122, 0.012, 0.292), &a.boot[1], s);
                    b.cube(Vec3::new(0.0, -0.106, -0.04), Vec3::new(0.06, 0.012, 0.11), &a.plate, s);
                }
                s
            };
            let l = skate(&mut b, rig.ankle(0));
            let r = skate(&mut b, rig.ankle(1));
            b.c.entity(root).despawn();
            return MountAttach { kind, root: l, bars: None, wheels, other: Some(r), center, whip_pivot, roll: 0.0 };
        }
    }
    MountAttach { kind, root, bars, wheels, other, center, whip_pivot, roll: 0.0 }
}

/// Give each rig the model for what its player rides (and take it away when they hop off).
fn sync_mount_models(
    mut commands: Commands,
    view: Res<View>,
    local: Res<LocalRide>,
    assets: Res<MountAssets>,
    mut rigs: Query<&mut Rig>,
) {
    let Some(snap) = view.snap.as_ref() else { return };
    for mut rig in &mut rigs {
        let (want, team) = match rig.player {
            None => (local.ride.as_ref().map(|r| r.mount), view.me().map_or(0, |p| p.team.index())),
            Some(id) => {
                let pv = snap.players.iter().find(|p| p.id == id);
                (pv.and_then(|p| p.ride.map(|r| r.mount)), pv.map_or(0, |p| p.team.index()))
            }
        };
        let have = rig.mount.as_ref().map(|m| m.kind);
        if have == want {
            continue;
        }
        if let Some(old) = rig.mount.take() {
            commands.entity(old.root).despawn();
            if let Some(o) = old.other {
                commands.entity(o).despawn();
            }
        }
        if let Some(kind) = want {
            let model = spawn_model(&mut commands, &assets, &rig, kind, team);
            rig.mount = Some(model);
        }
    }
}

/// Pose each mount model under its rider for this frame's trick / grind.
fn pose_mounts(time: Res<Time>, mut rigs: Query<&mut Rig>, mut transforms: Query<&mut Transform, Without<Rig>>) {
    let dt = time.delta_secs();
    for mut rig in &mut rigs {
        let (drop, lunge, lift, yaw) = (rig.pose_drop, rig.pose_lunge, rig.pose_lift, rig.pose_yaw);
        let Some(ride) = rig.ride else { continue };
        let Some(m) = rig.mount.as_mut() else { continue };
        // Roll the wheels.
        m.roll += ride.speed * dt;
        for (w, r) in &m.wheels {
            if let Ok(mut tf) = transforms.get_mut(*w) {
                tf.rotation = Quat::from_rotation_x(-m.roll / r.max(0.01));
            }
        }
        if ride.kind.on_feet() {
            continue; // skates ride along on the ankles
        }
        // Ground point under the pivot, in the pivot's frame.
        let ground = Vec3::new(0.0, -(PIVOT - drop) - lift, lunge);
        // Undo the rider's side-on stance (and spins, on boards) so the model points where we go.
        let stance = if ride.stance == Stance::Sideways { yaw } else { 0.0 };
        let mut base = Quat::from_rotation_y(-stance);
        let mut spin = Quat::IDENTITY;
        let mut raise = 0.0;
        let mut bar = Quat::IDENTITY;
        let mut whip = Quat::IDENTITY;
        if let Some(g) = ride.grind {
            base *= Quat::from_rotation_y(g.angle.to_radians()) * Quat::from_rotation_x(g.tilt.to_radians());
        } else if let Some((cat, a, p)) = ride.trick {
            let p = p.min(1.0);
            spin = Quat::from_rotation_y(TAU * a.board_spin * p)
                * Quat::from_rotation_z(TAU * a.board_roll * p)
                * Quat::from_rotation_x(TAU * a.board_pitch * p);
            bar = Quat::from_rotation_y(TAU * a.bar_spin * p);
            whip = Quat::from_rotation_y(TAU * a.frame_whip * p);
            // Tucked legs bring the board up with the feet.
            if a.legs == Legs::Tuck && cat != TrickCat::AirFlip {
                let w = if cat == TrickCat::Grab { (p * 4.0).min(1.0) } else { (p * std::f32::consts::PI).sin() };
                raise = 0.35 * w;
            }
        }
        if ride.bail {
            spin = Quat::from_rotation_z((time.elapsed_secs() * 9.0).sin() * 0.4);
        }
        let center = m.center;
        let pivot = m.whip_pivot;
        if let Ok(mut tf) = transforms.get_mut(m.root) {
            // Whip round the head tube, then flip about the model's centre.
            let whip_tf = Transform::from_translation(pivot) * Transform::from_rotation(whip) * Transform::from_translation(-pivot);
            let flip_tf = Transform::from_translation(center) * Transform::from_rotation(spin) * Transform::from_translation(-center);
            let local = flip_tf * whip_tf;
            let placed = Transform::from_translation(ground + Vec3::Y * raise).with_rotation(base) * local;
            *tf = placed;
        }
        if let Some(bars) = m.bars {
            if let Ok(mut tf) = transforms.get_mut(bars) {
                // Bars stay with the rider through a tailwhip; barspins spin them.
                tf.rotation = whip.inverse() * bar;
            }
        }
    }
}
