//! On-screen text: score, connection/lobby info, vitals, kill feed, crosshair, controls.

use bevy::prelude::*;

use bevy::window::PrimaryWindow;

use crate::camera::{CursorLocked, FollowCamera};
use crate::player::{Charge, LocalDash, LockOn, MoveIntent, DASH_CHARGES, DASH_RECHARGE};
use crate::session::{SessionSet, View};
use crate::sim::ball::BallKind;
use crate::sim::{
    BallState, GameMode, MapId, Phase, PlayerId, PlayerView, Snapshot, Team, HEARTS, KO_TARGET, MAX_PLAYERS,
    PERFECT_SECS,
};

/// Enemy balls further than this are not shown as incoming.
const INCOMING_RANGE: f32 = 30.0;
/// Radius of the incoming-ball marker ring around the crosshair, in pixels.
const MARKER_RING: f32 = 110.0;

#[derive(Component)]
enum HudText {
    Score,
    Info,
    Vitals,
    Feed,
    Crosshair,
    /// Big warnings under the crosshair: incoming ball, locked on, slimed.
    Alert,
}

/// Flashing red ring around the crosshair: press catch now for a perfect catch.
#[derive(Component)]
struct PerfectRing;

/// The red part of heart `0..HEARTS`; its width shows full, half or empty.
#[derive(Component)]
struct HeartFill(u8);

/// Row of hearts, hidden when there is nothing to show.
#[derive(Component)]
struct HeartRow;

const HEART_SIZE: f32 = 30.0;

/// A heart from three shapes: two round lobes on top and a square turned 45 degrees for
/// the point.
fn spawn_heart_shape(parent: &mut ChildSpawnerCommands, color: Color) {
    let lobe = HEART_SIZE * 0.54;
    let at = |left: f32, top: f32, size: f32| Node {
        position_type: PositionType::Absolute,
        left: Val::Px(left),
        top: Val::Px(top),
        width: Val::Px(size),
        height: Val::Px(size),
        border_radius: BorderRadius::MAX,
        ..default()
    };
    parent.spawn((at(0.0, 0.0, lobe), BackgroundColor(color)));
    parent.spawn((at(HEART_SIZE - lobe, 0.0, lobe), BackgroundColor(color)));
    let point = HEART_SIZE * 0.55;
    parent.spawn((
        Node { border_radius: BorderRadius::ZERO, ..at((HEART_SIZE - point) / 2.0, HEART_SIZE * 0.24, point) },
        BackgroundColor(color),
        UiTransform::from_rotation(Rot2::degrees(45.0)),
    ));
}

/// How full heart `index` is (0, 0.5 or 1) for `hp` half-hearts.
pub fn heart_fill(hp: u8, index: u8) -> f32 {
    let halves = hp.saturating_sub(index * 2).min(2);
    halves as f32 / 2.0
}

/// "!" that orbits the crosshair, pointing at the closest incoming enemy ball.
#[derive(Component)]
struct IncomingMarker;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_hud)
            .add_systems(Update, (update_hud, update_hearts, place_incoming_marker, perfect_ring).after(SessionSet));
    }
}

const CONTROLS: &str = "WASD move   Shift sprint   Ctrl dash/tackle   Space jump (x2 in air)\n\
Hold LMB: charge (locks nearest), release: throw   Q/E: curve   F: lob\n\
RMB: catch - time it! red ring = PERFECT   Tab/MMB while charging: switch target\n\
In the air: Q/E spin 180   W+Q/E front flip   S+Q/E back flip   Esc: free mouse";

fn spawn_hud(mut commands: Commands) {
    let text = |s: &str, size: f32| (Text::new(s), TextFont::from_font_size(size), TextColor(Color::WHITE));
    let abs = |top: Option<f32>, bottom: Option<f32>, left: Option<f32>, right: Option<f32>| Node {
        position_type: PositionType::Absolute,
        top: top.map_or(Val::Auto, Val::Px),
        bottom: bottom.map_or(Val::Auto, Val::Px),
        left: left.map_or(Val::Auto, Val::Px),
        right: right.map_or(Val::Auto, Val::Px),
        ..default()
    };
    // Score bar, centred along the top.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Px(10.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|c| {
            c.spawn((text("", 30.0), HudText::Score));
        });
    commands.spawn((text("", 16.0), abs(Some(10.0), None, Some(12.0), None), HudText::Info));
    commands.spawn((text("", 22.0), abs(None, Some(12.0), Some(12.0), None), HudText::Vitals));
    // Hearts, just above the vitals line. Each heart: a dark heart with a red one on top,
    // clipped to 0%, 50% or 100% width. Drawn from shapes so no special font is needed.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(72.0),
                left: Val::Px(12.0),
                column_gap: Val::Px(4.0),
                ..default()
            },
            Visibility::Hidden,
            HeartRow,
        ))
        .with_children(|row| {
            for i in 0..HEARTS {
                let slot_node = Node {
                    position_type: PositionType::Relative,
                    width: Val::Px(HEART_SIZE),
                    height: Val::Px(HEART_SIZE),
                    ..default()
                };
                row.spawn(slot_node).with_children(|slot| {
                    spawn_heart_shape(slot, Color::srgba(0.08, 0.08, 0.1, 0.75));
                    slot.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            top: Val::Px(0.0),
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        HeartFill(i),
                    ))
                    .with_children(|clip| {
                        // Full-size box inside the clip so the heart keeps its shape when cut.
                        clip.spawn(Node {
                            position_type: PositionType::Absolute,
                            width: Val::Px(HEART_SIZE),
                            height: Val::Px(HEART_SIZE),
                            ..default()
                        })
                        .with_children(|inner| spawn_heart_shape(inner, Color::srgb(0.95, 0.15, 0.2)));
                    });
                });
            }
        });
    commands.spawn((text("", 18.0), abs(Some(10.0), None, None, Some(12.0)), HudText::Feed));
    commands.spawn((
        Text::new(CONTROLS),
        TextFont::from_font_size(14.0),
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
        abs(None, Some(12.0), None, Some(12.0)),
    ));
    // Crosshair at the exact screen centre.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|c| {
            c.spawn((text("+", 26.0), HudText::Crosshair));
            c.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Px(84.0),
                    height: Val::Px(84.0),
                    border: UiRect::all(Val::Px(5.0)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BorderColor::all(Color::srgb(1.0, 0.1, 0.1)),
                Visibility::Hidden,
                PerfectRing,
            ));
        });
    // Alerts, a bit below the crosshair.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Percent(60.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|c| {
            c.spawn((text("", 26.0), HudText::Alert));
        });
    commands.spawn((
        Text::new("!"),
        TextFont::from_font_size(44.0),
        TextColor(Color::srgb(1.0, 0.2, 0.15)),
        Node { position_type: PositionType::Absolute, ..default() },
        Visibility::Hidden,
        IncomingMarker,
    ));
}

/// Enemy balls flying at `me` (or homing on `me`), closest first.
pub fn incoming_balls(snap: &Snapshot, me: &PlayerView) -> Vec<Vec3> {
    let chest = Vec3::from_array(me.pos) + Vec3::Y * 1.1;
    let mut v: Vec<(f32, Vec3)> = snap
        .balls
        .iter()
        .filter_map(|b| match b.state {
            BallState::Live { team, homing, .. } if team != me.team => {
                let pos = Vec3::from_array(b.pos);
                let to_me = chest - pos;
                let dir = Vec3::from_array(b.vel).normalize_or_zero();
                let aimed = dir.dot(to_me.normalize_or_zero()) > 0.8;
                let dist = to_me.length();
                (dist < INCOMING_RANGE && (aimed || homing == Some(me.id))).then_some((dist, pos))
            }
            _ => None,
        })
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v.into_iter().map(|(_, p)| p).collect()
}

/// Screen offset (pixels, +x right, +y down) for a marker pointing at `target` from `from`
/// when the camera looks along `yaw`.
pub fn marker_offset(from: Vec3, target: Vec3, yaw: f32) -> Vec2 {
    let d = Vec3::new(target.x - from.x, 0.0, target.z - from.z).normalize_or_zero();
    let forward = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
    let right = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
    Vec2::new(d.dot(right), -d.dot(forward)).normalize_or(Vec2::NEG_Y) * MARKER_RING
}

fn place_incoming_marker(
    view: Res<View>,
    cam: Single<&FollowCamera>,
    window: Single<&Window, With<PrimaryWindow>>,
    marker: Single<(&mut Node, &mut Visibility), With<IncomingMarker>>,
) {
    let (mut node, mut vis) = marker.into_inner();
    let closest = view
        .snap
        .as_ref()
        .zip(view.me())
        .and_then(|(s, me)| incoming_balls(s, me).first().map(|b| (Vec3::from_array(me.pos), *b)));
    let Some((me, ball)) = closest else {
        *vis = Visibility::Hidden;
        return;
    };
    let off = marker_offset(me, ball, cam.yaw);
    node.left = Val::Px(window.width() / 2.0 + off.x - 8.0);
    node.top = Val::Px(window.height() / 2.0 + off.y - 26.0);
    *vis = Visibility::Visible;
}

fn bar(fraction: f32, width: usize) -> String {
    let filled = (fraction.clamp(0.0, 1.0) * width as f32).round() as usize;
    format!("[{}{}]", "|".repeat(filled), ".".repeat(width - filled))
}

#[allow(clippy::too_many_arguments)]
fn update_hud(
    time: Res<Time>,
    view: Res<View>,
    charge: Res<Charge>,
    locked: Res<CursorLocked>,
    lock: Res<LockOn>,
    dash: Res<LocalDash>,
    intent: Res<MoveIntent>,
    mut texts: Query<(&HudText, &mut Text, &mut TextColor)>,
) {
    let snap = view.snap.as_ref();
    let me = view.me();
    for (kind, mut text, mut color) in &mut texts {
        let s = match kind {
            HudText::Score => match snap.map(|s| (s.phase, s.score, s.mode)) {
                None => String::new(),
                Some((Phase::Lobby, _, _)) => "WARM-UP".to_string(),
                Some((Phase::Playing, _, GameMode::Deathmatch)) => {
                    let alive = |t: Team| snap.map_or(0, |s| s.players.iter().filter(|p| p.team == t && p.alive()).count());
                    format!("DEATHMATCH   RED {} standing  -  {} standing BLUE", alive(Team::Red), alive(Team::Blue))
                }
                Some((Phase::Playing, [r, b], GameMode::Standard)) => {
                    format!("RED {r}  -  {b} BLUE      first to {KO_TARGET}")
                }
                Some((Phase::Ended { winner }, [r, b], _)) => {
                    let w = if winner == Team::Red { "RED" } else { "BLUE" };
                    format!("{w} WINS   {r} - {b}")
                }
            },
            HudText::Info => {
                let mut lines = vec![view.status.clone()];
                if let Some(s) = snap {
                    lines.push(format!("Players {}/{MAX_PLAYERS}:", s.players.len()));
                    for p in &s.players {
                        let you = if Some(p.id) == view.local { " (you)" } else { "" };
                        let bot = if p.bot { " [bot]" } else { "" };
                        let hearts = p.hp as f32 / 2.0;
                        lines.push(format!(
                            "  {:?}  {}{bot}{you}   {hearts:.1} hearts   KO {}  out {}",
                            p.team, p.name, p.kos, p.deaths
                        ));
                    }
                    let mode = match s.mode {
                        GameMode::Standard => "Standard (respawns, first to 10)",
                        GameMode::Deathmatch => "Deathmatch (no respawns)",
                    };
                    let map = match s.map {
                        MapId::Day => "Day arena",
                        MapId::Night => "Night arena",
                    };
                    lines.push(format!("Mode: {mode}   Map: {map}"));
                    let host_keys = match s.phase {
                        Phase::Lobby => "Enter: start   B/N: add/remove bot   M: mode   V: map",
                        Phase::Ended { .. } => "Enter: back to warm-up   M: mode   V: map",
                        Phase::Playing => "",
                    };
                    if view.is_host && !host_keys.is_empty() {
                        lines.push(host_keys.to_string());
                    } else if !view.is_host && s.phase == Phase::Lobby {
                        lines.push("Waiting for the host to start...".to_string());
                    }
                }
                if !locked.0 {
                    lines.push("Click to capture the mouse".to_string());
                }
                lines.join("\n")
            }
            HudText::Vitals => match me {
                None => String::new(),
                Some(m) if !m.alive() => {
                    let out_for_good = snap.is_some_and(|s| s.mode == GameMode::Deathmatch && s.phase == Phase::Playing);
                    if out_for_good {
                        "OUT - spectating until the match ends".to_string()
                    } else {
                        format!("KNOCKED OUT - back in {:.1}s", m.respawn_timer)
                    }
                }
                Some(m) => {
                    let kind = held_kind(snap, m.id).map_or("", |k| match k {
                        BallKind::Normal => "",
                        BallKind::Boomerang => " BOOMERANG",
                        BallKind::Slime => " SLIME",
                        BallKind::Brick => " BRICK",
                    });
                    let ball = match (m.holding, m.powered, m.charged) {
                        (true, true, _) => format!("BALL{kind} (POWERED!)"),
                        (true, _, true) => format!("BALL{kind} (CHARGED)"),
                        (true, _, _) => format!("BALL{kind}"),
                        _ => "no ball".to_string(),
                    };
                    let catch = if m.catching {
                        "CATCHING".to_string()
                    } else if m.catch_cooldown > 0.0 {
                        format!("catch in {:.1}s", m.catch_cooldown)
                    } else {
                        "catch ready".to_string()
                    };
                    let stacks = snap
                        .and_then(|s| s.balls.iter().find(|b| b.state == BallState::Held { by: m.id }))
                        .map_or(0, |b| b.stacks);
                    let speed = if stacks > 0 { format!("  x{} SPEED", 1 + stacks) } else { String::new() };
                    let dashes = if dash.charges == 0 {
                        format!("DASH in {:.1}s", (DASH_RECHARGE - dash.since_last).max(0.0))
                    } else {
                        format!("DASH {}{}", "[#] ".repeat(dash.charges as usize), "[ ] ".repeat((DASH_CHARGES - dash.charges) as usize))
                    };
                    let sprint = if intent.sprint { "  SPRINT" } else { "" };
                    let mut s = format!("{ball}{speed}   {catch}\n{dashes}{sprint}");
                    if let Some(c) = charge.amount(time.elapsed_secs()) {
                        s.push_str(&format!("\nPOWER {}", bar(c, 20)));
                    }
                    s
                }
            },
            HudText::Feed => view.feed.iter().map(|(_, m)| m.as_str()).collect::<Vec<_>>().join("\n"),
            HudText::Alert => match (snap, me) {
                (Some(s), Some(m)) if m.alive() => alerts(s, m, lock.0),
                _ => String::new(),
            },
            HudText::Crosshair => {
                color.0 = if me.is_some_and(|m| m.holding) { Color::srgb(1.0, 0.9, 0.3) } else { Color::WHITE };
                "+".to_string()
            }
        };
        if text.0 != s {
            text.0 = s;
        }
    }
}

/// Seconds until the closest enemy ball reaches `me`, if one is flying at them.
pub fn closest_eta(snap: &Snapshot, me: &PlayerView) -> Option<f32> {
    let chest = Vec3::from_array(me.pos) + Vec3::Y * 1.1;
    snap.balls
        .iter()
        .filter(|b| matches!(b.state, BallState::Live { team, .. } if team != me.team) || matches!(b.state, BallState::Ricochet { .. }))
        .filter_map(|b| {
            let pos = Vec3::from_array(b.pos);
            let vel = Vec3::from_array(b.vel);
            let to_me = chest - pos;
            let closing = vel.dot(to_me.normalize_or_zero());
            (closing > 1.0 && vel.normalize_or_zero().dot(to_me.normalize_or_zero()) > 0.8)
                .then(|| (to_me.length() - 0.75).max(0.0) / closing)
        })
        .min_by(|a, b| a.total_cmp(b))
}

/// Show the perfect-catch ring while a catch pressed now would be perfect.
fn perfect_ring(
    time: Res<Time>,
    view: Res<View>,
    ring: Single<(&mut Visibility, &mut BorderColor), With<PerfectRing>>,
) {
    let (mut vis, mut color) = ring.into_inner();
    let ready = view.snap.as_ref().zip(view.me()).is_some_and(|(s, me)| {
        me.alive() && !me.holding && !me.catching && me.catch_cooldown <= 0.0
            && closest_eta(s, me).is_some_and(|eta| eta <= PERFECT_SECS)
    });
    *vis = if ready { Visibility::Inherited } else { Visibility::Hidden };
    let pulse = 0.55 + 0.45 * (time.elapsed_secs() * 40.0).sin();
    *color = BorderColor::all(Color::srgba(1.0, 0.1, 0.1, pulse));
}

fn update_hearts(
    view: Res<View>,
    row: Single<&mut Visibility, With<HeartRow>>,
    mut fills: Query<(&HeartFill, &mut Node)>,
) {
    let me = view.me().filter(|m| m.alive());
    *row.into_inner() = if me.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    let Some(me) = me else { return };
    for (h, mut node) in &mut fills {
        node.width = Val::Percent(heart_fill(me.hp, h.0) * 100.0);
    }
}

fn held_kind(snap: Option<&Snapshot>, id: PlayerId) -> Option<BallKind> {
    snap?.balls.iter().find(|b| b.state == BallState::Held { by: id }).map(|b| b.kind)
}

/// Warning lines for the local player.
fn alerts(s: &Snapshot, me: &PlayerView, my_lock: Option<PlayerId>) -> String {
    let name = |id: PlayerId| s.players.iter().find(|p| p.id == id).map_or("?".to_string(), |p| p.name.clone());
    let mut lines = Vec::new();
    if me.stunned > 0.0 {
        lines.push(format!("KNOCKED FLAT! up in {:.1}s", me.stunned));
    }
    if me.knocked {
        lines.push("LAUNCHED!".to_string());
    }
    if me.slimed > 0.0 {
        lines.push(format!("SLIMED! stuck and helpless {:.1}s", me.slimed));
    }
    if !incoming_balls(s, me).is_empty() {
        lines.push(if me.catching { "INCOMING - catching!".to_string() } else { "INCOMING!  RMB to catch".to_string() });
    }
    let hunters: Vec<String> = s.players.iter().filter(|p| p.lock == Some(me.id)).map(|p| p.name.clone()).collect();
    if !hunters.is_empty() {
        lines.push(format!("LOCKED ON BY {}", hunters.join(", ")));
    }
    if let Some(t) = my_lock {
        lines.push(format!("locked: {}", name(t)));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eta_only_for_balls_coming_at_me() {
        let mut m = crate::sim::Match::new();
        let me = m.add_player("me", false).unwrap();
        let foe = m.add_player("foe", false).unwrap();
        m.set_pose(me, Vec3::new(14.0, 0.0, 0.0), 0.0);
        m.balls.truncate(1);
        let team_foe = m.player(foe).unwrap().team;
        m.balls[0].pos = Vec3::new(14.0, 1.1, -10.75);
        m.balls[0].vel = Vec3::new(0.0, 0.0, 20.0);
        m.balls[0].state = BallState::Live {
            thrower: foe, team: team_foe, curve: 0.0, lob: false, powered: false, homing: None, track: None,
        };
        let snap = m.snapshot();
        let mine = snap.players.iter().find(|p| p.id == me).unwrap();
        let eta = closest_eta(&snap, mine).unwrap();
        assert!((eta - 0.5).abs() < 0.01, "10 m at 20 m/s: {eta}");
        m.balls[0].vel = Vec3::new(0.0, 0.0, -20.0);
        let snap = m.snapshot();
        let mine = snap.players.iter().find(|p| p.id == me).unwrap();
        assert!(closest_eta(&snap, mine).is_none(), "flying away");
    }

    #[test]
    fn hearts_fill_in_halves() {
        // 8 half-hearts = 4 full hearts.
        assert_eq!((0..4).map(|i| heart_fill(8, i)).collect::<Vec<_>>(), [1.0, 1.0, 1.0, 1.0]);
        // One hit: last heart half gone.
        assert_eq!((0..4).map(|i| heart_fill(7, i)).collect::<Vec<_>>(), [1.0, 1.0, 1.0, 0.5]);
        // A brick (4 halves) from full: two hearts left.
        assert_eq!((0..4).map(|i| heart_fill(4, i)).collect::<Vec<_>>(), [1.0, 1.0, 0.0, 0.0]);
        assert_eq!(heart_fill(0, 0), 0.0);
    }

    #[test]
    fn marker_points_where_the_ball_is() {
        let me = Vec3::ZERO;
        // Camera yaw 0 looks down -Z: a ball ahead is at the top, one to the right is right.
        let ahead = marker_offset(me, Vec3::new(0.0, 0.0, -10.0), 0.0);
        assert!(ahead.y < 0.0 && ahead.x.abs() < 1e-3);
        let right = marker_offset(me, Vec3::new(10.0, 0.0, 0.0), 0.0);
        assert!(right.x > 0.0 && right.y.abs() < 1e-3);
        let behind = marker_offset(me, Vec3::new(0.0, 0.0, 10.0), 0.0);
        assert!(behind.y > 0.0);
    }

    #[test]
    fn incoming_only_for_enemy_balls_aimed_at_me() {
        let mut m = crate::sim::Match::new();
        let me = m.add_player("me", false).unwrap();
        let foe = m.add_player("foe", false).unwrap();
        m.set_pose(me, Vec3::new(14.0, 0.0, 0.0), 0.0);
        m.balls.truncate(2);
        let team_foe = m.player(foe).unwrap().team;
        let live = BallState::Live {
            thrower: foe,
            team: team_foe,
            curve: 0.0,
            lob: false,
            powered: false,
            homing: None,
            track: None,
        };
        // Ball 0 flies at me, ball 1 flies away.
        m.balls[0].pos = Vec3::new(14.0, 1.1, -10.0);
        m.balls[0].vel = Vec3::new(0.0, 0.0, 30.0);
        m.balls[0].state = live;
        m.balls[1].pos = Vec3::new(14.0, 1.1, -10.0);
        m.balls[1].vel = Vec3::new(0.0, 0.0, -30.0);
        m.balls[1].state = live;
        let snap = m.snapshot();
        let mine = snap.players.iter().find(|p| p.id == me).unwrap();
        assert_eq!(incoming_balls(&snap, mine).len(), 1);
        let theirs = snap.players.iter().find(|p| p.id == foe).unwrap();
        assert!(incoming_balls(&snap, theirs).is_empty(), "own team's balls are not incoming");
    }


    #[test]
    fn bar_fills() {
        assert_eq!(bar(0.0, 4), "[....]");
        assert_eq!(bar(0.5, 4), "[||..]");
        assert_eq!(bar(2.0, 4), "[||||]");
    }
}
