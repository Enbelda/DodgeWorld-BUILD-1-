//! The sewer race course, between deathmatch maps.
//!
//! It sits under the arena, so it is always in the same place whichever map is on top.
//! When the race starts, a hole opens in the middle of the arena floor. Players jump in,
//! fall 30 m into the sewer, and race a five-lane course to the finish. Each lane is 12 m
//! wide and runs along X, and the turns at the lane ends form a zig-zag:
//!
//! ```text
//!  z 48  ====================================> FINISH (x > 58)
//!  z 36  <===================================
//!  z 24  ====================================>
//!  z 12  <===================================
//!  z  0  START (under the hole) =============>
//! ```
//!
//! Each lane has a grind rail along one side and low walls or pipes to ollie over on the
//! other, so the centre line (the bots' path) stays clear.

use bevy::math::Vec3;

use super::layout::{is_ledge, Aabb, Rail, RAIL_HALF};

/// Floor and ceiling heights of the sewer.
pub const SEWER_FLOOR: f32 = -30.0;
pub const SEWER_CEILING: f32 = -22.0;
/// Below this height a player counts as being down the hole.
pub const BELOW: f32 = -3.0;
/// Half the size of the square gap in the arena floor (the open manhole).
pub const HOLE_HALF: f32 = 1.5;
/// The lanes: centre z of each, their width, and how far they run along X.
pub const LANE_W: f32 = 12.0;
pub const LANES: usize = 5;
pub const X_MIN: f32 = -6.0;
pub const X_MAX: f32 = 66.0;
pub const Z_MIN: f32 = -6.0;
pub const Z_MAX: f32 = 54.0;
/// The finish line, across the end of the last lane.
pub const FINISH_X: f32 = 58.0;
const WALL_T: f32 = 1.0;
/// How long the gaps at the lane ends are.
const GAP: f32 = 10.0;
/// Rail height in the sewer (above its floor).
const RAIL_TOP: f32 = 0.6;

fn lane_z(k: usize) -> f32 {
    k as f32 * LANE_W
}

/// Lane k runs toward +X when k is even, toward -X when odd.
fn lane_dir(k: usize) -> f32 {
    if k.is_multiple_of(2) {
        1.0
    } else {
        -1.0
    }
}

/// A box standing on the sewer floor: centre (x, z), half sizes (x, z) and height.
fn block(x: f32, z: f32, hx: f32, hz: f32, h: f32) -> Aabb {
    Aabb::from_center(Vec3::new(x, SEWER_FLOOR + h / 2.0, z), Vec3::new(hx, h / 2.0, hz))
}

/// Outer walls and the walls between lanes (with a gap at alternating ends).
fn walls() -> Vec<Aabb> {
    let h = SEWER_CEILING - SEWER_FLOOR;
    let (cx, hx) = ((X_MIN + X_MAX) / 2.0, (X_MAX - X_MIN) / 2.0);
    let (cz, hz) = ((Z_MIN + Z_MAX) / 2.0, (Z_MAX - Z_MIN) / 2.0);
    let t = WALL_T / 2.0;
    let mut v = vec![
        block(cx, Z_MIN - t, hx + WALL_T, t, h),
        block(cx, Z_MAX + t, hx + WALL_T, t, h),
        block(X_MIN - t, cz, t, hz, h),
        block(X_MAX + t, cz, t, hz, h),
    ];
    for k in 0..LANES - 1 {
        let z = lane_z(k) + LANE_W / 2.0;
        // Going +X the gap is at the east end, going -X at the west end.
        let (a, b) = if lane_dir(k) > 0.0 { (X_MIN, X_MAX - GAP) } else { (X_MIN + GAP, X_MAX) };
        v.push(block((a + b) / 2.0, z, (b - a) / 2.0, t, h));
    }
    v
}

/// Obstacles: low walls (ollie, vault or grind them) and pipes, kept off the centre line.
/// (lane, x, z offset from the lane centre, half x, half z, height)
const OBSTACLES: [(usize, f32, f32, f32, f32, f32); 10] = [
    (0, 18.0, -3.5, 3.0, 0.4, 1.0),
    (0, 40.0, 4.0, 0.5, 1.5, 1.6),
    (1, 44.0, -3.5, 4.0, 0.4, 1.0),
    (1, 14.0, 4.0, 0.6, 1.6, 1.4),
    (2, 12.0, 3.5, 3.0, 0.4, 1.0),
    (2, 32.0, -4.0, 1.0, 1.0, 2.4),
    (2, 32.0, 4.0, 1.0, 1.0, 2.4),
    (3, 46.0, 3.5, 3.5, 0.4, 1.0),
    (3, 20.0, -4.0, 0.6, 1.6, 1.4),
    (4, 30.0, 3.5, 5.0, 0.4, 1.0),
];

/// Free-standing grind rails: (lane, x from, x to, z offset).
const RAILS: [(usize, f32, f32, f32); 5] = [
    (0, 24.0, 36.0, 3.5),
    (1, 36.0, 22.0, 3.5),
    (2, 40.0, 52.0, -3.5),
    (3, 34.0, 10.0, -3.5),
    (4, 8.0, 22.0, -3.5),
];

fn obstacles() -> Vec<Aabb> {
    OBSTACLES.iter().map(|&(k, x, dz, hx, hz, h)| block(x, lane_z(k) + dz, hx, hz, h)).collect()
}

fn rail_lines() -> Vec<(Vec3, Vec3)> {
    RAILS
        .iter()
        .map(|&(k, a, b, dz)| {
            let (y, z) = (SEWER_FLOOR + RAIL_TOP, lane_z(k) + dz);
            (Vec3::new(a, y, z), Vec3::new(b, y, z))
        })
        .collect()
}

/// Every solid box in the sewer (walls, obstacles, rail bars). The floor is separate.
pub fn solids() -> Vec<Aabb> {
    let mut v = walls();
    v.extend(obstacles());
    for (a, b) in rail_lines() {
        let c = (a + b) * 0.5;
        let along = (b - a).abs() * 0.5;
        v.push(block(c.x, c.z, along.x.max(RAIL_HALF), along.z.max(RAIL_HALF), RAIL_TOP));
    }
    v
}

/// The sewer floor slab.
pub fn floor() -> Aabb {
    Aabb {
        min: Vec3::new(X_MIN - WALL_T, SEWER_FLOOR - 0.5, Z_MIN - WALL_T),
        max: Vec3::new(X_MAX + WALL_T, SEWER_FLOOR, Z_MAX + WALL_T),
    }
}

/// Everything grindable: the rails and the tops of the low walls.
pub fn rails() -> Vec<Rail> {
    let mut v: Vec<Rail> = rail_lines().into_iter().map(|(a, b)| Rail { a, b }).collect();
    for b in obstacles() {
        // Ledges are judged against the sewer floor, like the arena's against y = 0.
        let lifted = Aabb { min: b.min - Vec3::Y * SEWER_FLOOR, max: b.max - Vec3::Y * SEWER_FLOOR };
        if is_ledge(&lifted) {
            let (c, h) = (b.center(), b.half());
            let (a, e) = if h.x > h.z {
                (Vec3::new(b.min.x + 0.2, b.max.y, c.z), Vec3::new(b.max.x - 0.2, b.max.y, c.z))
            } else {
                (Vec3::new(c.x, b.max.y, b.min.z + 0.2), Vec3::new(c.x, b.max.y, b.max.z - 0.2))
            };
            v.push(Rail { a, b: e });
        }
    }
    v
}

/// The racing line along the lane centres (on the sewer floor), start to finish.
pub fn waypoints() -> Vec<Vec3> {
    let (east, west) = (X_MAX - GAP / 2.0, X_MIN + GAP / 2.0);
    let mut v = vec![Vec3::new(0.0, SEWER_FLOOR, 0.0)];
    for k in 0..LANES {
        let z = lane_z(k);
        let end = if k == LANES - 1 { X_MAX - 2.0 } else if lane_dir(k) > 0.0 { east } else { west };
        v.push(Vec3::new(end, SEWER_FLOOR, z));
        if k + 1 < LANES {
            v.push(Vec3::new(end, SEWER_FLOOR, lane_z(k + 1)));
        }
    }
    v
}

/// Is `pos` in the sewer (fell down the hole)?
pub fn is_below(pos: Vec3) -> bool {
    pos.y < BELOW
}

/// Has a racer at `pos` crossed the finish line?
pub fn finished(pos: Vec3) -> bool {
    is_below(pos) && pos.z > lane_z(LANES - 1) - LANE_W / 2.0 && pos.x > FINISH_X
}

/// How far along the course `pos` is, in metres (for standings).
pub fn progress(pos: Vec3) -> f32 {
    if !is_below(pos) {
        return 0.0;
    }
    let wps = waypoints();
    // The lane the racer is in, and the distance along the racing line to its start.
    let k = ((pos.z / LANE_W).round().max(0.0) as usize).min(LANES - 1);
    let mut before = 0.0;
    for w in wps.windows(2).take(2 * k) {
        before += w[0].distance(w[1]);
    }
    let start = wps[2 * k];
    let along = (pos.x - start.x) * lane_dir(k);
    before + along.max(0.0)
}

/// Where a player who never jumped in is dropped (above the start, falling).
pub fn drop_point(slot: usize) -> Vec3 {
    let x = (slot % 4) as f32 * 1.2 - 1.8;
    let z = (slot / 4) as f32 * 1.2 - 0.6;
    Vec3::new(x, SEWER_CEILING - 1.0, z)
}

/// Clamp a reported position into the sewer's bounds.
pub fn clamp(pos: Vec3) -> Vec3 {
    pos.clamp(Vec3::new(X_MIN, SEWER_FLOOR - 5.0, Z_MIN), Vec3::new(X_MAX, 50.0, Z_MAX))
}

/// Lights along the lanes.
pub fn lights() -> Vec<Vec3> {
    let mut v = Vec::new();
    for k in 0..LANES {
        for x in [6.0, 30.0, 54.0] {
            v.push(Vec3::new(x, SEWER_CEILING - 1.0, lane_z(k)));
        }
    }
    v
}

/// Pick the racing line's next waypoint for a racer at `pos` heading for waypoint `i`.
pub fn next_waypoint(pos: Vec3, i: usize) -> usize {
    let wps = waypoints();
    let i = i.min(wps.len() - 1);
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z);
    if flat(wps[i]).distance(flat(pos)) < 2.0 && i + 1 < wps.len() {
        i + 1
    } else {
        i
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocked(p: Vec3) -> bool {
        solids().iter().any(|s| {
            let e = s.expanded(0.5);
            p.x > e.min.x && p.x < e.max.x && p.z > e.min.z && p.z < e.max.z
        })
    }

    #[test]
    fn the_racing_line_is_clear_start_to_finish() {
        let wps = waypoints();
        assert!(finished(*wps.last().unwrap() + Vec3::Y * 0.1));
        assert!(!finished(wps[0]));
        for w in wps.windows(2) {
            for i in 0..=100 {
                let p = w[0].lerp(w[1], i as f32 / 100.0) + Vec3::Y * 0.1;
                assert!(!blocked(p), "blocked at {p}");
            }
        }
    }

    #[test]
    fn the_start_is_under_the_hole() {
        let below = Vec3::new(0.0, SEWER_FLOOR + 0.1, 0.0);
        assert!(!blocked(below));
        assert!(floor().max.y == SEWER_FLOOR && floor().min.x < -HOLE_HALF && floor().max.x > HOLE_HALF);
        for slot in 0..8 {
            let d = drop_point(slot);
            assert!(d.x.abs() < HOLE_HALF + 1.0 && d.z.abs() < HOLE_HALF + 1.0);
            assert!(!blocked(d.with_y(SEWER_FLOOR + 0.1)));
        }
    }

    #[test]
    fn progress_grows_along_the_course() {
        let wps = waypoints();
        let mut last = -1.0;
        for w in wps.windows(2) {
            for i in 0..10 {
                let p = w[0].lerp(w[1], i as f32 / 10.0) + Vec3::Y * 0.1;
                let now = progress(p);
                assert!(now + 0.5 >= last, "{p}: {now} after {last}");
                last = now;
            }
        }
        assert!(last > 250.0, "a long course: {last}");
        assert_eq!(progress(Vec3::new(0.0, 0.0, 0.0)), 0.0, "not down the hole yet");
    }

    #[test]
    fn every_lane_has_something_to_grind() {
        let rails = rails();
        for k in 0..LANES {
            let z = lane_z(k);
            assert!(rails.iter().any(|r| (r.a.z - z).abs() < LANE_W / 2.0), "lane {k}");
        }
        assert!(rails.len() > RAILS.len(), "low walls are ledges too");
    }

    #[test]
    fn following_waypoints_reaches_the_end() {
        let wps = waypoints();
        let (mut pos, mut i) = (wps[0] + Vec3::Y * 0.01, 0);
        for _ in 0..5000 {
            i = next_waypoint(pos, i);
            let d = Vec3::new(wps[i].x - pos.x, 0.0, wps[i].z - pos.z);
            pos += d.normalize_or_zero() * 0.5;
            if finished(pos) {
                return;
            }
        }
        panic!("never finished: at {pos}, waypoint {i}");
    }
}
