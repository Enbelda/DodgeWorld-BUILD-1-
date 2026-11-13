//! Arena geometry shared by the simulation (ball bounces, bots) and the renderer/physics.

use bevy::math::Vec3;

use serde::{Deserialize, Serialize};

use super::ball::BallKind;

/// Half the width/length of the classic square play area, in metres.
pub const ARENA_HALF: f32 = 20.0;
pub const WALL_HEIGHT: f32 = 8.0;
pub const WALL_THICKNESS: f32 = 1.0;
/// The grand arena: twice the size, taller walls.
pub const GRAND_HALF: f32 = 40.0;
pub const GRAND_WALL_HEIGHT: f32 = 10.0;

/// Which arena geometry a match is played on (the day/night look is separate).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum ArenaId {
    /// 40 x 40 m: the original arena.
    #[default]
    Classic,
    /// 80 x 80 m: more cover, bunkers, wall-run corridors and two pairs of parkour panels.
    Grand,
}

impl ArenaId {
    pub fn half(self) -> f32 {
        match self {
            ArenaId::Classic => ARENA_HALF,
            ArenaId::Grand => GRAND_HALF,
        }
    }
    pub fn wall_height(self) -> f32 {
        match self {
            ArenaId::Classic => WALL_HEIGHT,
            ArenaId::Grand => GRAND_WALL_HEIGHT,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            ArenaId::Classic => "Classic arena",
            ArenaId::Grand => "Grand arena",
        }
    }
}

/// Axis-aligned box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn from_center(center: Vec3, half: Vec3) -> Self {
        Aabb { min: center - half, max: center + half }
    }
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }
    pub fn half(&self) -> Vec3 {
        (self.max - self.min) * 0.5
    }
    pub fn expanded(&self, r: f32) -> Aabb {
        Aabb { min: self.min - Vec3::splat(r), max: self.max + Vec3::splat(r) }
    }
    pub fn contains(&self, p: Vec3) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }
}

/// Every solid box in the arena: four boundary walls (always the first four) plus cover.
pub fn solids(arena: ArenaId) -> Vec<Aabb> {
    let h = arena.half();
    let t = WALL_THICKNESS;
    let y = arena.wall_height() / 2.0;
    let mut v = vec![
        Aabb::from_center(Vec3::new(0.0, y, h + t / 2.0), Vec3::new(h + t, y, t / 2.0)),
        Aabb::from_center(Vec3::new(0.0, y, -h - t / 2.0), Vec3::new(h + t, y, t / 2.0)),
        Aabb::from_center(Vec3::new(h + t / 2.0, y, 0.0), Vec3::new(t / 2.0, y, h)),
        Aabb::from_center(Vec3::new(-h - t / 2.0, y, 0.0), Vec3::new(t / 2.0, y, h)),
    ];
    v.extend(cover(arena));
    v.extend(rail_boxes(arena));
    v
}

/// Height of the top of a free-standing grind rail.
pub const RAIL_HEIGHT: f32 = 0.55;
/// Half the thickness of a rail bar.
pub const RAIL_HALF: f32 = 0.04;

/// A grindable line: a rail's top, or the top edge of a low wall (a ledge).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rail {
    pub a: Vec3,
    pub b: Vec3,
}

impl Rail {
    pub fn dir(&self) -> Vec3 {
        (self.b - self.a).normalize_or_zero()
    }
    pub fn len(&self) -> f32 {
        self.a.distance(self.b)
    }
    pub fn is_empty(&self) -> bool {
        self.len() < 1e-3
    }
    /// The point at distance `s` along the rail.
    pub fn at(&self, s: f32) -> Vec3 {
        self.a + self.dir() * s
    }
    /// Distance along the rail of the point closest to `p` (clamped to the rail).
    pub fn project(&self, p: Vec3) -> f32 {
        (p - self.a).dot(self.dir()).clamp(0.0, self.len())
    }
}

/// A rail's two ends on the floor plan: (x, z), (x, z).
type RailPlan = ((f32, f32), (f32, f32));

/// Free-standing rails (start, end of the top bar), mirrored through the centre.
fn rail_lines(arena: ArenaId) -> Vec<(Vec3, Vec3)> {
    let half: &[RailPlan] = match arena {
        ArenaId::Classic => &[((12.0, 6.0), (12.0, 14.0))],
        ArenaId::Grand => &[((10.0, 18.0), (10.0, 26.0)), ((-20.0, 5.0), (-12.0, 5.0)), ((-4.0, 19.0), (4.0, 19.0))],
    };
    let mut v = Vec::new();
    for s in [1.0_f32, -1.0] {
        for &((ax, az), (bx, bz)) in half {
            v.push((Vec3::new(ax * s, RAIL_HEIGHT, az * s), Vec3::new(bx * s, RAIL_HEIGHT, bz * s)));
        }
    }
    v
}

/// The rails as solid bars (part of `solids`).
pub fn rail_boxes(arena: ArenaId) -> Vec<Aabb> {
    rail_lines(arena)
        .into_iter()
        .map(|(a, b)| {
            let c = (a + b) * 0.5;
            let along = (b - a).abs() * 0.5;
            Aabb::from_center(
                Vec3::new(c.x, RAIL_HEIGHT / 2.0, c.z),
                Vec3::new(along.x.max(RAIL_HALF), RAIL_HEIGHT / 2.0, along.z.max(RAIL_HALF)),
            )
        })
        .collect()
}

/// A long, thin, low box: its top edge is a ledge you can grind.
pub fn is_ledge(b: &Aabb) -> bool {
    let h = b.half();
    h.x.min(h.z) <= 0.5 && h.x.max(h.z) >= 2.0 && b.max.y <= 2.0 && b.min.y <= 0.01
}

/// Everything grindable: the free-standing rails and the top of every low wall.
pub fn rails(arena: ArenaId) -> Vec<Rail> {
    let mut v: Vec<Rail> = rail_lines(arena).into_iter().map(|(a, b)| Rail { a, b }).collect();
    for b in cover(arena).iter().filter(|b| is_ledge(b)) {
        let c = b.center();
        let h = b.half();
        let (a, e) = if h.x > h.z {
            (Vec3::new(b.min.x + 0.2, b.max.y, c.z), Vec3::new(b.max.x - 0.2, b.max.y, c.z))
        } else {
            (Vec3::new(c.x, b.max.y, b.min.z + 0.2), Vec3::new(c.x, b.max.y, b.max.z - 0.2))
        };
        v.push(Rail { a, b: e });
    }
    v
}

/// Cover blocks only (mirrored so both halves are fair).
pub fn cover(arena: ArenaId) -> Vec<Aabb> {
    match arena {
        ArenaId::Classic => classic_cover(),
        ArenaId::Grand => grand_cover(),
    }
}

fn classic_cover() -> Vec<Aabb> {
    let mut v = Vec::new();
    for s in [-1.0_f32, 1.0] {
        // Low wall near each team's half, a tall pillar on each flank.
        v.push(Aabb::from_center(Vec3::new(0.0, 0.6, 7.0 * s), Vec3::new(3.0, 0.6, 0.4)));
        v.push(Aabb::from_center(Vec3::new(9.0 * s, 1.5, 0.0), Vec3::new(1.0, 1.5, 1.0)));
        v.push(Aabb::from_center(Vec3::new(-9.0 * s, 1.0, 12.0 * s), Vec3::new(1.5, 1.0, 1.5)));
        // Parkour panel on each flank: 5.5 m tall, 8 m long, 4.2 m from the side wall, so
        // players can wall run, climb, hang from its top and jump wall to wall.
        v.push(Aabb::from_center(Vec3::new(15.5 * s, 2.75, 0.0), Vec3::new(0.3, 2.75, 4.0)));
    }
    v
}

/// The grand arena's cover, listed for the Red (+Z) half and mirrored through the centre
/// (x, z) -> (-x, -z) for Blue, so both teams get the same field.
fn grand_cover() -> Vec<Aabb> {
    // (centre x, centre z, half x, height, half z)
    const HALF: [(f32, f32, f32, f32, f32); 17] = [
        // Low wall in front of the spawn, two flanking low walls.
        (0.0, 25.0, 6.0, 1.2, 0.4),
        (-14.0, 15.0, 3.5, 1.2, 0.4),
        (14.0, 15.0, 3.5, 1.2, 0.4),
        // Side-on low walls either side of the centre.
        (8.0, 2.5, 0.4, 1.2, 2.5),
        // Pillars.
        (-6.0, 9.0, 1.0, 3.0, 1.0),
        (6.0, 9.0, 1.0, 3.0, 1.0),
        // Crates and a step block up onto them.
        (-22.0, 21.0, 2.0, 2.0, 2.0),
        (-19.0, 18.2, 1.0, 1.0, 1.0),
        (24.0, 30.0, 1.5, 2.0, 1.5),
        // L-shaped bunker (2.5 m) on the -X side.
        (-25.0, 6.0, 4.0, 2.5, 0.4),
        (-28.6, 9.4, 0.4, 2.5, 3.0),
        // Wall-run corridor: two 4 m walls 3.6 m apart.
        (17.0, 3.0, 0.3, 4.0, 3.5),
        (21.2, 3.0, 0.3, 4.0, 3.5),
        // Parkour panels by the side walls (4.2 m corridor) and a centre-line panel.
        (35.5, 6.0, 0.3, 5.5, 5.0),
        (-35.5, 22.0, 0.3, 5.5, 4.0),
        (0.0, 14.0, 2.5, 5.5, 0.3),
        // A long mid-height wall to hang from and vault.
        (-10.0, 30.0, 4.0, 1.9, 0.5),
    ];
    let mut v = Vec::new();
    for s in [1.0_f32, -1.0] {
        for (x, z, hx, h, hz) in HALF {
            v.push(Aabb::from_center(Vec3::new(x * s, h / 2.0, z * s), Vec3::new(hx, h / 2.0, hz)));
        }
    }
    v
}

/// Where balls sit at the start of a match, and which kind each is.
/// Specials sit on the centre line so neither team is closer.
pub fn ball_pads(arena: ArenaId) -> Vec<(Vec3, BallKind)> {
    if arena == ArenaId::Grand {
        return vec![
            (Vec3::new(-4.0, 0.3, 0.0), BallKind::Normal),
            (Vec3::new(4.0, 0.3, 0.0), BallKind::Normal),
            (Vec3::new(0.0, 0.3, -5.0), BallKind::Normal),
            (Vec3::new(0.0, 0.3, 5.0), BallKind::Normal),
            (Vec3::new(-27.0, 0.3, 0.0), BallKind::Normal),
            (Vec3::new(27.0, 0.3, 0.0), BallKind::Normal),
            (Vec3::new(-13.0, 0.3, 0.0), BallKind::Boomerang),
            (Vec3::new(13.0, 0.3, 0.0), BallKind::Slime),
            (Vec3::new(0.0, 0.3, 0.0), BallKind::Brick),
        ];
    }
    vec![
        (Vec3::new(-4.0, 0.3, 0.0), BallKind::Normal),
        (Vec3::new(4.0, 0.3, 0.0), BallKind::Normal),
        (Vec3::new(0.0, 0.3, -3.0), BallKind::Normal),
        (Vec3::new(0.0, 0.3, 3.0), BallKind::Normal),
        (Vec3::new(-13.0, 0.3, 0.0), BallKind::Boomerang),
        (Vec3::new(13.0, 0.3, 0.0), BallKind::Slime),
        (Vec3::new(0.0, 0.3, 0.0), BallKind::Brick),
    ]
}

/// Where a slimed player at `pos` gets stuck: the closest point on the side of any solid
/// at least waist-high, pushed out by the body radius.
pub fn nearest_wall_anchor(pos: Vec3, solids: &[Aabb], body_radius: f32) -> Vec3 {
    let mut best = (f32::MAX, pos);
    for s in solids.iter().filter(|s| s.max.y > pos.y + 0.8) {
        let e = s.expanded(body_radius);
        let mut p = Vec3::new(pos.x.clamp(e.min.x, e.max.x), pos.y, pos.z.clamp(e.min.z, e.max.z));
        // Already overlapping in both axes: push out along the shallowest side.
        if p.x == pos.x && p.z == pos.z {
            let pens = [pos.x - e.min.x, e.max.x - pos.x, pos.z - e.min.z, e.max.z - pos.z];
            let i = (0..4).min_by(|&a, &b| pens[a].total_cmp(&pens[b])).unwrap_or(0);
            match i {
                0 => p.x = e.min.x,
                1 => p.x = e.max.x,
                2 => p.z = e.min.z,
                _ => p.z = e.max.z,
            }
        }
        let d = Vec3::new(p.x - pos.x, 0.0, p.z - pos.z).length();
        if d < best.0 {
            best = (d, p);
        }
    }
    best.1
}

/// Spawn point for the n-th player of a team. Red spawns at +Z, Blue at -Z.
pub fn spawn_point(arena: ArenaId, red: bool, slot: usize) -> Vec3 {
    let back = arena.half() - 4.0;
    let z = if red { back } else { -back };
    let x = (slot as f32 - 1.5) * 3.0;
    Vec3::new(x, 0.0, z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_is_on_closest_wall() {
        let walls = solids(ArenaId::Classic);
        // Near the +X boundary wall in an open spot.
        let a = nearest_wall_anchor(Vec3::new(17.0, 0.0, 10.0), &walls, 0.45);
        assert!((a.x - (ARENA_HALF - 0.45)).abs() < 1e-4, "{a}");
        assert_eq!(a.z, 10.0);
    }

    #[test]
    fn parkour_panels_leave_a_corridor_by_the_side_walls() {
        let panels: Vec<Aabb> = cover(ArenaId::Classic).into_iter().filter(|c| c.max.y > 4.0).collect();
        assert_eq!(panels.len(), 2);
        for p in panels {
            let gap = ARENA_HALF - p.max.x.abs().max(p.min.x.abs());
            assert!((4.0..4.5).contains(&gap), "corridor {gap}");
        }
    }

    #[test]
    fn special_pads_are_mirrored() {
        let pads = ball_pads(ArenaId::Classic);
        let b = pads.iter().find(|p| p.1 == BallKind::Boomerang).unwrap().0;
        let s = pads.iter().find(|p| p.1 == BallKind::Slime).unwrap().0;
        assert_eq!(b.z, 0.0);
        assert_eq!(s.z, 0.0);
        assert_eq!(b.x, -s.x);
    }

    #[test]
    fn grand_arena_is_twice_as_big_and_mirrored() {
        let g = ArenaId::Grand;
        assert_eq!(g.half(), 2.0 * ArenaId::Classic.half());
        let c = cover(g);
        assert!(c.len() > 2 * cover(ArenaId::Classic).len(), "more cover");
        for b in &c {
            let m = Aabb::from_center(Vec3::new(-b.center().x, b.center().y, -b.center().z), b.half());
            assert!(c.iter().any(|o| o.min.abs_diff_eq(m.min, 1e-4)), "{b:?} has no mirror");
            assert!(b.max.x.abs() < g.half() && b.max.z.abs() < g.half(), "inside the walls");
        }
    }

    #[test]
    fn grand_pads_and_spawns_are_clear_of_cover() {
        let g = ArenaId::Grand;
        let s = solids(g);
        let clear = |p: Vec3, r: f32| !s.iter().any(|b| b.expanded(r).contains(Vec3::new(p.x, 0.5, p.z)));
        for (p, _) in ball_pads(g) {
            assert!(clear(p, 0.8), "pad {p} inside cover");
        }
        for red in [true, false] {
            for slot in 0..4 {
                let p = spawn_point(g, red, slot);
                assert!(clear(p, 0.6), "spawn {p} inside cover");
            }
        }
    }

    #[test]
    fn both_arenas_have_rails_and_grindable_ledges() {
        for arena in [ArenaId::Classic, ArenaId::Grand] {
            let r = rails(arena);
            let free = rail_boxes(arena).len();
            assert!(free >= 2 && r.len() > free, "{arena:?}: rails plus ledges");
            for rail in &r {
                assert!(rail.len() > 3.0, "{rail:?}");
                assert!(rail.a.x.abs() < arena.half() && rail.a.z.abs() < arena.half());
            }
            // Rails don't run through cover.
            let cover = cover(arena);
            for b in rail_boxes(arena) {
                assert!(!cover.iter().any(|c| c.expanded(0.3).contains(b.center())), "{b:?}");
            }
        }
        let r = Rail { a: Vec3::ZERO, b: Vec3::new(0.0, 0.0, 4.0) };
        assert_eq!(r.project(Vec3::new(3.0, 1.0, 2.5)), 2.5);
        assert_eq!(r.project(Vec3::new(0.0, 0.0, 9.0)), 4.0);
        assert_eq!(r.at(1.0), Vec3::new(0.0, 0.0, 1.0));
    }
}
