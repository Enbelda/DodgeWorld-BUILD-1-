//! Small blocky humanoids: torso, head with a visor, two-part arms with hands,
//! two-part legs with feet. Posed every frame from the player's state:
//! walking, holding, winding up, throwing, reaching out to catch, knocked flat,
//! slimed to a wall, flipping, and a white flash when hit.

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
    Torso,
    Head,
    Visor,
    Limb,
    Hand,
}

/// Joint order inside `Rig::joints`.
const L_SHOULDER: usize = 0;
const L_ELBOW: usize = 1;
const R_SHOULDER: usize = 2;
const R_ELBOW: usize = 3;
const L_HIP: usize = 4;
const L_KNEE: usize = 5;
const R_HIP: usize = 6;
const R_KNEE: usize = 7;

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
    joints: [Entity; 8],
    parts: Vec<(Entity, Part)>,
    anim: Anim,
}

/// Extra local-only inputs: throw charge and the trick we are doing right now.
#[derive(Resource, Default)]
pub struct LocalRigInput {
    pub charge: Option<f32>,
    pub trick: Option<TrickState>,
}

#[derive(Resource)]
pub struct RigAssets {
    torso: Handle<Mesh>,
    head: Handle<Mesh>,
    visor: Handle<Mesh>,
    upper_arm: Handle<Mesh>,
    forearm: Handle<Mesh>,
    hand: Handle<Mesh>,
    thigh: Handle<Mesh>,
    shin: Handle<Mesh>,
    foot: Handle<Mesh>,
    torso_mat: [Handle<StandardMaterial>; 2],
    limb_mat: [Handle<StandardMaterial>; 2],
    skin: Handle<StandardMaterial>,
    visor_mat: Handle<StandardMaterial>,
    visor_catch: Handle<StandardMaterial>,
    slimed: Handle<StandardMaterial>,
    flash: Handle<StandardMaterial>,
}

pub fn make_rig_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let glow = |m: &mut Assets<StandardMaterial>, c: Color, e: LinearRgba| {
        m.add(StandardMaterial { base_color: c, emissive: e, ..default() })
    };
    commands.insert_resource(RigAssets {
        torso: meshes.add(Cuboid::new(0.46, 0.55, 0.26)),
        head: meshes.add(Sphere::new(0.19)),
        visor: meshes.add(Cuboid::new(0.28, 0.07, 0.06)),
        upper_arm: meshes.add(Cuboid::new(0.12, 0.30, 0.12)),
        forearm: meshes.add(Cuboid::new(0.10, 0.28, 0.10)),
        hand: meshes.add(Sphere::new(0.075)),
        thigh: meshes.add(Cuboid::new(0.15, 0.42, 0.15)),
        shin: meshes.add(Cuboid::new(0.13, 0.40, 0.13)),
        foot: meshes.add(Cuboid::new(0.14, 0.07, 0.24)),
        torso_mat: [materials.add(Color::srgb(0.90, 0.25, 0.22)), materials.add(Color::srgb(0.22, 0.45, 0.95))],
        limb_mat: [materials.add(Color::srgb(0.62, 0.16, 0.14)), materials.add(Color::srgb(0.14, 0.28, 0.62))],
        skin: materials.add(Color::srgb(0.93, 0.78, 0.62)),
        visor_mat: materials.add(Color::srgb(0.08, 0.08, 0.1)),
        visor_catch: glow(&mut materials, Color::srgb(1.0, 1.0, 0.6), LinearRgba::rgb(4.0, 4.0, 1.5)),
        slimed: glow(&mut materials, Color::srgb(0.35, 0.85, 0.3), LinearRgba::rgb(0.2, 1.2, 0.15)),
        flash: glow(&mut materials, Color::WHITE, LinearRgba::rgb(8.0, 8.0, 8.0)),
    });
}

/// Build a rig under `parent`. The rig's origin is at the feet; `offset` places it.
pub fn spawn_rig(commands: &mut Commands, a: &RigAssets, parent: Entity, offset: Vec3, player: Option<PlayerId>) -> Entity {
    let mut parts = Vec::new();
    let root = commands.spawn((Transform::from_translation(offset), Visibility::default(), ChildOf(parent))).id();
    let flip = commands.spawn((Transform::from_xyz(0.0, PIVOT, 0.0), Visibility::default(), ChildOf(root))).id();

    let mut part = |c: &mut Commands, mesh: &Handle<Mesh>, at: Vec3, parent: Entity, kind: Part| {
        let e = c
            .spawn((Mesh3d(mesh.clone()), MeshMaterial3d(a.skin.clone()), Transform::from_translation(at), ChildOf(parent)))
            .id();
        parts.push((e, kind));
    };
    part(commands, &a.torso, Vec3::new(0.0, 0.30, 0.0), flip, Part::Torso);
    part(commands, &a.head, Vec3::new(0.0, 0.72, 0.0), flip, Part::Head);
    part(commands, &a.visor, Vec3::new(0.0, 0.74, -0.17), flip, Part::Visor);

    let joint = |c: &mut Commands, at: Vec3, parent: Entity| {
        c.spawn((Transform::from_translation(at), Visibility::default(), ChildOf(parent))).id()
    };
    let mut joints = [Entity::PLACEHOLDER; 8];
    for (side, x) in [(0usize, -0.31_f32), (1, 0.31)] {
        let shoulder = joint(commands, Vec3::new(x, 0.52, 0.0), flip);
        part(commands, &a.upper_arm, Vec3::new(0.0, -0.15, 0.0), shoulder, Part::Limb);
        let elbow = joint(commands, Vec3::new(0.0, -0.30, 0.0), shoulder);
        part(commands, &a.forearm, Vec3::new(0.0, -0.14, 0.0), elbow, Part::Limb);
        part(commands, &a.hand, Vec3::new(0.0, -0.31, 0.0), elbow, Part::Hand);
        joints[side * 2] = shoulder;
        joints[side * 2 + 1] = elbow;
    }
    for (side, x) in [(0usize, -0.12_f32), (1, 0.12)] {
        let hip = joint(commands, Vec3::new(x, -0.02, 0.0), flip);
        part(commands, &a.thigh, Vec3::new(0.0, -0.21, 0.0), hip, Part::Limb);
        let knee = joint(commands, Vec3::new(0.0, -0.42, 0.0), hip);
        part(commands, &a.shin, Vec3::new(0.0, -0.20, 0.0), knee, Part::Limb);
        part(commands, &a.foot, Vec3::new(0.0, -0.43, -0.05), knee, Part::Limb);
        joints[4 + side * 2] = hip;
        joints[4 + side * 2 + 1] = knee;
    }
    commands.entity(root).insert(Rig { player, flip, joints, parts, anim: Anim::default() });
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
}

/// Joint angles: (pitch about X, roll about Z). Positive shoulder pitch raises the arm forward;
/// negative knee pitch bends the knee.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    pub joints: [(f32, f32); 8],
    /// Rotation of the whole body about X (flips, lying down).
    pub body_pitch: f32,
    /// How far the pivot drops (lying on the floor).
    pub drop: f32,
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

pub fn pose(i: &PoseInput) -> Pose {
    let mut p = Pose::default();
    let swing = i.walk_phase.sin() * i.stride;
    let j = &mut p.joints;

    // Walk cycle: legs swing opposite, arms opposite to legs, knees bend on the back swing.
    j[L_HIP] = (swing * 0.7, 0.0);
    j[R_HIP] = (-swing * 0.7, 0.0);
    j[L_KNEE] = (-(i.walk_phase.cos().max(0.0)) * 0.9 * i.stride, 0.0);
    j[R_KNEE] = (-((-i.walk_phase.cos()).max(0.0)) * 0.9 * i.stride, 0.0);
    j[L_SHOULDER] = (-swing * 0.6 + 0.1, -0.12);
    j[R_SHOULDER] = (swing * 0.6 + 0.1, 0.12);
    j[L_ELBOW] = (0.3, 0.0);
    j[R_ELBOW] = (0.3, 0.0);

    // Right arm: hold the ball out front, cock it back while charging, swing through on release.
    if i.throw_t < THROW_SECS {
        let t = i.throw_t / THROW_SECS;
        j[R_SHOULDER] = (lerp(-2.5, 1.6, t), 0.1);
        j[R_ELBOW] = (lerp(1.2, 0.1, t), 0.0);
        j[L_SHOULDER] = (0.8, -0.2);
    } else if i.holding {
        match i.charge {
            Some(c) => {
                j[R_SHOULDER] = (lerp(1.1, -2.5, c), 0.15);
                j[R_ELBOW] = (lerp(0.6, 1.2, c), 0.0);
                j[L_SHOULDER] = (lerp(0.2, 1.2, c), -0.15);
            }
            None => {
                j[R_SHOULDER] = (1.1, 0.1);
                j[R_ELBOW] = (0.6, 0.0);
            }
        }
    }

    // Reaching out to catch: both arms straight forward, slightly apart.
    if i.catching {
        j[L_SHOULDER] = (1.5, -0.25);
        j[R_SHOULDER] = (1.5, 0.25);
        j[L_ELBOW] = (0.15, 0.0);
        j[R_ELBOW] = (0.15, 0.0);
    }

    // Slimed to a wall: spread-eagle.
    if i.slimed {
        j[L_SHOULDER] = (0.0, -1.4);
        j[R_SHOULDER] = (0.0, 1.4);
        j[L_HIP] = (0.0, -0.3);
        j[R_HIP] = (0.0, 0.3);
        j[L_KNEE] = (0.0, 0.0);
        j[R_KNEE] = (0.0, 0.0);
    }

    if let Some(t) = i.trick {
        let tuck = (t.progress * PI).sin();
        match t.kind {
            TrickKind::FrontFlip => p.body_pitch = -TAU * t.progress,
            TrickKind::BackFlip => p.body_pitch = TAU * t.progress,
            TrickKind::SpinLeft | TrickKind::SpinRight => {}
        }
        if matches!(t.kind, TrickKind::FrontFlip | TrickKind::BackFlip) {
            j[L_HIP] = (1.3 * tuck, 0.0);
            j[R_HIP] = (1.3 * tuck, 0.0);
            j[L_KNEE] = (-1.8 * tuck, 0.0);
            j[R_KNEE] = (-1.8 * tuck, 0.0);
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
        j[L_HIP] = (0.1, -0.15);
        j[R_HIP] = (0.1, 0.15);
        j[L_KNEE] = (-0.3, 0.0);
        j[R_KNEE] = (-0.2, 0.0);
    }
    p
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

        let team = match pv.team {
            Team::Red => 0,
            Team::Blue => 1,
        };
        let flashing = a.flash > 0.0;
        let slimed = pv.slimed > 0.0;
        for &(e, kind) in &r.parts {
            let Ok(mut m) = mats.get_mut(e) else { continue };
            m.0 = if flashing {
                assets.flash.clone()
            } else {
                match kind {
                    Part::Visor if pv.catching => assets.visor_catch.clone(),
                    Part::Visor => assets.visor_mat.clone(),
                    Part::Torso | Part::Limb if slimed => assets.slimed.clone(),
                    Part::Torso => assets.torso_mat[team].clone(),
                    Part::Limb => assets.limb_mat[team].clone(),
                    Part::Head | Part::Hand => assets.skin.clone(),
                }
            };
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
    fn catching_reaches_both_arms_forward() {
        let p = pose(&PoseInput { catching: true, ..idle() });
        assert!(p.joints[L_SHOULDER].0 > 1.3 && p.joints[R_SHOULDER].0 > 1.3);
        assert!(p.joints[L_ELBOW].0 < 0.3, "arms straight");
    }

    #[test]
    fn charging_cocks_the_right_arm_back() {
        let hold = pose(&PoseInput { holding: true, ..idle() });
        let wound = pose(&PoseInput { holding: true, charge: Some(1.0), ..idle() });
        assert!(hold.joints[R_SHOULDER].0 > 1.0, "ball held out front");
        assert!(wound.joints[R_SHOULDER].0 < -2.0, "arm up and back");
    }

    #[test]
    fn throw_swings_through() {
        let start = pose(&PoseInput { throw_t: 0.0, ..idle() });
        let end = pose(&PoseInput { throw_t: THROW_SECS * 0.99, ..idle() });
        assert!(start.joints[R_SHOULDER].0 < 0.0 && end.joints[R_SHOULDER].0 > 1.0);
    }

    #[test]
    fn walking_swings_legs_opposite() {
        let p = pose(&PoseInput { walk_phase: FRAC_PI_2, stride: 1.0, ..idle() });
        assert!(p.joints[L_HIP].0 > 0.5 && p.joints[R_HIP].0 < -0.5);
        let still = pose(&idle());
        assert_eq!(still.joints[L_HIP].0, 0.0);
    }

    #[test]
    fn flips_rotate_opposite_ways_and_tuck() {
        let t = |kind| Some(TrickState { kind, progress: 0.25 });
        let front = pose(&PoseInput { trick: t(TrickKind::FrontFlip), ..idle() });
        let back = pose(&PoseInput { trick: t(TrickKind::BackFlip), ..idle() });
        assert!(front.body_pitch < 0.0 && back.body_pitch > 0.0);
        assert!(front.joints[L_KNEE].0 < -1.0, "knees tucked");
        let spin = pose(&PoseInput { trick: t(TrickKind::SpinLeft), ..idle() });
        assert_eq!(spin.body_pitch, 0.0, "spins turn the player, not the rig");
    }

    #[test]
    fn stunned_lies_flat_on_the_floor() {
        let p = pose(&PoseInput { stunned: true, trick: Some(TrickState { kind: TrickKind::FrontFlip, progress: 0.5 }), ..idle() });
        assert!((p.body_pitch - FRAC_PI_2).abs() < 1e-6, "stun overrides tricks");
        assert!(PIVOT - p.drop < 0.3);
    }
}
