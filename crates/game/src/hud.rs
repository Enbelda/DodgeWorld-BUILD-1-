//! On-screen text: score, connection/lobby info, vitals, kill feed, crosshair, controls.

use bevy::prelude::*;

use bevy::window::PrimaryWindow;

use crate::camera::{CursorLocked, FollowCamera};
use crate::player::{Charge, LockOn};
use crate::session::{SessionSet, View};
use crate::sim::ball::BallKind;
use crate::sim::{BallState, Phase, PlayerId, PlayerView, Snapshot, Team, KO_TARGET, MAX_HP, MAX_PLAYERS};

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

/// "!" that orbits the crosshair, pointing at the closest incoming enemy ball.
#[derive(Component)]
struct IncomingMarker;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_hud).add_systems(Update, (update_hud, place_incoming_marker).after(SessionSet));
    }
}

const CONTROLS: &str = "WASD move   Space jump (x2 in air)   Mouse aim\n\
Hold LMB: charge, release: throw   Q/E: curve   F: lob\n\
Hold RMB: catch (face the ball)   Tab/MMB: lock on   Esc: free mouse";

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

fn update_hud(
    time: Res<Time>,
    view: Res<View>,
    charge: Res<Charge>,
    locked: Res<CursorLocked>,
    lock: Res<LockOn>,
    mut texts: Query<(&HudText, &mut Text, &mut TextColor)>,
) {
    let snap = view.snap.as_ref();
    let me = view.me();
    for (kind, mut text, mut color) in &mut texts {
        let s = match kind {
            HudText::Score => match snap.map(|s| (s.phase, s.score)) {
                None => String::new(),
                Some((Phase::Lobby, _)) => "WARM-UP".to_string(),
                Some((Phase::Playing, [r, b])) => format!("RED {r}  -  {b} BLUE      first to {KO_TARGET}"),
                Some((Phase::Ended { winner }, [r, b])) => {
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
                        lines.push(format!("  {:?}  {}{bot}{you}   KO {}  out {}", p.team, p.name, p.kos, p.deaths));
                    }
                    let host_keys = match s.phase {
                        Phase::Lobby => "Enter: start match   B: add bot   N: remove bot",
                        Phase::Ended { .. } => "Enter: back to warm-up",
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
                Some(m) if !m.alive() => format!("KNOCKED OUT - back in {:.1}s", m.respawn_timer),
                Some(m) => {
                    let hearts = format!("{}{}", "O ".repeat(m.hp as usize), "- ".repeat((MAX_HP - m.hp) as usize));
                    let kind = held_kind(snap, m.id).map_or("", |k| match k {
                        BallKind::Normal => "",
                        BallKind::Boomerang => " BOOMERANG",
                        BallKind::Slime => " SLIME",
                    });
                    let ball = match (m.holding, m.powered, m.charged) {
                        (true, true, _) => format!("BALL{kind} (POWERED!)"),
                        (true, _, true) => format!("BALL{kind} (CHARGED)"),
                        (true, _, _) => format!("BALL{kind}"),
                        _ => "no ball".to_string(),
                    };
                    let catch = if m.catching { "CATCHING" } else { "hold RMB to catch" };
                    let mut s = format!("HP {hearts}  {ball}  {catch}");
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

fn held_kind(snap: Option<&Snapshot>, id: PlayerId) -> Option<BallKind> {
    snap?.balls.iter().find(|b| b.state == BallState::Held { by: id }).map(|b| b.kind)
}

/// Warning lines for the local player.
fn alerts(s: &Snapshot, me: &PlayerView, my_lock: Option<PlayerId>) -> String {
    let name = |id: PlayerId| s.players.iter().find(|p| p.id == id).map_or("?".to_string(), |p| p.name.clone());
    let mut lines = Vec::new();
    if me.slimed > 0.0 {
        lines.push(format!("SLIMED! stuck {:.1}s", me.slimed));
    }
    if !incoming_balls(s, me).is_empty() {
        lines.push(if me.catching { "INCOMING - catching!".to_string() } else { "INCOMING!  hold RMB".to_string() });
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
        let live = BallState::Live { thrower: foe, team: team_foe, curve: 0.0, lob: false, powered: false, homing: None };
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
