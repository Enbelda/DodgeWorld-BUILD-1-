//! Riding a mount: momentum, carving, ollies, air tricks, grinds, combos and bails.
//!
//! Pure logic (no Bevy systems) so it can be tested; the local player's movement system
//! feeds it input and moves the body by the velocity it returns, or places the feet on a
//! rail while grinding.
//!
//! - On the ground: WASD (camera-relative) carves toward that direction and pushes; pulling
//!   the opposite way brakes; letting go coasts. Shift pushes harder (20% higher top speed).
//!   Space ollies.
//! - In the air: Q + keys = flip trick, E + keys = grab (hold E to keep it: bonus points),
//!   R + W / R + S = front / back flip. One trick at a time; chain several before landing.
//!   Landing a flip trick before 80% of it is done is a bail (knocked off balance for 1 s,
//!   combo lost).
//! - Holding F as you come down onto a rail or a ledge grinds it (F + keys picks the grind);
//!   Space jumps off, the end of the rail pops you off.
//! - Landing banks the combo: the sum of its tricks times how many there were (max x10).

use bevy::math::Vec3;

use crate::mounts::{Dir, MountDef, MountKind, TrickCat, TrickInput};
use crate::sim::layout::Rail;
use crate::sim::RideView;

pub const RIDE_GRAVITY: f32 = 22.0;
/// A flip must be this far through to be landed.
pub const LAND_OK: f32 = 0.8;
pub const BAIL_SECS: f32 = 1.0;
/// How close (sideways) and how high above / below a rail's top the feet can be to lock on.
const GRIND_REACH: f32 = 0.55;
const GRIND_ABOVE: f32 = 0.9;
const GRIND_BELOW: f32 = 0.3;
/// Holding a grab past its time: extra points per second, as a share of the trick's points.
const GRAB_BONUS: f32 = 0.5;
pub const MAX_MULTIPLIER: u32 = 10;
/// Shift: push harder, go faster.
const SPRINT_PUSH: f32 = 1.25;
const SPRINT_TOP: f32 = 1.2;
/// Coming off the end of a rail.
const RAIL_EXIT_POP: f32 = 5.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActiveTrick {
    pub cat: TrickCat,
    pub index: usize,
    /// Seconds into it.
    pub t: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grinding {
    pub rail: usize,
    /// Distance along the rail.
    pub s: f32,
    /// +1 riding from `a` toward `b`, -1 the other way.
    pub sign: f32,
    /// Which grind trick.
    pub index: usize,
    pub t: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Combo {
    pub names: Vec<String>,
    pub points: f32,
}

impl Combo {
    pub fn multiplier(&self) -> u32 {
        (self.names.len() as u32).clamp(1, MAX_MULTIPLIER)
    }
    pub fn total(&self) -> u32 {
        (self.points * self.multiplier() as f32).round() as u32
    }
}

/// Input for one frame of riding.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RideInput {
    /// Where WASD points in the world (camera-relative), unit or zero.
    pub dir: Vec3,
    /// The raw WASD keys and Shift, for picking tricks.
    pub keys: TrickInput,
    pub sprint: bool,
    pub jump: bool,
    /// Q pressed: flip trick.
    pub flip: bool,
    /// E pressed this frame / held.
    pub grab_start: bool,
    pub grab: bool,
    /// F held: grind.
    pub grind: bool,
    /// R pressed: front / back flip.
    pub air_flip: bool,
    pub grounded: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RideEvent {
    /// A trick started.
    Trick(String),
    /// Locked onto a rail.
    Grind(String),
    /// Landed a combo.
    Landed { names: Vec<String>, points: u32 },
    /// Fell off balance.
    Bail(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct RideOut {
    /// Velocity to move by (unless placed).
    pub vel: Vec3,
    /// Put the feet exactly here (grinding).
    pub place: Option<Vec3>,
    pub events: Vec<RideEvent>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ride {
    pub mount: MountKind,
    /// Direction of travel (yaw).
    pub heading: f32,
    pub speed: f32,
    pub trick: Option<ActiveTrick>,
    pub grind: Option<Grinding>,
    pub combo: Combo,
    /// Seconds left off balance after a bail.
    pub bail: f32,
    pub airborne: bool,
    /// Pushing / striding animation phase.
    pub push_phase: f32,
}

pub fn heading_dir(yaw: f32) -> Vec3 {
    Vec3::new(-yaw.sin(), 0.0, -yaw.cos())
}

fn yaw_of(dir: Vec3) -> f32 {
    (-dir.x).atan2(-dir.z)
}

fn wrap(a: f32) -> f32 {
    let mut d = a % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    } else if d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    d
}

impl Ride {
    pub fn new(mount: MountKind, heading: f32, speed: f32) -> Self {
        Ride {
            mount,
            heading,
            speed,
            trick: None,
            grind: None,
            combo: Combo::default(),
            bail: 0.0,
            airborne: false,
            push_phase: 0.0,
        }
    }

    /// What everyone else needs to draw this rider.
    pub fn view(&self, def: &MountDef) -> RideView {
        RideView {
            mount: self.mount,
            trick: self.trick.and_then(|t| {
                let secs = def.trick(t.cat, t.index)?.secs;
                Some((t.cat, t.index as u8, (t.t / secs).min(4.0)))
            }),
            grind: self.grind.map(|g| g.index as u8),
            bail: self.bail > 0.0,
            speed: self.speed,
            push: self.push_phase,
        }
    }

    fn bail_out(&mut self, why: &str, ev: &mut Vec<RideEvent>) {
        self.trick = None;
        self.grind = None;
        self.combo = Combo::default();
        self.bail = BAIL_SECS;
        self.speed *= 0.4;
        ev.push(RideEvent::Bail(why.to_string()));
    }

    /// Finish the trick in progress (if it can be): bank its points into the combo.
    fn finish_trick(&mut self, def: &MountDef) {
        if let Some(t) = self.trick.take() {
            if let Some(tr) = def.trick(t.cat, t.index) {
                let extra = if t.cat == TrickCat::Grab { (t.t - tr.secs).max(0.0) * tr.points as f32 * GRAB_BONUS } else { 0.0 };
                self.combo.names.push(tr.name.clone());
                self.combo.points += tr.points as f32 + extra;
            }
        }
    }

    /// Touching down: a half-done flip is a bail, anything else lands the combo.
    fn land(&mut self, def: &MountDef, ev: &mut Vec<RideEvent>) {
        if let Some(t) = self.trick {
            let secs = def.trick(t.cat, t.index).map_or(1.0, |tr| tr.secs);
            let flip = matches!(t.cat, TrickCat::Flip | TrickCat::AirFlip);
            if flip && t.t < secs * LAND_OK {
                let name = def.trick(t.cat, t.index).map_or("trick".to_string(), |tr| tr.name.clone());
                self.bail_out(&format!("landed the {name} too early"), ev);
                return;
            }
            self.finish_trick(def);
        }
        if !self.combo.names.is_empty() {
            ev.push(RideEvent::Landed { names: self.combo.names.clone(), points: self.combo.total() });
            self.combo = Combo::default();
        }
    }

    /// The rail to lock onto from `feet`, if any: (rail, distance along it).
    fn find_rail(feet: Vec3, rails: &[Rail]) -> Option<(usize, f32)> {
        rails
            .iter()
            .enumerate()
            .filter(|(_, r)| !r.is_empty())
            .filter_map(|(i, r)| {
                let s = r.project(feet);
                let p = r.at(s);
                let side = Vec3::new(feet.x - p.x, 0.0, feet.z - p.z).length();
                let dy = feet.y - p.y;
                let inside = s > 0.15 && s < r.len() - 0.15;
                (inside && side <= GRIND_REACH && (-GRIND_BELOW..=GRIND_ABOVE).contains(&dy)).then_some((side, i, s))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, i, s)| (i, s))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update(&mut self, def: &MountDef, i: &RideInput, vel_y: &mut f32, feet: Vec3, dt: f32, rails: &[Rail]) -> RideOut {
        let mut ev = Vec::new();
        let st = def.stats;

        // On a rail: slide along it.
        if let Some(mut g) = self.grind {
            let Some(rail) = rails.get(g.rail).copied() else {
                self.grind = None;
                return RideOut { vel: heading_dir(self.heading) * self.speed, place: None, events: ev };
            };
            let along = rail.dir() * g.sign;
            self.heading = yaw_of(along);
            self.speed += (st.grind_speed - self.speed) * (1.0 - (-2.0 * dt).exp());
            g.s += g.sign * self.speed * dt;
            g.t += dt;
            if let Some(tr) = def.trick(TrickCat::Grind, g.index) {
                self.combo.points += tr.points as f32 * dt;
            }
            if i.jump {
                // Ollie off the rail; the combo carries on.
                self.grind = None;
                self.airborne = true;
                *vel_y = st.jump;
                let at = rail.at(g.s.clamp(0.0, rail.len()));
                return RideOut { vel: along * self.speed + Vec3::Y * *vel_y, place: Some(at), events: ev };
            }
            if g.s < 0.0 || g.s > rail.len() {
                // Off the end.
                self.grind = None;
                self.airborne = true;
                *vel_y = RAIL_EXIT_POP;
                return RideOut { vel: along * self.speed + Vec3::Y * *vel_y, place: None, events: ev };
            }
            self.grind = Some(g);
            *vel_y = 0.0;
            return RideOut { vel: along * self.speed, place: Some(rail.at(g.s) + Vec3::Y * 0.02), events: ev };
        }

        let fwd = heading_dir(self.heading);
        if self.bail > 0.0 {
            // Off balance: roll on, slowing, no control.
            self.bail -= dt;
            self.speed *= (-2.5 * dt).exp();
            if i.grounded {
                *vel_y = (*vel_y).min(0.0);
                self.airborne = false;
            } else {
                *vel_y -= RIDE_GRAVITY * dt;
            }
            return RideOut { vel: fwd * self.speed + Vec3::Y * *vel_y, place: None, events: ev };
        }

        if i.grounded && *vel_y <= 0.0 {
            if self.airborne {
                self.land(def, &mut ev);
            }
            self.airborne = false;
            *vel_y = 0.0;
            // Carve toward WASD; pull the other way to brake; let go to coast.
            let top = st.top_speed * if i.sprint { SPRINT_TOP } else { 1.0 };
            if i.dir.length_squared() > 0.01 {
                let diff = wrap(yaw_of(i.dir) - self.heading);
                let slow = 1.0 - (self.speed / st.top_speed).min(1.0);
                let rate = st.turn * (0.55 + 0.9 * slow);
                let align = diff.cos();
                // Nearly stopped: just turn round. Pulling back at speed brakes without
                // turning; otherwise carve toward the keys.
                let step = if self.speed < 1.0 {
                    diff
                } else if align < -0.3 {
                    0.0
                } else {
                    diff.clamp(-rate * dt, rate * dt)
                };
                self.heading = wrap(self.heading + step);
                if align > 0.3 {
                    let push = st.push * if i.sprint { SPRINT_PUSH } else { 1.0 };
                    if self.speed < top {
                        self.speed = (self.speed + push * dt).min(top);
                    } else {
                        self.speed -= (self.speed - top) * (1.0 - (-1.5 * dt).exp());
                    }
                    self.push_phase += dt * 6.0;
                } else if align < -0.3 {
                    self.speed = (self.speed - st.push * 2.0 * dt).max(0.0);
                }
            } else {
                self.speed = (self.speed - (st.friction * self.speed + 0.3) * dt).max(0.0);
            }
            if i.jump {
                *vel_y = st.jump;
                self.airborne = true;
            }
            let fwd = heading_dir(self.heading);
            return RideOut { vel: fwd * self.speed + Vec3::Y * *vel_y, place: None, events: ev };
        }

        // In the air.
        self.airborne = true;
        *vel_y -= RIDE_GRAVITY * dt;

        // Grind: coming down onto a rail / ledge with F held.
        if i.grind && *vel_y <= 1.0 {
            if let Some((rail, s)) = Ride::find_rail(feet, rails) {
                if let Some(t) = self.trick {
                    let secs = def.trick(t.cat, t.index).map_or(1.0, |tr| tr.secs);
                    if matches!(t.cat, TrickCat::Flip | TrickCat::AirFlip) && t.t < secs * LAND_OK {
                        self.bail_out("came down on the rail mid-flip", &mut ev);
                        return RideOut { vel: fwd * self.speed + Vec3::Y * *vel_y, place: None, events: ev };
                    }
                    self.finish_trick(def);
                }
                let r = rails[rail];
                let sign = if fwd.dot(r.dir()) >= 0.0 { 1.0 } else { -1.0 };
                let index = def.find(TrickCat::Grind, i.keys).unwrap_or(0);
                if let Some(tr) = def.trick(TrickCat::Grind, index) {
                    self.combo.names.push(tr.name.clone());
                    ev.push(RideEvent::Grind(tr.name.clone()));
                }
                self.grind = Some(Grinding { rail, s, sign, index, t: 0.0 });
                self.speed = self.speed.max(4.0);
                *vel_y = 0.0;
                return RideOut { vel: r.dir() * sign * self.speed, place: Some(r.at(s) + Vec3::Y * 0.02), events: ev };
            }
        }

        // The trick in progress.
        if let Some(mut t) = self.trick {
            t.t += dt;
            self.trick = Some(t);
            let secs = def.trick(t.cat, t.index).map_or(0.5, |tr| tr.secs);
            let done = match t.cat {
                TrickCat::Grab => t.t >= secs && !i.grab,
                _ => t.t >= secs,
            };
            if done {
                self.finish_trick(def);
            }
        }
        // Start a new one.
        if self.trick.is_none() {
            let start = if i.flip {
                Some(TrickCat::Flip)
            } else if i.grab_start {
                Some(TrickCat::Grab)
            } else if i.air_flip {
                Some(TrickCat::AirFlip)
            } else {
                None
            };
            if let Some(cat) = start {
                let keys = if cat == TrickCat::AirFlip {
                    // R + S (any back direction) is a back flip, anything else a front flip.
                    let back = matches!(i.keys.dir, Dir::Down | Dir::DownLeft | Dir::DownRight);
                    TrickInput { dir: if back { Dir::Down } else { Dir::Up }, shift: false }
                } else {
                    i.keys
                };
                if let Some(index) = def.find(cat, keys) {
                    self.trick = Some(ActiveTrick { cat, index, t: 0.0 });
                    if let Some(tr) = def.trick(cat, index) {
                        ev.push(RideEvent::Trick(tr.name.clone()));
                    }
                }
            }
        }
        // A little air control.
        if i.dir.length_squared() > 0.01 {
            let diff = wrap(yaw_of(i.dir) - self.heading);
            let rate = st.turn * 0.35;
            self.heading = wrap(self.heading + diff.clamp(-rate * dt, rate * dt));
        }
        let fwd = heading_dir(self.heading);
        RideOut { vel: fwd * self.speed + Vec3::Y * *vel_y, place: None, events: ev }
    }

    /// The body ran into something: keep only the speed it actually made.
    pub fn blocked(&mut self, actual_speed: f32) {
        if self.grind.is_none() && actual_speed + 0.5 < self.speed {
            self.speed = actual_speed.max(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mounts::MountLibrary;

    const DT: f32 = 1.0 / 60.0;

    /// A rider on flat ground (y = 0) with optional rails.
    struct Rig {
        ride: Ride,
        def: MountDef,
        feet: Vec3,
        vel_y: f32,
        rails: Vec<Rail>,
        events: Vec<RideEvent>,
    }

    impl Rig {
        fn new(kind: MountKind) -> Self {
            let lib = MountLibrary::built_in();
            Rig {
                ride: Ride::new(kind, 0.0, 0.0),
                def: lib.get(kind).clone(),
                feet: Vec3::ZERO,
                vel_y: 0.0,
                rails: vec![],
                events: vec![],
            }
        }
        fn frame(&mut self, mut i: RideInput) {
            i.grounded = self.feet.y <= 0.0 && self.vel_y <= 0.0;
            let out = self.ride.update(&self.def, &i, &mut self.vel_y, self.feet, DT, &self.rails);
            self.events.extend(out.events);
            match out.place {
                Some(p) => self.feet = p,
                None => {
                    self.feet += out.vel * DT;
                    if self.feet.y < 0.0 {
                        self.feet.y = 0.0;
                    }
                }
            }
        }
        fn run(&mut self, secs: f32, i: RideInput) {
            for _ in 0..(secs / DT).round() as usize {
                self.frame(i);
            }
        }
        fn landed(&self) -> Option<(Vec<String>, u32)> {
            self.events.iter().rev().find_map(|e| match e {
                RideEvent::Landed { names, points } => Some((names.clone(), *points)),
                _ => None,
            })
        }
        fn bailed(&self) -> bool {
            self.events.iter().any(|e| matches!(e, RideEvent::Bail(_)))
        }
    }

    fn push() -> RideInput {
        RideInput { dir: Vec3::NEG_Z, ..Default::default() }
    }

    #[test]
    fn pushing_builds_speed_to_the_top_and_coasting_slows() {
        let mut r = Rig::new(MountKind::Skateboard);
        r.run(3.0, push());
        let top = r.def.stats.top_speed;
        assert!((r.ride.speed - top).abs() < 0.01, "{}", r.ride.speed);
        assert!(r.feet.z < -15.0, "rolled forward");
        r.run(1.0, RideInput { dir: Vec3::NEG_Z, sprint: true, ..Default::default() });
        assert!(r.ride.speed > top * 1.1, "shift pushes harder");
        let fast = r.ride.speed;
        r.run(1.0, RideInput::default());
        assert!(r.ride.speed < fast && r.ride.speed > 0.0, "coasting slows gently");
        r.run(0.5, RideInput { dir: Vec3::Z, ..Default::default() });
        assert!(r.ride.speed < 3.0, "pulling back brakes");
    }

    #[test]
    fn carving_turns_gradually_at_speed() {
        let mut r = Rig::new(MountKind::Longboard);
        r.run(3.0, push());
        r.frame(RideInput { dir: Vec3::X, ..Default::default() });
        assert!(r.ride.heading.abs() < 0.1, "no snap turn at speed");
        r.run(1.5, RideInput { dir: Vec3::X, ..Default::default() });
        assert!(heading_dir(r.ride.heading).dot(Vec3::X) > 0.9, "carved round");
    }

    #[test]
    fn ollie_and_kickflip_lands_for_points() {
        let mut r = Rig::new(MountKind::Skateboard);
        r.run(1.0, push());
        r.frame(RideInput { jump: true, ..push() });
        assert!(r.ride.airborne && r.vel_y > 0.0);
        r.frame(RideInput { flip: true, ..push() });
        assert!(matches!(r.events.last(), Some(RideEvent::Trick(n)) if n == "Kickflip"));
        r.run(1.0, push());
        assert!(!r.ride.airborne);
        let (names, points) = r.landed().expect("landed");
        assert_eq!(names, vec!["Kickflip"]);
        assert_eq!(points, 300);
        assert!(!r.bailed());
    }

    #[test]
    fn landing_mid_flip_is_a_bail() {
        let mut r = Rig::new(MountKind::Skateboard);
        r.run(0.5, push());
        // A tiny hop: back on the ground long before the 0.65 s laser flip is done.
        r.vel_y = 3.0;
        r.feet.y = 0.01;
        r.ride.airborne = true;
        r.frame(RideInput { flip: true, keys: TrickInput { dir: Dir::None, shift: true }, ..push() });
        r.run(0.5, push());
        assert!(r.bailed());
        assert!(r.ride.bail > 0.0 || r.events.iter().any(|e| matches!(e, RideEvent::Bail(_))));
        assert!(r.landed().is_none(), "nothing scored");
        let slow = r.ride.speed;
        r.run(0.3, RideInput { dir: Vec3::X, ..Default::default() });
        assert!(r.ride.speed <= slow, "no control while off balance");
    }

    #[test]
    fn combos_multiply_and_grabs_pay_for_holding() {
        let mut r = Rig::new(MountKind::Skateboard);
        r.run(1.0, push());
        r.frame(RideInput { jump: true, ..push() });
        r.vel_y = 12.0; // a big air
        // Pop shove-it (0.4 s), then an indy grab held a bit past its time.
        r.frame(RideInput { flip: true, keys: TrickInput { dir: Dir::Left, shift: false }, ..push() });
        r.run(0.42, push());
        assert!(r.ride.trick.is_none(), "flip done");
        r.frame(RideInput { grab_start: true, grab: true, ..push() });
        r.run(0.55, RideInput { grab: true, ..push() });
        r.run(0.5, push());
        let (names, points) = r.landed().expect("landed");
        assert_eq!(names, vec!["Pop Shove-it", "Indy"]);
        // (250 + 250 + a little grab bonus) x 2.
        assert!(points > 1000 && points < 1200, "{points}");
    }

    #[test]
    fn front_and_back_flips_with_r() {
        for (dir, back) in [(Dir::Up, false), (Dir::Down, true), (Dir::None, false)] {
            let mut r = Rig::new(MountKind::Bmx);
            r.run(1.0, push());
            r.frame(RideInput { jump: true, ..push() });
            r.frame(RideInput { air_flip: true, keys: TrickInput { dir, shift: false }, ..push() });
            let t = r.ride.trick.expect("flipping");
            let flip = r.def.trick(t.cat, t.index).unwrap().anim.body_flip;
            assert_eq!(flip > 0.0, back, "{dir:?}");
        }
    }

    #[test]
    fn grinding_a_rail_then_off_the_end() {
        let mut r = Rig::new(MountKind::Skateboard);
        r.rails = vec![Rail { a: Vec3::new(0.0, 0.55, -4.0), b: Vec3::new(0.0, 0.55, -18.0) }];
        r.run(1.0, push());
        r.frame(RideInput { jump: true, ..push() });
        // Hold F (and S: a 5-0) on the way down.
        let grind = RideInput { grind: true, keys: TrickInput { dir: Dir::Down, shift: false }, ..push() };
        for _ in 0..120 {
            r.frame(grind);
            if r.ride.grind.is_some() {
                break;
            }
        }
        let g = r.ride.grind.expect("on the rail");
        assert_eq!(r.def.grinds[g.index].name, "5-0");
        assert!((r.feet.y - 0.57).abs() < 0.01 && r.feet.x.abs() < 1e-4, "on top of it: {}", r.feet);
        assert_eq!(g.sign, 1.0, "riding a -> b");
        r.run(3.0, grind);
        assert!(r.ride.grind.is_none(), "rolled off the end");
        r.run(1.0, push());
        let (names, points) = r.landed().expect("landed");
        assert_eq!(names, vec!["5-0"]);
        assert!(points > 100, "points for the time on the rail: {points}");
    }

    #[test]
    fn jumping_off_a_rail_keeps_the_combo_going() {
        let mut r = Rig::new(MountKind::InlineSkates);
        r.rails = vec![Rail { a: Vec3::new(0.0, 0.55, -2.0), b: Vec3::new(0.0, 0.55, -30.0) }];
        r.run(0.3, push());
        r.frame(RideInput { jump: true, ..push() });
        for _ in 0..120 {
            r.frame(RideInput { grind: true, ..push() });
            if r.ride.grind.is_some() {
                break;
            }
        }
        assert!(r.ride.grind.is_some());
        r.run(0.5, RideInput { grind: true, ..push() });
        r.frame(RideInput { jump: true, ..push() });
        assert!(r.ride.grind.is_none() && r.vel_y > 0.0);
        r.frame(RideInput { flip: true, ..push() });
        r.run(1.5, push());
        let (names, _) = r.landed().expect("landed");
        assert_eq!(names, vec!["Royale", "Flatspin 360"]);
    }

    #[test]
    fn no_grind_without_f_or_far_from_the_rail() {
        let mut r = Rig::new(MountKind::Scooter);
        r.rails = vec![Rail { a: Vec3::new(3.0, 0.55, -2.0), b: Vec3::new(3.0, 0.55, -12.0) }];
        r.run(0.4, push());
        r.frame(RideInput { jump: true, ..push() });
        r.run(1.0, RideInput { grind: true, ..push() });
        assert!(r.ride.grind.is_none(), "3 m away");
        assert!(Ride::find_rail(Vec3::new(3.2, 0.7, -5.0), &r.rails).is_some());
        assert!(Ride::find_rail(Vec3::new(3.2, 2.0, -5.0), &r.rails).is_none(), "too high above it");
    }

    #[test]
    fn view_reports_trick_progress() {
        let mut r = Rig::new(MountKind::Skateboard);
        r.run(1.0, push());
        r.frame(RideInput { jump: true, ..push() });
        r.frame(RideInput { flip: true, ..push() });
        r.run(0.2, push());
        let v = r.ride.view(&r.def);
        let (cat, idx, p) = v.trick.unwrap();
        assert_eq!((cat, idx), (TrickCat::Flip, 0));
        assert!(p > 0.3 && p < 0.6, "{p}");
        assert_eq!(v.mount, MountKind::Skateboard);
    }

    #[test]
    fn running_into_a_wall_kills_the_speed() {
        let mut r = Ride::new(MountKind::Skateboard, 0.0, 10.0);
        r.blocked(1.0);
        assert_eq!(r.speed, 1.0);
        r.blocked(5.0);
        assert_eq!(r.speed, 1.0, "never speeds up");
    }
}
