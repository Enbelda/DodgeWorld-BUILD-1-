//! On-screen text: score, connection/lobby info, vitals, kill feed, crosshair, controls.

use bevy::prelude::*;

use crate::camera::CursorLocked;
use crate::player::Charge;
use crate::session::{SessionSet, View};
use crate::sim::{Phase, Team, KO_TARGET, MAX_HP, MAX_PLAYERS};

#[derive(Component)]
enum HudText {
    Score,
    Info,
    Vitals,
    Feed,
    Crosshair,
}

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_hud).add_systems(Update, update_hud.after(SessionSet));
    }
}

const CONTROLS: &str = "WASD move   Space jump   Mouse aim\n\
Hold LMB: charge, release: throw   Q/E: curve   F: lob\n\
RMB: catch (face the ball)   Esc: free mouse";

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
                    let ball = match (m.holding, m.powered) {
                        (true, true) => "BALL (POWERED!)",
                        (true, false) => "BALL",
                        _ => "no ball",
                    };
                    let catch = if m.catching {
                        "CATCHING".to_string()
                    } else if m.catch_cooldown > 0.0 {
                        format!("catch in {:.1}s", m.catch_cooldown)
                    } else {
                        "catch ready".to_string()
                    };
                    let mut s = format!("HP {hearts}  {ball}  {catch}");
                    if let Some(c) = charge.amount(time.elapsed_secs()) {
                        s.push_str(&format!("\nPOWER {}", bar(c, 20)));
                    }
                    s
                }
            },
            HudText::Feed => view.feed.iter().map(|(_, m)| m.as_str()).collect::<Vec<_>>().join("\n"),
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

#[cfg(test)]
mod tests {
    use super::bar;

    #[test]
    fn bar_fills() {
        assert_eq!(bar(0.0, 4), "[....]");
        assert_eq!(bar(0.5, 4), "[||..]");
        assert_eq!(bar(2.0, 4), "[||||]");
    }
}
