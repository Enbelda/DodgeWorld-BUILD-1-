//! Arena geometry shared by the simulation (ball bounces, bots) and the renderer/physics.

use bevy::math::Vec3;

use super::ball::BallKind;

/// Half the width/length of the square play area, in metres.
pub const ARENA_HALF: f32 = 20.0;
pub const WALL_HEIGHT: f32 = 8.0;
pub const WALL_THICKNESS: f32 = 1.0;

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

/// Every solid box in the arena: four boundary walls plus cover blocks.
pub fn solids() -> Vec<Aabb> {
    let h = ARENA_HALF;
    let t = WALL_THICKNESS;
    let y = WALL_HEIGHT / 2.0;
    let mut v = vec![
        Aabb::from_center(Vec3::new(0.0, y, h + t / 2.0), Vec3::new(h + t, y, t / 2.0)),
        Aabb::from_center(Vec3::new(0.0, y, -h - t / 2.0), Vec3::new(h + t, y, t / 2.0)),
        Aabb::from_center(Vec3::new(h + t / 2.0, y, 0.0), Vec3::new(t / 2.0, y, h)),
        Aabb::from_center(Vec3::new(-h - t / 2.0, y, 0.0), Vec3::new(t / 2.0, y, h)),
    ];
    v.extend(cover());
    v
}

/// Cover blocks only (mirrored so both halves are fair).
pub fn cover() -> Vec<Aabb> {
    let mut v = Vec::new();
    for s in [-1.0_f32, 1.0] {
        // Low wall near each team's half, a tall pillar on each flank.
        v.push(Aabb::from_center(Vec3::new(0.0, 0.6, 7.0 * s), Vec3::new(3.0, 0.6, 0.4)));
        v.push(Aabb::from_center(Vec3::new(9.0 * s, 1.5, 0.0), Vec3::new(1.0, 1.5, 1.0)));
        v.push(Aabb::from_center(Vec3::new(-9.0 * s, 1.0, 12.0 * s), Vec3::new(1.5, 1.0, 1.5)));
    }
    v
}

/// Where balls sit at the start of a match, and which kind each is.
/// Specials sit on the centre line so neither team is closer.
pub fn ball_pads() -> Vec<(Vec3, BallKind)> {
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
pub fn spawn_point(red: bool, slot: usize) -> Vec3 {
    let z = if red { 16.0 } else { -16.0 };
    let x = (slot as f32 - 1.5) * 3.0;
    Vec3::new(x, 0.0, z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_is_on_closest_wall() {
        let walls = solids();
        // Near the +X boundary wall in an open spot.
        let a = nearest_wall_anchor(Vec3::new(17.0, 0.0, 5.0), &walls, 0.45);
        assert!((a.x - (ARENA_HALF - 0.45)).abs() < 1e-4, "{a}");
        assert_eq!(a.z, 5.0);
    }

    #[test]
    fn special_pads_are_mirrored() {
        let pads = ball_pads();
        let b = pads.iter().find(|p| p.1 == BallKind::Boomerang).unwrap().0;
        let s = pads.iter().find(|p| p.1 == BallKind::Slime).unwrap().0;
        assert_eq!(b.z, 0.0);
        assert_eq!(s.z, 0.0);
        assert_eq!(b.x, -s.x);
    }
}
