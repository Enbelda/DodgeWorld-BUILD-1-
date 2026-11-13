//! Procedural humanoids with a real joint chain: pelvis, a bending spine, chest and
//! shoulder yoke, neck and head (hair, ears, nose, jaw, visor), arms with shoulder, elbow
//! and wrist joints, hands with a palm, four two-jointed fingers and a thumb, legs with hip,
//! knee (kneecap) and ankle joints, calves and shoes. Every joint has a rounded joint
//! capsule so the segments stay visibly connected as they bend.
//!
//! Posed every frame from the player's state: walk/run cycle with a forward lean and
//! breathing, gripping the held ball, winding up and swinging through a throw, open hands
//! reaching out to catch, knocked flat, slimed spread-eagle, tucked flips, and a white
//! flash when hit.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::prelude::*;

use crate::sim::{PlayerId, PlayerView, Team, TrickKind, TrickState};

/// Height of the hip/flip pivot above the feet. Flips rotate the body around it.
const PIVOT: f32 = 0.9;
/// Seconds the throw swing lasts.
const THROW_SECS: f32 = 0.3;
/// Seconds of white flash after a hit.
const FLASH_SECS: f32 = 0.15;
/// Running speed that counts as a full stride.
const FULL_STRIDE_SPEED: f32 = 7.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Part {
    Shirt,
    Pants,
    Skin,
    Shoe,
    Hair,
    Visor,
}

/// Joint order inside `Pose::joints` / `Rig::joints`.
const L_SHOULDER: usize = 0;
const L_ELBOW: usize = 1;
const R_SHOULDER: usize = 2;
const R_ELBOW: usize = 3;
const L_HIP: usize = 4;
const L_KNEE: usize = 5;
const R_HIP: usize = 6;
const R_KNEE: usize = 7;
const SPINE: usize = 8;
const NECK: usize = 9;
const L_WRIST: usize = 10;
const R_WRIST: usize = 11;
const L_ANKLE: usize = 12;
const R_ANKLE: usize = 13;
const JOINTS: usize = 14;

/// Which bone of a finger a finger joint moves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FingerBone {
    /// Knuckle: the first bone of a finger.
    Base,
    /// Middle knuckle: the outer bone.
    Tip,
    ThumbBase,
    ThumbTip,
}

#[derive(Default)]
struct Anim {
    walk_phase: f32,
    last_pos: Option<Vec3>,
    speed: f32,
    was_holding: bool,
    throw_t: f32,
    last_hits: Option<u32>,
    flash: f32,
}

/// A humanoid model. `player` is `None` for the local player (driven by `LocalRigInput`).
#[derive(Component)]
pub struct Rig {
    pub player: Option<PlayerId>,
    flip: Entity,
    joints: [Entity; JOINTS],
    /// (joint, hand 0 = left / 1 = right, bone).
    fingers: Vec<(Entity, usize, FingerBone)>,
    parts: Vec<(Entity, Part)>,
    anim: Anim,
}

/// Extra local-only inputs: throw charge and the trick we are doing right now.
#[derive(Resource, Default)]
pub struct LocalRigInput {
    pub charge: Option<f32>,
    pub trick: Option<TrickState>,
    pub dashing: bool,
}

#[derive(Resource)]
pub struct RigAssets {
    sphere: Handle<Mesh>,
    capsule: Handle<Mesh>,
    cylinder: Handle<Mesh>,
    cube: Handle<Mesh>,
    shirt: [Handle<StandardMaterial>; 2],
    pants: [Handle<StandardMaterial>; 2],
    skin: Handle<StandardMaterial>,
    shoe: Handle<StandardMaterial>,
    hair: Handle<StandardMaterial>,
    visor_mat: Handle<StandardMaterial>,
    visor_catch: Handle<StandardMaterial>,
    slimed: Handle<StandardMaterial>,
    flash: Handle<StandardMaterial>,
    /// Glowing blue while dashing (invincible).
    dash: Handle<StandardMaterial>,
}

pub fn make_rig_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let glow = |m: &mut Assets<StandardMaterial>, c: Color, e: LinearRgba| {
        m.add(StandardMaterial { base_color: c, emissive: e, ..default() })
    };
    let matte = |m: &mut Assets<StandardMaterial>, c: Color| {
        m.add(StandardMaterial { base_color: c, perceptual_roughness: 0.8, ..default() })
    };
    // Unit shapes; every body part scales one of these.
    commands.insert_resource(RigAssets {
        sphere: meshes.add(Sphere::new(0.5).mesh().uv(16, 10)),
        capsule: meshes.add(Capsule3d::new(0.5, 1.0).mesh().rings(2).latitudes(8).longitudes(12)),
        cylinder: meshes.add(Cylinder::new(0.5, 1.0).mesh().resolution(12)),
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        shirt: [matte(&mut materials, Color::srgb(0.90, 0.25, 0.22)), matte(&mut materials, Color::srgb(0.22, 0.45, 0.95))],
        pants: [matte(&mut materials, Color::srgb(0.45, 0.10, 0.10)), matte(&mut materials, Color::srgb(0.10, 0.20, 0.45))],
        skin: matte(&mut materials, Color::srgb(0.93, 0.76, 0.60)),
        shoe: matte(&mut materials, Color::srgb(0.12, 0.12, 0.14)),
        hair: matte(&mut materials, Color::srgb(0.20, 0.13, 0.08)),
        visor_mat: materials.add(StandardMaterial {
            base_color: Color::srgb(0.06, 0.06, 0.08),
            perceptual_roughness: 0.2,
            ..default()
        }),
        visor_catch: glow(&mut materials, Color::srgb(1.0, 1.0, 0.6), LinearRgba::rgb(4.0, 4.0, 1.5)),
        slimed: glow(&mut materials, Color::srgb(0.35, 0.85, 0.3), LinearRgba::rgb(0.2, 1.2, 0.15)),
        flash: glow(&mut materials, Color::WHITE, LinearRgba::rgb(8.0, 8.0, 8.0)),
        dash: glow(&mut materials, Color::srgb(0.3, 0.6, 1.0), LinearRgba::rgb(1.0, 3.0, 9.0)),
    });
}

#[derive(Clone, Copy)]
enum Shape {
    Sphere,
    Capsule,
    Cylinder,
    Cube,
}

/// Scale of the unit capsule (radius 0.5, total height 2.0) for a radius and total height.
fn capsule_scale(radius: f32, total_height: f32) -> Vec3 {
    Vec3::new(radius * 2.0, total_height / 2.0, radius * 2.0)
}

struct Builder<'a, 'w, 's> {
    c: &'a mut Commands<'w, 's>,
    a: &'a RigAssets,
    parts: Vec<(Entity, Part)>,
}

impl Builder<'_, '_, '_> {
    fn joint(&mut self, at: Vec3, parent: Entity) -> Entity {
        self.c.spawn((Transform::from_translation(at), Visibility::default(), ChildOf(parent))).id()
    }

    fn part(&mut self, shape: Shape, tf: Transform, parent: Entity, kind: Part) {
        let mesh = match shape {
            Shape::Sphere => &self.a.sphere,
            Shape::Capsule => &self.a.capsule,
            Shape::Cylinder => &self.a.cylinder,
            Shape::Cube => &self.a.cube,
        };
        let e = self.c.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(self.a.skin.clone()), tf, ChildOf(parent))).id();
        self.parts.push((e, kind));
    }

    /// Sphere of `radius` at `at`.
    fn ball(&mut self, at: Vec3, radius: f32, parent: Entity, kind: Part) {
        self.part(Shape::Sphere, Transform::from_translation(at).with_scale(Vec3::splat(radius * 2.0)), parent, kind);
    }

    /// Scaled shape at `at` with full size `size`.
    fn shape(&mut self, shape: Shape, at: Vec3, size: Vec3, parent: Entity, kind: Part) {
        self.part(shape, Transform::from_translation(at).with_scale(size), parent, kind);
    }

    /// A limb segment hanging down from `parent`'s origin: capsule from y=0 to y=-length.
    fn bone(&mut self, length: f32, radius: f32, parent: Entity, kind: Part) {
        let tf = Transform::from_xyz(0.0, -length / 2.0, 0.0).with_scale(capsule_scale(radius, length + radius));
        self.part(Shape::Capsule, tf, parent, kind);
    }
}

/// Fingers from front (index, -Z) to back (little): z offset, base and tip bone lengths.
const FINGERS: [(f32, f32, f32); 4] =
    [(-0.027, 0.040, 0.034), (-0.009, 0.044, 0.038), (0.009, 0.041, 0.035), (0.027, 0.032, 0.028)];

/// Palm, four fingers with two joints each, and a thumb, under the wrist joint.
fn build_hand(b: &mut Builder, wrist: Entity, hand: usize, fingers: &mut Vec<(Entity, usize, FingerBone)>) {
    // Palm faces the body: inward is +X for the left hand, -X for the right.
    let inward = if hand == 0 { 1.0 } else { -1.0 };
    b.ball(Vec3::ZERO, 0.036, wrist, Part::Skin);
    b.shape(Shape::Cube, Vec3::new(0.0, -0.048, 0.0), Vec3::new(0.032, 0.085, 0.078), wrist, Part::Skin);
    for (z, base_len, tip_len) in FINGERS {
        let knuckle = b.joint(Vec3::new(0.0, -0.09, z), wrist);
        b.ball(Vec3::ZERO, 0.012, knuckle, Part::Skin);
        b.shape(Shape::Cube, Vec3::new(0.0, -base_len / 2.0, 0.0), Vec3::new(0.022, base_len, 0.017), knuckle, Part::Skin);
        let mid = b.joint(Vec3::new(0.0, -base_len, 0.0), knuckle);
        b.ball(Vec3::ZERO, 0.0105, mid, Part::Skin);
        b.shape(Shape::Cube, Vec3::new(0.0, -tip_len / 2.0, 0.0), Vec3::new(0.019, tip_len, 0.016), mid, Part::Skin);
        fingers.push((knuckle, hand, FingerBone::Base));
        fingers.push((mid, hand, FingerBone::Tip));
    }
    let thumb = b.joint(Vec3::new(0.012 * inward, -0.02, -0.04), wrist);
    b.ball(Vec3::ZERO, 0.016, thumb, Part::Skin);
    b.shape(Shape::Cube, Vec3::new(0.0, -0.018, 0.0), Vec3::new(0.024, 0.036, 0.02), thumb, Part::Skin);
    let thumb_tip = b.joint(Vec3::new(0.0, -0.036, 0.0), thumb);
    b.ball(Vec3::ZERO, 0.012, thumb_tip, Part::Skin);
    b.shape(Shape::Cube, Vec3::new(0.0, -0.015, 0.0), Vec3::new(0.021, 0.03, 0.018), thumb_tip, Part::Skin);
    fingers.push((thumb, hand, FingerBone::ThumbBase));
    fingers.push((thumb_tip, hand, FingerBone::ThumbTip));
}

/// Build a rig under `parent`. The rig's origin is at the feet; `offset` places it.
pub fn spawn_rig(commands: &mut Commands, a: &RigAssets, parent: Entity, offset: Vec3, player: Option<PlayerId>) -> Entity {
    let root = commands.spawn((Transform::from_translation(offset), Visibility::default(), ChildOf(parent))).id();
    let flip = commands.spawn((Transform::from_xyz(0.0, PIVOT, 0.0), Visibility::default(), ChildOf(root))).id();
    let mut b = Builder { c: commands, a, parts: Vec::new() };
    let mut joints = [Entity::PLACEHOLDER; JOINTS];
    let mut fingers = Vec::new();

    // Pelvis (shorts).
    b.shape(Shape::Capsule, Vec3::ZERO, Vec3::new(0.34, 0.11, 0.22), flip, Part::Pants);

    // Spine: abdomen, chest, and the trapezius band joining neck and shoulders.
    let spine = b.joint(Vec3::new(0.0, 0.06, 0.0), flip);
    joints[SPINE] = spine;
    b.shape(Shape::Capsule, Vec3::new(0.0, 0.1, 0.0), Vec3::new(0.30, 0.17, 0.21), spine, Part::Shirt);
    b.shape(Shape::Capsule, Vec3::new(0.0, 0.31, 0.0), Vec3::new(0.43, 0.21, 0.25), spine, Part::Shirt);
    b.part(
        Shape::Capsule,
        Transform::from_xyz(0.0, 0.44, 0.01)
            .with_rotation(Quat::from_rotation_z(FRAC_PI_2))
            .with_scale(Vec3::new(0.12, 0.25, 0.12)),
        spine,
        Part::Shirt,
    );

    // Neck and head: skull, jaw, hair, ears, nose, visor.
    let neck = b.joint(Vec3::new(0.0, 0.48, 0.0), spine);
    joints[NECK] = neck;
    b.shape(Shape::Cylinder, Vec3::new(0.0, 0.06, 0.0), Vec3::new(0.11, 0.14, 0.11), neck, Part::Skin);
    let head = Vec3::new(0.0, 0.21, 0.0);
    b.shape(Shape::Sphere, head, Vec3::new(0.21, 0.255, 0.235), neck, Part::Skin);
    b.shape(Shape::Sphere, head + Vec3::new(0.0, -0.07, -0.035), Vec3::new(0.15, 0.11, 0.15), neck, Part::Skin);
    b.shape(Shape::Sphere, head + Vec3::new(0.0, 0.035, 0.012), Vec3::new(0.225, 0.22, 0.245), neck, Part::Hair);
    for x in [-0.105_f32, 0.105] {
        b.shape(Shape::Sphere, head + Vec3::new(x, -0.005, 0.01), Vec3::new(0.03, 0.06, 0.045), neck, Part::Skin);
    }
    b.shape(Shape::Sphere, head + Vec3::new(0.0, -0.025, -0.118), Vec3::new(0.035, 0.05, 0.04), neck, Part::Skin);
    b.shape(Shape::Cube, head + Vec3::new(0.0, 0.015, -0.105), Vec3::new(0.19, 0.045, 0.04), neck, Part::Visor);

    // Arms: deltoid, sleeve, upper arm with biceps, elbow, forearm, wrist, hand.
    for (side, x) in [(0usize, -0.235_f32), (1, 0.235)] {
        let shoulder = b.joint(Vec3::new(x, 0.42, 0.0), spine);
        b.ball(Vec3::ZERO, 0.075, shoulder, Part::Shirt);
        b.shape(Shape::Cylinder, Vec3::new(0.0, -0.06, 0.0), Vec3::new(0.13, 0.12, 0.13), shoulder, Part::Shirt);
        b.bone(0.28, 0.052, shoulder, Part::Skin);
        b.shape(Shape::Sphere, Vec3::new(0.0, -0.15, -0.018), Vec3::new(0.085, 0.15, 0.08), shoulder, Part::Skin);
        let elbow = b.joint(Vec3::new(0.0, -0.28, 0.0), shoulder);
        b.ball(Vec3::ZERO, 0.047, elbow, Part::Skin);
        b.bone(0.25, 0.043, elbow, Part::Skin);
        b.shape(Shape::Sphere, Vec3::new(0.0, -0.07, 0.0), Vec3::new(0.095, 0.15, 0.09), elbow, Part::Skin);
        let wrist = b.joint(Vec3::new(0.0, -0.25, 0.0), elbow);
        build_hand(&mut b, wrist, side, &mut fingers);
        joints[side * 2] = shoulder;
        joints[side * 2 + 1] = elbow;
        joints[L_WRIST + side] = wrist;
    }

    // Legs: hip, thigh, knee with kneecap, shin with calf, ankle, shoe.
    for (side, x) in [(0usize, -0.1_f32), (1, 0.1)] {
        let hip = b.joint(Vec3::new(x, -0.03, 0.0), flip);
        b.ball(Vec3::ZERO, 0.085, hip, Part::Pants);
        b.bone(0.42, 0.078, hip, Part::Pants);
        let knee = b.joint(Vec3::new(0.0, -0.42, 0.0), hip);
        b.ball(Vec3::ZERO, 0.062, knee, Part::Skin);
        b.ball(Vec3::new(0.0, 0.0, -0.035), 0.04, knee, Part::Skin);
        b.bone(0.39, 0.058, knee, Part::Skin);
        b.shape(Shape::Sphere, Vec3::new(0.0, -0.13, 0.025), Vec3::new(0.11, 0.2, 0.11), knee, Part::Skin);
        let ankle = b.joint(Vec3::new(0.0, -0.39, 0.0), knee);
        b.ball(Vec3::ZERO, 0.046, ankle, Part::Skin);
        b.shape(Shape::Cube, Vec3::new(0.0, -0.045, -0.05), Vec3::new(0.11, 0.07, 0.24), ankle, Part::Shoe);
        b.shape(Shape::Sphere, Vec3::new(0.0, -0.05, -0.16), Vec3::new(0.11, 0.07, 0.08), ankle, Part::Shoe);
        joints[4 + side * 2] = hip;
        joints[4 + side * 2 + 1] = knee;
        joints[L_ANKLE + side] = ankle;
    }
    let parts = b.parts;
    commands.entity(root).insert(Rig { player, flip, joints, fingers, parts, anim: Anim::default() });
    root
}

/// What drives a pose.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PoseInput {
    pub walk_phase: f32,
    /// 0 (standing) .. 1 (full run).
    pub stride: f32,
    pub holding: bool,
    /// Wind-up 0..1 while charging a throw.
    pub charge: Option<f32>,
    /// Seconds since the last throw (>= THROW_SECS means done).
    pub throw_t: f32,
    pub catching: bool,
    pub stunned: bool,
    pub slimed: bool,
    pub trick: Option<TrickState>,
    /// Seconds since start, for breathing.
    pub time: f32,
}

/// Joint angles: (pitch about X, roll about Z). Positive shoulder pitch raises the arm forward;
/// negative knee pitch bends the knee; negative spine pitch leans forward; negative ankle
/// pitch points the toes down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    pub joints: [(f32, f32); JOINTS],
    /// Rotation of the whole body about X (flips, lying down).
    pub body_pitch: f32,
    /// How far the pivot drops (lying on the floor).
    pub drop: f32,
    /// How closed each hand is: 0 open and spread, 1 a fist. [left, right].
    pub grip: [f32; 2],
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

pub fn pose(i: &PoseInput) -> Pose {
    let mut p = Pose { grip: [0.35, 0.35], ..default() };
    let swing = i.walk_phase.sin() * i.stride;
    let breathe = (i.time * 1.8).sin();
    let j = &mut p.joints;

    // Walk cycle: legs swing opposite, arms opposite to legs, knees bend on the back swing,
    // toes push off, the body leans into the run and the elbows bend more.
    j[L_HIP] = (swing * 0.7, 0.0);
    j[R_HIP] = (-swing * 0.7, 0.0);
    j[L_KNEE] = (-(i.walk_phase.cos().max(0.0)) * 0.9 * i.stride, 0.0);
    j[R_KNEE] = (-((-i.walk_phase.cos()).max(0.0)) * 0.9 * i.stride, 0.0);
    j[L_ANKLE] = (j[L_KNEE].0 * 0.4, 0.0);
    j[R_ANKLE] = (j[R_KNEE].0 * 0.4, 0.0);
    j[L_SHOULDER] = (-swing * 0.6 + 0.1, -0.1 - 0.02 * breathe);
    j[R_SHOULDER] = (swing * 0.6 + 0.1, 0.1 + 0.02 * breathe);
    j[L_ELBOW] = (0.3 + 0.9 * i.stride, 0.0);
    j[R_ELBOW] = (0.3 + 0.9 * i.stride, 0.0);
    j[SPINE] = (-0.18 * i.stride + 0.02 * breathe, 0.0);
    j[NECK] = (0.15 * i.stride - 0.015 * breathe, 0.0);

    // Right arm: grip the ball out front, cock it back while charging, swing through on release.
    if i.throw_t < THROW_SECS {
        let t = i.throw_t / THROW_SECS;
        j[R_SHOULDER] = (lerp(-2.5, 1.6, t), 0.1);
        j[R_ELBOW] = (lerp(1.2, 0.1, t), 0.0);
        j[R_WRIST] = (lerp(-0.6, 0.5, t), 0.0);
        j[L_SHOULDER] = (0.8, -0.2);
        j[SPINE] = (lerp(0.2, -0.35, t), 0.0);
        p.grip[1] = 0.1; // fingers open as the ball leaves
    } else if i.holding {
        p.grip[1] = 0.9;
        match i.charge {
            Some(c) => {
                j[R_SHOULDER] = (lerp(1.1, -2.5, c), 0.15);
                j[R_ELBOW] = (lerp(0.6, 1.2, c), 0.0);
                j[R_WRIST] = (lerp(0.0, -0.6, c), 0.0);
                j[L_SHOULDER] = (lerp(0.2, 1.2, c), -0.15);
                j[SPINE] = (lerp(j[SPINE].0, 0.2, c), 0.0);
                p.grip[0] = lerp(0.35, 0.1, c); // leading hand opens toward the target
            }
            None => {
                j[R_SHOULDER] = (1.1, 0.1);
                j[R_ELBOW] = (0.6, 0.0);
            }
        }
    }

    // Reaching out to catch: both arms straight forward, slightly apart, hands wide open.
    if i.catching {
        j[L_SHOULDER] = (1.5, -0.25);
        j[R_SHOULDER] = (1.5, 0.25);
        j[L_ELBOW] = (0.15, 0.0);
        j[R_ELBOW] = (0.15, 0.0);
        j[L_WRIST] = (0.3, 0.0);
        j[R_WRIST] = (0.3, 0.0);
        j[SPINE] = (-0.1, 0.0);
        p.grip = [0.05, 0.05];
    }

    // Slimed to a wall: spread-eagle, fingers splayed.
    if i.slimed {
        j[L_SHOULDER] = (0.0, -1.4);
        j[R_SHOULDER] = (0.0, 1.4);
        j[L_ELBOW] = (0.1, 0.0);
        j[R_ELBOW] = (0.1, 0.0);
        j[L_HIP] = (0.0, -0.3);
        j[R_HIP] = (0.0, 0.3);
        j[L_KNEE] = (0.0, 0.0);
        j[R_KNEE] = (0.0, 0.0);
        j[SPINE] = (0.05, 0.0);
        p.grip = [0.0, 0.0];
    }

    if let Some(t) = i.trick {
        let tuck = (t.progress * PI).sin();
        match t.kind {
            TrickKind::FrontFlip => p.body_pitch = -TAU * t.progress,
            TrickKind::BackFlip => p.body_pitch = TAU * t.progress,
            TrickKind::SpinLeft | TrickKind::SpinRight => {
                // Arms pulled in to spin fast.
                j[L_SHOULDER] = (0.6 * tuck, -0.1);
                j[L_ELBOW] = (1.6 * tuck, 0.0);
                if !i.holding {
                    j[R_SHOULDER] = (0.6 * tuck, 0.1);
                    j[R_ELBOW] = (1.6 * tuck, 0.0);
                }
            }
        }
        if matches!(t.kind, TrickKind::FrontFlip | TrickKind::BackFlip) {
            // Tucked: knees to the chest, arms hugging the shins, head down.
            j[L_HIP] = (1.3 * tuck, 0.0);
            j[R_HIP] = (1.3 * tuck, 0.0);
            j[L_KNEE] = (-1.8 * tuck, 0.0);
            j[R_KNEE] = (-1.8 * tuck, 0.0);
            j[SPINE] = (-0.45 * tuck, 0.0);
            j[NECK] = (-0.3 * tuck, 0.0);
            j[L_SHOULDER] = (lerp(j[L_SHOULDER].0, 0.9, tuck), -0.15);
            j[L_ELBOW] = (lerp(j[L_ELBOW].0, 1.3, tuck), 0.0);
            p.grip[0] = lerp(p.grip[0], 0.8, tuck);
            if !i.holding {
                j[R_SHOULDER] = (lerp(j[R_SHOULDER].0, 0.9, tuck), 0.15);
                j[R_ELBOW] = (lerp(j[R_ELBOW].0, 1.3, tuck), 0.0);
                p.grip[1] = lerp(p.grip[1], 0.8, tuck);
            }
        }
    }

    // Knocked flat by a brick: on the back, limbs limp.
    if i.stunned {
        p.body_pitch = FRAC_PI_2;
        p.drop = PIVOT - 0.2;
        j[L_SHOULDER] = (0.3, -1.1);
        j[R_SHOULDER] = (0.3, 1.1);
        j[L_ELBOW] = (0.4, 0.0);
        j[R_ELBOW] = (0.4, 0.0);
        j[L_WRIST] = (0.0, 0.0);
        j[R_WRIST] = (0.0, 0.0);
        j[L_HIP] = (0.1, -0.15);
        j[R_HIP] = (0.1, 0.15);
        j[L_KNEE] = (-0.3, 0.0);
        j[R_KNEE] = (-0.2, 0.0);
        j[L_ANKLE] = (0.3, 0.0);
        j[R_ANKLE] = (0.3, 0.0);
        j[SPINE] = (0.0, 0.0);
        j[NECK] = (0.25, 0.1);
        p.grip = [0.45, 0.45];
    }
    p
}

/// Rotation of one finger joint for a hand that is `grip` closed.
fn finger_rotation(hand: usize, bone: FingerBone, grip: f32) -> Quat {
    // Curling toward the palm turns about Z: +Z for the left hand, -Z for the right.
    let inward = if hand == 0 { 1.0 } else { -1.0 };
    let spread = 1.0 - grip;
    match bone {
        FingerBone::Base => Quat::from_rotation_z(inward * (grip * 1.5 - 0.1 * spread)),
        FingerBone::Tip => Quat::from_rotation_z(inward * grip * 1.6),
        // The thumb points forward-down and folds across the palm.
        FingerBone::ThumbBase => Quat::from_rotation_x(0.6 + 0.2 * spread) * Quat::from_rotation_z(inward * grip * 0.9),
        FingerBone::ThumbTip => Quat::from_rotation_z(inward * grip * 0.8),
    }
}

/// Pose every rig from its player's state.
#[allow(clippy::too_many_arguments)]
pub fn animate_rigs(
    time: Res<Time>,
    view: Res<crate::session::View>,
    local: Res<LocalRigInput>,
    assets: Res<RigAssets>,
    mut rigs: Query<(&mut Rig, &GlobalTransform)>,
    mut transforms: Query<&mut Transform, Without<Rig>>,
    mut mats: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    let dt = time.delta_secs().max(1e-4);
    let Some(snap) = view.snap.as_ref() else { return };
    for (mut rig, gt) in &mut rigs {
        let pv: Option<&PlayerView> = match rig.player {
            Some(id) => snap.players.iter().find(|p| p.id == id),
            None => view.me(),
        };
        let Some(pv) = pv else { continue };
        let is_local = rig.player.is_none();
        let r: &mut Rig = &mut rig;
        let a = &mut r.anim;

        // Speed from how far the model actually moved.
        let pos = gt.translation();
        let moved = a.last_pos.map_or(0.0, |l| Vec3::new(pos.x - l.x, 0.0, pos.z - l.z).length());
        a.last_pos = Some(pos);
        let speed = if moved > 2.0 { 0.0 } else { moved / dt };
        a.speed += (speed - a.speed) * (1.0 - (-10.0 * dt).exp());
        let stride = (a.speed / FULL_STRIDE_SPEED).clamp(0.0, 1.0);
        a.walk_phase = (a.walk_phase + dt * 11.0 * stride) % TAU;

        if a.was_holding && !pv.holding && pv.alive() && pv.stunned <= 0.0 {
            a.throw_t = 0.0;
        } else {
            a.throw_t += dt;
        }
        a.was_holding = pv.holding;
        if a.last_hits.is_some_and(|h| h != pv.hits) {
            a.flash = FLASH_SECS;
        }
        a.last_hits = Some(pv.hits);
        a.flash = (a.flash - dt).max(0.0);

        let input = PoseInput {
            walk_phase: a.walk_phase,
            stride,
            holding: pv.holding,
            charge: if is_local { local.charge } else { None },
            throw_t: a.throw_t,
            catching: pv.catching,
            stunned: pv.stunned > 0.0,
            slimed: pv.slimed > 0.0,
            trick: if is_local { local.trick } else { pv.trick },
            // Offset per player so a team doesn't breathe in sync.
            time: time.elapsed_secs() + pv.id as f32 * 0.7,
        };
        let p = pose(&input);

        if let Ok(mut tf) = transforms.get_mut(r.flip) {
            tf.translation.y = PIVOT - p.drop;
            tf.rotation = Quat::from_rotation_x(p.body_pitch);
        }
        for (k, &(pitch, roll)) in p.joints.iter().enumerate() {
            if let Ok(mut tf) = transforms.get_mut(r.joints[k]) {
                tf.rotation = Quat::from_rotation_z(roll) * Quat::from_rotation_x(pitch);
            }
        }
        for &(e, hand, bone) in &r.fingers {
            if let Ok(mut tf) = transforms.get_mut(e) {
                tf.rotation = finger_rotation(hand, bone, p.grip[hand]);
            }
        }

        let team = match pv.team {
            Team::Red => 0,
            Team::Blue => 1,
        };
        let flashing = a.flash > 0.0;
        let dashing = if is_local { local.dashing } else { pv.dashing };
        let slimed = pv.slimed > 0.0;
        for &(e, kind) in &r.parts {
            let Ok(mut m) = mats.get_mut(e) else { continue };
            let want = if dashing {
                &assets.dash
            } else if flashing {
                &assets.flash
            } else {
                match kind {
                    Part::Visor if pv.catching => &assets.visor_catch,
                    Part::Visor => &assets.visor_mat,
                    Part::Shirt | Part::Pants if slimed => &assets.slimed,
                    Part::Shirt => &assets.shirt[team],
                    Part::Pants => &assets.pants[team],
                    Part::Skin => &assets.skin,
                    Part::Shoe => &assets.shoe,
                    Part::Hair => &assets.hair,
                }
            };
            if m.0 != *want {
                m.0 = want.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> PoseInput {
        PoseInput { throw_t: 10.0, ..default() }
    }

    #[test]
    fn catching_reaches_both_arms_forward_with_open_hands() {
        let p = pose(&PoseInput { catching: true, ..idle() });
        assert!(p.joints[L_SHOULDER].0 > 1.3 && p.joints[R_SHOULDER].0 > 1.3);
        assert!(p.joints[L_ELBOW].0 < 0.3, "arms straight");
        assert!(p.grip[0] < 0.1 && p.grip[1] < 0.1, "hands open");
    }

    #[test]
    fn charging_cocks_the_right_arm_back() {
        let hold = pose(&PoseInput { holding: true, ..idle() });
        let wound = pose(&PoseInput { holding: true, charge: Some(1.0), ..idle() });
        assert!(hold.joints[R_SHOULDER].0 > 1.0, "ball held out front");
        assert!(hold.grip[1] > 0.8, "fingers wrapped round the ball");
        assert!(wound.joints[R_SHOULDER].0 < -2.0, "arm up and back");
        assert!(wound.joints[SPINE].0 > 0.1, "leaning back into the wind-up");
    }

    #[test]
    fn throw_swings_through_and_lets_go() {
        let start = pose(&PoseInput { throw_t: 0.0, ..idle() });
        let end = pose(&PoseInput { throw_t: THROW_SECS * 0.99, ..idle() });
        assert!(start.joints[R_SHOULDER].0 < 0.0 && end.joints[R_SHOULDER].0 > 1.0);
        assert!(end.joints[SPINE].0 < -0.2, "follow-through leans forward");
        assert!(end.grip[1] < 0.2, "fingers open");
    }

    #[test]
    fn walking_swings_legs_opposite() {
        let p = pose(&PoseInput { walk_phase: FRAC_PI_2, stride: 1.0, ..idle() });
        assert!(p.joints[L_HIP].0 > 0.5 && p.joints[R_HIP].0 < -0.5);
        assert!(p.joints[SPINE].0 < -0.1, "leans into the run");
        let still = pose(&idle());
        assert_eq!(still.joints[L_HIP].0, 0.0);
    }

    #[test]
    fn ankles_follow_the_knees() {
        let p = pose(&PoseInput { walk_phase: 0.0, stride: 1.0, ..idle() });
        assert!(p.joints[L_KNEE].0 < -0.5);
        assert!(p.joints[L_ANKLE].0 < 0.0, "toes push off");
    }

    #[test]
    fn flips_rotate_opposite_ways_and_tuck() {
        let t = |kind| Some(TrickState { kind, progress: 0.25 });
        let front = pose(&PoseInput { trick: t(TrickKind::FrontFlip), ..idle() });
        let back = pose(&PoseInput { trick: t(TrickKind::BackFlip), ..idle() });
        assert!(front.body_pitch < 0.0 && back.body_pitch > 0.0);
        assert!(front.joints[L_KNEE].0 < -1.0, "knees tucked");
        assert!(front.joints[SPINE].0 < -0.2, "curled up");
        let spin = pose(&PoseInput { trick: t(TrickKind::SpinLeft), ..idle() });
        assert_eq!(spin.body_pitch, 0.0, "spins turn the player, not the rig");
    }

    #[test]
    fn stunned_lies_flat_on_the_floor() {
        let p = pose(&PoseInput {
            stunned: true,
            trick: Some(TrickState { kind: TrickKind::FrontFlip, progress: 0.5 }),
            ..idle()
        });
        assert!((p.body_pitch - FRAC_PI_2).abs() < 1e-6, "stun overrides tricks");
        assert!(PIVOT - p.drop < 0.3);
    }

    #[test]
    fn fingers_curl_toward_each_palm() {
        // Fingers hang along -Y; a closed hand curls them inward (toward the body's centre).
        let tip = |hand| finger_rotation(hand, FingerBone::Base, 1.0) * Vec3::NEG_Y;
        assert!(tip(0).x > 0.5, "left hand curls toward +X");
        assert!(tip(1).x < -0.5, "right hand curls toward -X");
        let open = finger_rotation(0, FingerBone::Tip, 0.0) * Vec3::NEG_Y;
        assert!(open.y < -0.99, "open hand is straight");
    }
}
