//! Glue between Bevy and the match: the host steps the simulation and serves peers,
//! a client talks to its host. Both end up with a `View` the renderer and HUD read.

use bevy::prelude::*;

use crate::camera::FollowCamera;
use crate::config::{LaunchConfig, Mode};
use crate::net::{ClientNet, ClientStatus, HostNet};
use crate::player::{LocalBody, FEET_OFFSET};
use crate::sim::{Action, GameEvent, GameMode, Match, MapId, Phase, PlayerId, Snapshot, TrickState};

const SIM_DT: f32 = 1.0 / 60.0;
const SEND_INTERVAL: f32 = 1.0 / 30.0;
const FEED_SECS: f64 = 5.0;

pub struct HostSession {
    pub sim: Match,
    pub net: Option<HostNet>,
    pub local: PlayerId,
    accum: f32,
    send_accum: f32,
    /// Events not yet sent to peers.
    unsent: Vec<GameEvent>,
}

#[derive(Resource)]
pub enum Session {
    Host(Box<HostSession>),
    Client(Box<ClientNet>),
}

/// What the local machine knows about the match this frame.
#[derive(Resource, Default)]
pub struct View {
    pub snap: Option<Snapshot>,
    pub local: Option<PlayerId>,
    pub is_host: bool,
    /// Connection line for the HUD.
    pub status: String,
    /// (time, message) kill feed.
    pub feed: Vec<(f64, String)>,
}

impl View {
    pub fn me(&self) -> Option<&crate::sim::PlayerView> {
        let id = self.local?;
        self.snap.as_ref()?.players.iter().find(|p| p.id == id)
    }
    pub fn name_of(&self, id: PlayerId) -> String {
        self.snap
            .as_ref()
            .and_then(|s| s.players.iter().find(|p| p.id == id))
            .map(|p| p.name.clone())
            .unwrap_or_else(|| format!("#{id}"))
    }
}

/// Actions the local player made this frame.
#[derive(Resource, Default)]
pub struct LocalActions(pub Vec<Action>);

/// The air trick the local player is doing, reported to the host for others to see.
#[derive(Resource, Default)]
pub struct LocalTrick(pub Option<TrickState>);

pub fn build_session(cfg: &LaunchConfig) -> Result<(Session, String), String> {
    match &cfg.mode {
        Mode::Host { lan } => {
            let mut sim = Match::new();
            let local = sim.add_player(&cfg.name, false).expect("empty match has room");
            for i in 0..cfg.bots {
                sim.add_player(&format!("Bot {}", i + 1), true);
            }
            if cfg.deathmatch {
                sim.set_mode(GameMode::Deathmatch);
            }
            if cfg.night {
                sim.set_map(MapId::Night);
            }
            let (net, status) = if *lan {
                let ip = cfg.bind.unwrap_or([0, 0, 0, 0].into());
                let net = HostNet::bind_addr((ip, cfg.port).into())
                    .map_err(|e| format!("could not open port {}: {e}", cfg.port))?;
                (Some(net), format!("Hosting LAN lobby on port {}", cfg.port))
            } else {
                (None, "Offline practice".to_string())
            };
            let host = HostSession { sim, net, local, accum: 0.0, send_accum: 0.0, unsent: Vec::new() };
            Ok((Session::Host(Box::new(host)), status))
        }
        Mode::Join(addr) => {
            let net = ClientNet::connect(addr, &cfg.name).map_err(|e| format!("could not join {addr}: {e}"))?;
            let status = format!("Joining {}", net.host_addr());
            Ok((Session::Client(Box::new(net)), status))
        }
    }
}

pub struct SessionPlugin;

impl Plugin for SessionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<View>()
            .init_resource::<LocalActions>()
            .init_resource::<LocalTrick>()
            .add_systems(Update, (host_controls, tick_session).chain().in_set(SessionSet));
    }
}

/// Runs after local input and before rendering.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct SessionSet;

fn host_controls(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    auto: Res<crate::player::Autopilot>,
    mut session: ResMut<Session>,
) {
    let Session::Host(host) = session.as_mut() else { return };
    // Automated play-tests start by themselves: offline at once, on LAN once a peer joined.
    let peers_ready = host.net.as_ref().is_none_or(|n| n.peer_count() > 0);
    let sim = &mut host.sim;
    match sim.phase {
        Phase::Lobby | Phase::Ended { .. } if keys.just_pressed(KeyCode::KeyM) => {
            let next = match sim.mode {
                GameMode::Standard => GameMode::Deathmatch,
                GameMode::Deathmatch => GameMode::Standard,
            };
            sim.set_mode(next);
        }
        Phase::Lobby | Phase::Ended { .. } if keys.just_pressed(KeyCode::KeyV) => {
            let next = match sim.map {
                MapId::Day => MapId::Night,
                MapId::Night => MapId::Day,
            };
            sim.set_map(next);
        }
        Phase::Lobby => {
            if auto.0 && time.elapsed_secs() > 3.0 && peers_ready {
                sim.start();
            }
            if keys.just_pressed(KeyCode::KeyB) {
                let n = sim.players.iter().filter(|p| p.bot).count() + 1;
                sim.add_player(&format!("Bot {n}"), true);
            }
            if keys.just_pressed(KeyCode::KeyN) {
                sim.remove_bot();
            }
            if keys.just_pressed(KeyCode::Enter) {
                sim.start();
            }
        }
        Phase::Ended { .. } => {
            if keys.just_pressed(KeyCode::Enter) {
                sim.to_lobby();
            }
        }
        Phase::Playing => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn tick_session(
    time: Res<Time>,
    mut session: ResMut<Session>,
    mut view: ResMut<View>,
    mut actions: ResMut<LocalActions>,
    trick: Res<LocalTrick>,
    dash: Res<crate::player::LocalDash>,
    body: Single<&Transform, With<LocalBody>>,
    cam: Single<&FollowCamera>,
) {
    let now = time.elapsed_secs_f64();
    let feet = body.translation - Vec3::Y * FEET_OFFSET;
    let yaw = cam.yaw;
    let mut new_events = Vec::new();

    match session.as_mut() {
        Session::Host(host) => {
            let local = host.local;
            host.sim.set_pose(local, feet, yaw);
            host.sim.set_trick(local, trick.0);
            host.sim.set_dash(local, dash.report());
            for a in actions.0.drain(..) {
                host.sim.apply(local, a);
            }
            if let Some(net) = host.net.as_mut() {
                net.receive(&mut host.sim, now);
            }
            host.accum = (host.accum + time.delta_secs()).min(0.25);
            while host.accum >= SIM_DT {
                host.sim.step(SIM_DT);
                host.accum -= SIM_DT;
            }
            new_events = std::mem::take(&mut host.sim.events);
            host.unsent.extend(new_events.iter().cloned());

            let mut snap = host.sim.snapshot();
            host.send_accum += time.delta_secs();
            if host.send_accum >= SEND_INTERVAL {
                host.send_accum = 0.0;
                if let Some(net) = host.net.as_ref() {
                    snap.events = std::mem::take(&mut host.unsent);
                    net.broadcast(&snap);
                } else {
                    host.unsent.clear();
                }
            }
            snap.events.clear();
            view.snap = Some(snap);
            view.local = Some(local);
            view.is_host = true;
            if let Some(net) = host.net.as_ref() {
                let base = view.status.split(" | ").next().unwrap_or("").to_string();
                view.status = format!("{base} | {} peer(s)", net.peer_count());
            }
        }
        Session::Client(net) => {
            for a in actions.0.drain(..) {
                net.push_action(a);
            }
            net.send_update(feet, yaw, trick.0, dash.report(), now);
            if net.receive(now) {
                if let Some(s) = net.latest.as_mut() {
                    new_events = std::mem::take(&mut s.events);
                }
                view.snap = net.latest.clone();
            }
            view.local = net.id();
            view.is_host = false;
            view.status = match &net.status {
                ClientStatus::Connecting => format!("Joining {} ...", net.host_addr()),
                ClientStatus::Connected(_) => format!("Connected to {}", net.host_addr()),
                ClientStatus::Rejected(r) => format!("Host refused: {r}"),
                ClientStatus::Lost => "Connection to host lost".to_string(),
            };
        }
    }

    for e in new_events {
        let msg = match e {
            GameEvent::Hit { thrower, target } => format!("{} hit {}", view.name_of(thrower), view.name_of(target)),
            GameEvent::Knockout { thrower, target } => {
                format!("{} KNOCKED OUT {}", view.name_of(thrower), view.name_of(target))
            }
            GameEvent::Caught { catcher, thrower, perfect } => {
                let how = if perfect { "PERFECT-caught" } else { "caught" };
                format!("{} {how} {}'s throw!", view.name_of(catcher), view.name_of(thrower))
            }
            GameEvent::Tackled { by, target } => {
                format!("{} TACKLED {}!", view.name_of(by), view.name_of(target))
            }
            GameEvent::KnockedBack { target } => format!("{} got knocked back by a ricochet", view.name_of(target)),
            GameEvent::Slimed { thrower, target } => {
                format!("{} got SLIMED by {}!", view.name_of(target), view.name_of(thrower))
            }
            GameEvent::Stunned { thrower, target } => {
                format!("{} got BRICKED by {}!", view.name_of(target), view.name_of(thrower))
            }
            GameEvent::BoomerangBack { owner } => format!("{}'s boomerang came back charged", view.name_of(owner)),
            GameEvent::MatchStarted => "Match started!".to_string(),
            GameEvent::MatchEnded { winner } => format!("{winner:?} team wins!"),
        };
        info!("feed: {msg}");
        view.feed.push((now, msg));
    }
    view.feed.retain(|(t, _)| now - t < FEED_SECS);
    let len = view.feed.len();
    if len > 6 {
        view.feed.drain(..len - 6);
    }
}

/// Tell the host we left when the window closes.
pub fn leave_on_exit(mut exit: MessageReader<AppExit>, session: Res<Session>) {
    if exit.read().next().is_some() {
        if let Session::Client(net) = session.as_ref() {
            net.leave();
        }
    }
}
