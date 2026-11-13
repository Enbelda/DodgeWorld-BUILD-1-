//! Parkour for every character (no mount needed): wall run, wall climb, wall-to-wall jump,
//! ledge hang (shimmy, climb up, jump off, let go) and vaulting / mounting onto cover.
//!
//! Pure logic over the arena's boxes so it can be tested without Bevy or Rapier. The local
//! player's movement system feeds it input each frame and does what it says (`Drive`).
//!
//! - Wall run: in the air next to a tall wall, moving along it. Runs along the wall for up
//!   to 1.3 s with gravity much reduced. Jump to kick off the wall (wall jump); kick off
//!   toward another wall to chain wall runs (wall-to-wall). Each wall once per landing.
//! - Wall climb: in the air, pushing straight into a wall too tall to vault: run up it for
//!   up to 0.5 s. Reaching the top edge grabs the ledge.
//! - Ledge hang: in the air near a ledge at hand height (falling or climbing): hang from it.
//!   A/D shimmy, Space climbs up onto the top (wall mount), S+Space jumps off backward,
//!   Ctrl lets go. You can still throw and catch while hanging.
//! - Vault: in the air pushing into a ledge at or below chest height: climb straight onto it.

use bevy::math::Vec3;

use crate::sim::layout::Aabb;
use crate::sim::{ParkourPose, BODY_RADIUS};

/// How far beyond the body's radius a wall still counts as touching.
pub const WALL_REACH: f32 = 0.3;
/// Wall runs need a wall reaching at least this far above the feet.
pub const WALLRUN_MIN_TOP: f32 = 1.8;
pub const WALLRUN_SPEED: f32 = 9.5;
/// Upward speed a wall run starts with at least (even if falling), and the reduced gravity
/// during it.
pub const WALLRUN_LIFT: f32 = 3.0;
pub const WALLRUN_GRAVITY: f32 = 6.0;
/// A wall run entered while still rising fast keeps at most this much of it.
pub const WALLRUN_MAX_RISE: f32 = 4.5;
pub const WALLRUN_SECS: f32 = 1.3;
/// Kicking off a wall: up speed and sideways push away from it.
pub const WALL_JUMP_UP: f32 = 8.0;
pub const WALL_JUMP_PUSH: f32 = 9.0;
pub const CLIMB_SPEED: f32 = 7.0;
pub const CLIMB_SECS: f32 = 0.5;
/// Ledge height above the feet the hands can grab: from chest-high to arms' reach.
pub const HANG_MIN: f32 = 1.4;
pub const HANG_REACH: f32 = 2.05;
/// While hanging the feet are this far below the ledge.
pub const HANG_DROP: f32 = 2.0;
/// Ledges up to this high above the feet are vaulted onto directly.
pub const VAULT_REACH: f32 = 1.4;
pub const SHIMMY_SPEED: f32 = 2.2;
pub const MANTLE_SECS: f32 = 0.35;
/// Wall-jump push fades by this rate per second while airborne.
pub const PUSH_DECAY: f32 = 2.0;
/// After letting go, the same ledge can't be grabbed again for this long.
pub const LET_GO_SECS: f32 = 0.5;
/// The top of a box must be at least this deep to stand on.
const MIN_TOP_DEPTH: f32 = 0.5;

/// A wall face next to the player.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Contact {
    /// Index into the solids list.
    pub wall: usize,
    /// Points out of the wall, toward the player (horizontal, axis-aligned).
    pub normal: Vec3,
    /// Closest point of the face, at the feet's height.
    pub face: Vec3,
    pub top: f32,
    pub dist: f32,
    /// How deep the box is behind the face (room to stand on top).
    pub depth: f32,
    /// The face's extent along its own axis (X or Z), as (min, max) coordinates.
    pub span: (f32, f32),
}

impl Contact {
    /// Horizontal direction along the face.
    pub fn along(&self) -> Vec3 {
        Vec3::Y.cross(self.normal)
    }
    /// Height of the top edge above `feet`.
    pub fn height_above(&self, feet: Vec3) -> f32 {
        self.top - feet.y
    }
}

/// The face of box `b` the body at `feet` is touching (within `reach` past its radius).
pub fn contact(feet: Vec3, b: &Aabb, wall: usize, reach: f32) -> Option<Contact> {
    if b.max.y < feet.y + 0.05 || b.min.y > feet.y + 1.5 {
        return None; // not beside the body
    }
    let q = Vec3::new(feet.x.clamp(b.min.x, b.max.x), feet.y, feet.z.clamp(b.min.z, b.max.z));
    let (dx, dz) = (feet.x - q.x, feet.z - q.z);
    // Off one face, not a corner (and not inside).
    let (normal, depth, span) = if dz == 0.0 && dx != 0.0 {
        (Vec3::X * dx.signum(), b.max.x - b.min.x, (b.min.z, b.max.z))
    } else if dx == 0.0 && dz != 0.0 {
        (Vec3::Z * dz.signum(), b.max.z - b.min.z, (b.min.x, b.max.x))
    } else {
        return None;
    };
    let dist = dx.abs().max(dz.abs());
    (dist <= BODY_RADIUS + reach).then_some(Contact { wall, normal, face: q, top: b.max.y, dist, depth, span })
}

/// All faces the body is touching, nearest first.
pub fn contacts(feet: Vec3, solids: &[Aabb], reach: f32) -> Vec<Contact> {
    let mut v: Vec<Contact> = solids.iter().enumerate().filter_map(|(i, b)| contact(feet, b, i, reach)).collect();
    v.sort_by(|a, b| a.dist.total_cmp(&b.dist));
    v
}

/// What the player is doing.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum State {
    #[default]
    Free,
    WallRun { wall: usize, normal: Vec3, dir: Vec3, t: f32 },
    Climb { wall: usize, normal: Vec3, t: f32 },
    Hang { wall: usize, normal: Vec3 },
    Mantle { from: Vec3, to: Vec3, t: f32 },
}

/// Vertical speed, wall-jump push and air jumps, shared with normal movement.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Motion {
    pub vel_y: f32,
    /// Horizontal momentum from wall jumps.
    pub push: Vec3,
    pub air_jumps: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Input {
    /// World-space move direction (unit or zero).
    pub dir: Vec3,
    /// Where the camera looks, flattened.
    pub forward: Vec3,
    pub jump: bool,
    /// Let go of a ledge (Ctrl).
    pub drop: bool,
    pub grounded: bool,
}

/// What the movement system should do this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Drive {
    /// Normal running/jumping/falling (parkour may have set vertical speed and push).
    Normal,
    /// Move by this much through the character controller.
    Translate(Vec3),
    /// Put the feet exactly here.
    Place(Vec3),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Out {
    pub drive: Drive,
    /// The jump press was used by parkour (don't also do a normal jump).
    pub used_jump: bool,
    /// Something started this frame (for logs and play-tests).
    pub event: Option<&'static str>,
}

impl Out {
    fn normal() -> Self {
        Out { drive: Drive::Normal, used_jump: false, event: None }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Parkour {
    pub state: State,
    /// Walls already run on / climbed since the last landing.
    used_run: Option<usize>,
    used_climb: Option<usize>,
    /// A ledge just let go of, and for how much longer it can't be grabbed.
    ignore: Option<(usize, f32)>,
}

impl Parkour {
    /// Forget everything (respawn, slimed, knocked).
    pub fn reset(&mut self) {
        *self = Parkour::default();
    }

    /// For animation and the network.
    pub fn pose(&self) -> ParkourPose {
        match self.state {
            State::Free => ParkourPose::None,
            State::WallRun { normal, dir, .. } => {
                // Wall on the left when its normal points to our right.
                let right = dir.cross(Vec3::Y);
                if normal.dot(right) > 0.0 {
                    ParkourPose::WallRunLeft
                } else {
                    ParkourPose::WallRunRight
                }
            }
            State::Climb { .. } => ParkourPose::Climb,
            State::Hang { .. } => ParkourPose::Hang,
            State::Mantle { .. } => ParkourPose::Mantle,
        }
    }

    /// The way the body should face during parkour (`None` = free to face the camera).
    pub fn facing(&self) -> Option<Vec3> {
        match self.state {
            State::Free => None,
            State::WallRun { dir, .. } => Some(dir),
            State::Climb { normal, .. } | State::Hang { normal, .. } => Some(-normal),
            State::Mantle { from, to, .. } => Some(Vec3::new(to.x - from.x, 0.0, to.z - from.z).normalize_or(Vec3::NEG_Z)),
        }
    }

    pub fn is_free(&self) -> bool {
        self.state == State::Free
    }

    fn ignored(&self, wall: usize) -> bool {
        self.ignore.is_some_and(|(w, _)| w == wall)
    }

    pub fn update(&mut self, m: &mut Motion, i: &Input, feet: Vec3, dt: f32, solids: &[Aabb]) -> Out {
        if let Some((_, t)) = self.ignore.as_mut() {
            *t -= dt;
            if *t <= 0.0 {
                self.ignore = None;
            }
        }
        let attached = matches!(self.state, State::Hang { .. } | State::Mantle { .. });
        if i.grounded && !attached {
            self.used_run = None;
            self.used_climb = None;
            m.push = Vec3::ZERO;
            self.state = State::Free;
        }
        match self.state {
            State::Free if i.grounded => Out::normal(),
            State::Free => self.start(m, i, feet, solids),
            State::WallRun { wall, normal, dir, t } => self.wall_run(m, i, feet, dt, solids, (wall, normal, dir, t)),
            State::Climb { wall, normal, t } => self.climb(m, i, feet, dt, solids, (wall, normal, t)),
            State::Hang { wall, normal } => self.hang(m, i, feet, dt, solids, wall, normal),
            State::Mantle { from, to, t } => self.mantle(m, dt, from, to, t),
        }
    }

    /// In the air: vault, grab a ledge, climb, or start a wall run (in that order).
    fn start(&mut self, m: &mut Motion, i: &Input, feet: Vec3, solids: &[Aabb]) -> Out {
        for c in contacts(feet, solids, WALL_REACH) {
            if self.ignored(c.wall) {
                continue;
            }
            let h = c.height_above(feet);
            let into = i.dir.dot(-c.normal);
            let facing = i.forward.dot(-c.normal);
            if into > 0.6 && h > 0.05 && h <= VAULT_REACH && c.depth >= MIN_TOP_DEPTH {
                return self.begin_mantle(m, feet, &c, "vault");
            }
            if (HANG_MIN..=HANG_REACH).contains(&h) && m.vel_y <= 0.5 && (facing > 0.3 || into > 0.3) {
                return self.begin_hang(m, feet, &c);
            }
            if into > 0.7 && h > HANG_REACH && self.used_climb != Some(c.wall) && m.vel_y > -3.0 {
                self.used_climb = Some(c.wall);
                self.state = State::Climb { wall: c.wall, normal: c.normal, t: 0.0 };
                m.push = Vec3::ZERO;
                return Out { event: Some("wall climb"), ..Out::normal() };
            }
            let along = i.dir.dot(c.along());
            if h >= WALLRUN_MIN_TOP && self.used_run != Some(c.wall) && along.abs() >= 0.5 && i.dir.dot(c.normal) < 0.3 {
                self.used_run = Some(c.wall);
                let dir = c.along() * along.signum();
                self.state = State::WallRun { wall: c.wall, normal: c.normal, dir, t: 0.0 };
                m.vel_y = m.vel_y.clamp(WALLRUN_LIFT, WALLRUN_MAX_RISE);
                m.push = Vec3::ZERO;
                m.air_jumps = crate::player::AIR_JUMPS;
                return Out { event: Some("wall run"), ..Out::normal() };
            }
        }
        Out::normal()
    }

    fn begin_hang(&mut self, m: &mut Motion, feet: Vec3, c: &Contact) -> Out {
        self.state = State::Hang { wall: c.wall, normal: c.normal };
        *m = Motion { vel_y: 0.0, push: Vec3::ZERO, air_jumps: crate::player::AIR_JUMPS };
        Out { drive: Drive::Place(hang_point(feet, c)), used_jump: false, event: Some("ledge hang") }
    }

    fn begin_mantle(&mut self, m: &mut Motion, feet: Vec3, c: &Contact, event: &'static str) -> Out {
        let to = mantle_target(feet, c);
        self.state = State::Mantle { from: feet, to, t: 0.0 };
        *m = Motion { vel_y: 0.0, push: Vec3::ZERO, air_jumps: crate::player::AIR_JUMPS };
        Out { drive: Drive::Place(feet), used_jump: false, event: Some(event) }
    }

    /// Kick off the wall with `normal`: up and away, keeping some of `carry`.
    fn wall_jump(&mut self, m: &mut Motion, normal: Vec3, carry: Vec3) -> Out {
        m.vel_y = WALL_JUMP_UP;
        m.push = normal * WALL_JUMP_PUSH + carry;
        m.air_jumps = crate::player::AIR_JUMPS;
        self.state = State::Free;
        Out { drive: Drive::Normal, used_jump: true, event: Some("wall jump") }
    }

    fn wall_run(
        &mut self,
        m: &mut Motion,
        i: &Input,
        feet: Vec3,
        dt: f32,
        solids: &[Aabb],
        (wall, normal, dir, t): (usize, Vec3, Vec3, f32),
    ) -> Out {
        if i.jump {
            return self.wall_jump(m, normal, dir * WALLRUN_SPEED * 0.5);
        }
        let still_there = contact(feet, &solids[wall], wall, WALL_REACH + 0.2).is_some_and(|c| c.normal == normal);
        if !still_there || t >= WALLRUN_SECS || i.dir.dot(normal) > 0.5 {
            self.state = State::Free;
            return Out::normal();
        }
        m.vel_y -= WALLRUN_GRAVITY * dt;
        self.state = State::WallRun { wall, normal, dir, t: t + dt };
        let step = dir * WALLRUN_SPEED * dt - normal * 0.3 * dt + Vec3::Y * m.vel_y * dt;
        Out { drive: Drive::Translate(step), used_jump: false, event: None }
    }

    fn climb(
        &mut self,
        m: &mut Motion,
        i: &Input,
        feet: Vec3,
        dt: f32,
        solids: &[Aabb],
        (wall, normal, t): (usize, Vec3, f32),
    ) -> Out {
        if i.jump {
            return self.wall_jump(m, normal, Vec3::ZERO);
        }
        let Some(c) = contact(feet, &solids[wall], wall, WALL_REACH + 0.2).filter(|c| c.normal == normal) else {
            self.state = State::Free;
            return Out::normal();
        };
        if c.height_above(feet) <= HANG_REACH {
            return self.begin_hang(m, feet, &c);
        }
        if t >= CLIMB_SECS || i.dir.dot(-normal) < 0.3 {
            self.state = State::Free;
            m.vel_y = m.vel_y.min(1.0);
            return Out::normal();
        }
        m.vel_y = CLIMB_SPEED;
        self.state = State::Climb { wall, normal, t: t + dt };
        Out { drive: Drive::Translate(Vec3::Y * CLIMB_SPEED * dt - normal * 0.3 * dt), used_jump: false, event: None }
    }

    #[allow(clippy::too_many_arguments)]
    fn hang(&mut self, m: &mut Motion, i: &Input, feet: Vec3, dt: f32, solids: &[Aabb], wall: usize, normal: Vec3) -> Out {
        let Some(c) = contact(feet, &solids[wall], wall, WALL_REACH + 0.3).filter(|c| c.normal == normal) else {
            self.state = State::Free;
            return Out { event: Some("lost the ledge"), ..Out::normal() };
        };
        *m = Motion { vel_y: 0.0, push: Vec3::ZERO, air_jumps: crate::player::AIR_JUMPS };
        if i.drop {
            self.state = State::Free;
            self.ignore = Some((wall, LET_GO_SECS));
            return Out { event: Some("let go"), ..Out::normal() };
        }
        if i.jump {
            if i.dir.dot(normal) > 0.5 || c.depth < MIN_TOP_DEPTH {
                self.ignore = Some((wall, LET_GO_SECS));
                return self.wall_jump(m, normal, Vec3::ZERO);
            }
            let out = self.begin_mantle(m, feet, &c, "climb up");
            return Out { used_jump: true, ..out };
        }
        // Shimmy along the ledge, staying on it.
        let along = c.along();
        let moved = feet + along * i.dir.dot(along) * SHIMMY_SPEED * dt;
        let edge = BODY_RADIUS * 0.6;
        let (lo, hi) = (c.span.0 + edge, c.span.1 - edge);
        let moved = if normal.x != 0.0 {
            Vec3::new(moved.x, moved.y, moved.z.clamp(lo, hi.max(lo)))
        } else {
            Vec3::new(moved.x.clamp(lo, hi.max(lo)), moved.y, moved.z)
        };
        Out { drive: Drive::Place(hang_point(moved, &c)), used_jump: false, event: None }
    }

    fn mantle(&mut self, m: &mut Motion, dt: f32, from: Vec3, to: Vec3, t: f32) -> Out {
        let t = (t + dt / MANTLE_SECS).min(1.0);
        *m = Motion { vel_y: 0.0, push: Vec3::ZERO, air_jumps: crate::player::AIR_JUMPS };
        let at = mantle_point(from, to, t);
        self.state = if t >= 1.0 { State::Free } else { State::Mantle { from, to, t } };
        Out { drive: Drive::Place(at), used_jump: false, event: None }
    }
}

/// Where the feet go while hanging from `c` (horizontally where `feet` is along the face).
fn hang_point(feet: Vec3, c: &Contact) -> Vec3 {
    let on_face = if c.normal.x != 0.0 { Vec3::new(c.face.x, 0.0, feet.z) } else { Vec3::new(feet.x, 0.0, c.face.z) };
    on_face + c.normal * (BODY_RADIUS + 0.03) + Vec3::Y * (c.top - HANG_DROP)
}

/// Standing spot on top of the box behind face `c`.
fn mantle_target(feet: Vec3, c: &Contact) -> Vec3 {
    let inset = (c.depth / 2.0).min(0.6);
    let on_face = if c.normal.x != 0.0 { Vec3::new(c.face.x, 0.0, feet.z) } else { Vec3::new(feet.x, 0.0, c.face.z) };
    on_face - c.normal * inset + Vec3::Y * (c.top + 0.03)
}

/// Mantle path: up to the ledge first, then over it.
pub fn mantle_point(from: Vec3, to: Vec3, t: f32) -> Vec3 {
    const UP: f32 = 0.55;
    if t < UP {
        let k = t / UP;
        Vec3::new(from.x, from.y + (to.y - from.y) * k, from.z)
    } else {
        let k = (t - UP) / (1.0 - UP);
        Vec3::new(from.x + (to.x - from.x) * k, to.y, from.z + (to.z - from.z) * k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::layout;

    const DT: f32 = 1.0 / 60.0;
    const GRAVITY: f32 = 22.0;
    const RUN: f32 = 7.0;

    /// A tiny stand-in for the character controller: gravity, a floor at y=0, box tops to
    /// land on, and walls that stop horizontal movement.
    struct Sim {
        pk: Parkour,
        m: Motion,
        feet: Vec3,
        solids: Vec<Aabb>,
        events: Vec<&'static str>,
    }

    impl Sim {
        fn new(feet: Vec3) -> Self {
            Sim { pk: Parkour::default(), m: Motion::default(), feet, solids: layout::solids(), events: vec![] }
        }

        fn ground_under(&self, p: Vec3) -> f32 {
            self.solids
                .iter()
                .filter(|b| {
                    p.x > b.min.x - 0.1 && p.x < b.max.x + 0.1 && p.z > b.min.z - 0.1 && p.z < b.max.z + 0.1 && b.max.y <= p.y + 0.05
                })
                .map(|b| b.max.y)
                .fold(0.0, f32::max)
        }

        fn grounded(&self) -> bool {
            self.feet.y <= self.ground_under(self.feet) + 0.01 && self.m.vel_y <= 0.0
        }

        /// Move without entering any box (sideways blocked by walls).
        fn shove(&mut self, d: Vec3) {
            let mut p = self.feet + d;
            let ground = self.ground_under(self.feet);
            if p.y < ground {
                p.y = ground;
                self.m.vel_y = self.m.vel_y.max(0.0);
            }
            let blocked = self.solids.iter().any(|b| {
                let e = b.expanded(BODY_RADIUS);
                p.x > e.min.x && p.x < e.max.x && p.z > e.min.z && p.z < e.max.z && p.y + 0.05 < b.max.y && p.y + 1.8 > b.min.y
            });
            if blocked {
                p.x = self.feet.x;
                p.z = self.feet.z;
            }
            self.feet = p;
        }

        fn frame(&mut self, dir: Vec3, forward: Vec3, jump: bool, drop: bool) {
            let grounded = self.grounded();
            let i = Input { dir: dir.normalize_or_zero(), forward, jump, drop, grounded };
            let out = self.pk.update(&mut self.m, &i, self.feet, DT, &self.solids);
            if let Some(e) = out.event {
                self.events.push(e);
            }
            match out.drive {
                Drive::Place(p) => self.feet = p,
                Drive::Translate(d) => self.shove(d),
                Drive::Normal => {
                    if jump && !out.used_jump && grounded {
                        self.m.vel_y = 8.0;
                    }
                    if !grounded || self.m.vel_y > 0.0 {
                        self.m.vel_y -= GRAVITY * DT;
                    }
                    let flat = i.dir * RUN + self.m.push;
                    self.m.push *= (-PUSH_DECAY * DT).exp();
                    self.shove(Vec3::new(flat.x, self.m.vel_y, flat.z) * DT);
                }
            }
        }

        fn run(&mut self, secs: f32, dir: Vec3, forward: Vec3) {
            for _ in 0..(secs / DT) as usize {
                self.frame(dir, forward, false, false);
            }
        }
    }

    /// The tall panel on the +X flank (x 15.2..15.8, z -4..4, 5.5 m high).
    fn panel() -> Aabb {
        layout::solids().into_iter().find(|b| b.min.x > 14.0 && b.max.y > 4.0 && b.max.x < 17.0).expect("panel")
    }

    #[test]
    fn contact_finds_the_face_not_corners() {
        let b = Aabb::from_center(Vec3::new(0.0, 1.0, 0.0), Vec3::new(1.0, 1.0, 1.0));
        let c = contact(Vec3::new(1.6, 0.5, 0.2), &b, 3, WALL_REACH).unwrap();
        assert_eq!(c.normal, Vec3::X);
        assert_eq!(c.wall, 3);
        assert!((c.dist - 0.6).abs() < 1e-5);
        assert!(contact(Vec3::new(1.6, 0.5, 1.6), &b, 3, WALL_REACH).is_none(), "corner");
        assert!(contact(Vec3::new(2.0, 0.5, 0.0), &b, 3, WALL_REACH).is_none(), "too far");
        assert!(contact(Vec3::new(1.5, 2.5, 0.0), &b, 3, WALL_REACH).is_none(), "above it");
    }

    #[test]
    fn wall_run_along_the_outer_wall_then_wall_jump_to_the_panel() {
        // In the corridor between the panel (x<=15.8) and the +X outer wall (x=20).
        let mut s = Sim::new(Vec3::new(19.2, 0.0, 9.0));
        let along_wall = Vec3::new(0.25, 0.0, -1.0);
        s.frame(along_wall, Vec3::NEG_Z, true, false); // jump
        s.run(0.2, along_wall, Vec3::NEG_Z);
        assert!(matches!(s.pk.state, State::WallRun { normal, .. } if normal == Vec3::NEG_X), "{:?}", s.pk.state);
        assert_eq!(s.pk.pose(), ParkourPose::WallRunRight, "outer wall is on our right running -Z");
        let start = s.feet;
        s.run(0.4, along_wall, Vec3::NEG_Z);
        assert!(start.z - s.feet.z > 3.0, "ran along the wall: {start} -> {}", s.feet);
        assert!(s.feet.y > 1.0, "still up on the wall: {}", s.feet.y);
        // Kick off toward the panel and keep heading along the corridor: lands on the panel.
        s.frame(along_wall, Vec3::NEG_Z, true, false);
        assert!(s.events.contains(&"wall jump"));
        assert!(s.m.push.x < -5.0, "pushed away from the outer wall");
        let to_panel = Vec3::new(-0.25, 0.0, -1.0);
        for _ in 0..60 {
            s.frame(to_panel, Vec3::NEG_Z, false, false);
            if matches!(s.pk.state, State::WallRun { normal, .. } if normal == Vec3::X) {
                break;
            }
        }
        assert!(
            matches!(s.pk.state, State::WallRun { normal, .. } if normal == Vec3::X),
            "wall-to-wall: now running on the panel, {:?} at {}",
            s.pk.state,
            s.feet
        );
    }

    #[test]
    fn same_wall_only_once_per_landing() {
        let mut s = Sim::new(Vec3::new(19.2, 0.0, 9.0));
        let along = Vec3::new(0.25, 0.0, -1.0);
        s.frame(along, Vec3::NEG_Z, true, false);
        s.run(1.6, along, Vec3::NEG_Z); // runs out of wall-run time
        let runs = s.events.iter().filter(|e| **e == "wall run").count();
        assert_eq!(runs, 1);
        // Landed: can wall run again.
        s.run(1.0, Vec3::ZERO, Vec3::NEG_Z);
        assert!(s.grounded());
        s.frame(along, Vec3::NEG_Z, true, false);
        s.run(0.2, along, Vec3::NEG_Z);
        assert_eq!(s.events.iter().filter(|e| **e == "wall run").count(), 2);
    }

    #[test]
    fn climb_the_panel_hang_then_climb_up_onto_it() {
        let p = panel();
        let mut s = Sim::new(Vec3::new(p.min.x - 0.6, 0.0, 0.0));
        let into = Vec3::X;
        s.frame(into, into, true, false);
        s.run(1.0, into, into);
        assert!(s.events.contains(&"wall climb"), "{:?}", s.events);
        assert!(s.events.contains(&"ledge hang"), "{:?}", s.events);
        assert!(matches!(s.pk.state, State::Hang { .. }));
        assert!((s.feet.y - (p.max.y - HANG_DROP)).abs() < 1e-4, "hanging below the ledge: {}", s.feet.y);
        assert_eq!(s.pk.pose(), ParkourPose::Hang);
        // Hanging holds still without input.
        let held = s.feet;
        s.run(0.5, Vec3::ZERO, into);
        assert_eq!(s.feet, held);
        // Space: climb up onto the top (wall mount).
        s.frame(into, into, true, false);
        s.run(MANTLE_SECS + 0.1, Vec3::ZERO, into);
        assert!(s.pk.is_free());
        assert!((s.feet.y - p.max.y).abs() < 0.1, "standing on top: {}", s.feet);
        assert!(s.feet.x > p.min.x && s.feet.x < p.max.x);
    }

    #[test]
    fn shimmy_stays_on_the_ledge_and_ctrl_lets_go() {
        let p = panel();
        let mut s = Sim::new(Vec3::new(p.min.x - 0.6, 0.0, 3.0));
        s.frame(Vec3::X, Vec3::X, true, false);
        s.run(1.0, Vec3::X, Vec3::X);
        assert!(matches!(s.pk.state, State::Hang { .. }));
        // Hold "right" (toward +Z when facing +X) for a long time: stops at the end.
        s.run(3.0, Vec3::Z, Vec3::X);
        assert!(matches!(s.pk.state, State::Hang { .. }));
        assert!(s.feet.z <= p.max.z - BODY_RADIUS * 0.6 + 1e-4 && s.feet.z > 3.5, "z {}", s.feet.z);
        s.frame(Vec3::ZERO, Vec3::X, false, true);
        assert!(s.pk.is_free());
        assert!(s.events.contains(&"let go"));
        s.run(1.0, Vec3::ZERO, Vec3::X);
        assert!(s.grounded() && s.feet.y < 0.01, "dropped to the floor");
    }

    #[test]
    fn jump_off_backward_from_a_hang() {
        let p = panel();
        let mut s = Sim::new(Vec3::new(p.min.x - 0.6, 0.0, 0.0));
        s.frame(Vec3::X, Vec3::X, true, false);
        s.run(1.0, Vec3::X, Vec3::X);
        assert!(matches!(s.pk.state, State::Hang { .. }));
        s.frame(Vec3::NEG_X, Vec3::X, true, false);
        assert!(s.events.contains(&"wall jump"));
        assert!(s.m.push.x < -5.0 && s.m.vel_y > 5.0);
    }

    #[test]
    fn vault_onto_the_low_wall() {
        // Low wall: x -3..3, z 6.6..7.4, 1.2 m high. Run at it from the -Z side and jump.
        let mut s = Sim::new(Vec3::new(0.0, 0.0, 5.6));
        let dir = Vec3::Z;
        s.frame(dir, dir, true, false);
        for _ in 0..60 {
            s.frame(dir, dir, false, false);
            if s.events.contains(&"vault") && s.pk.is_free() {
                break;
            }
        }
        assert!(s.events.contains(&"vault"), "{:?}", s.events);
        assert!((s.feet.y - 1.2).abs() < 0.1 && (6.6..7.4).contains(&s.feet.z), "standing on it: {}", s.feet);
    }

    #[test]
    fn grab_a_pillar_top_from_a_jump() {
        // Pillar at x=9: 3 m tall. Jumping at it from the floor: falling near the top, grab.
        let mut s = Sim::new(Vec3::new(7.1, 0.0, 0.0));
        let mut grabbed = false;
        s.frame(Vec3::ZERO, Vec3::X, true, false);
        for _ in 0..60 {
            // Face it but don't push in, so it's a grab not a climb.
            s.frame(Vec3::ZERO, Vec3::X, false, false);
            grabbed |= matches!(s.pk.state, State::Hang { .. });
        }
        assert!(!grabbed, "too far from the wall to grab");
        let mut s = Sim::new(Vec3::new(7.5, 0.0, 0.0));
        s.frame(Vec3::ZERO, Vec3::X, true, false);
        for _ in 0..60 {
            s.frame(Vec3::ZERO, Vec3::X, false, false);
            grabbed |= matches!(s.pk.state, State::Hang { .. });
        }
        assert!(grabbed, "{:?}", s.events);
    }

    #[test]
    fn mantle_rises_then_moves_over() {
        let from = Vec3::new(0.0, 1.0, 0.0);
        let to = Vec3::new(1.0, 3.0, 0.0);
        assert_eq!(mantle_point(from, to, 0.0), from);
        let mid = mantle_point(from, to, 0.55);
        assert!((mid.y - 3.0).abs() < 1e-4 && mid.x == 0.0, "up first");
        assert_eq!(mantle_point(from, to, 1.0), to);
    }
}
