//! Authoritative match simulation. Plain Rust, no ECS: the host owns one `Match`,
//! feeds it player poses and actions, steps it, and sends `snapshot()` to everyone.

pub mod ball;
pub mod bot;
pub mod layout;

use bevy::math::Vec3;
use serde::{Deserialize, Serialize};

use crate::mounts::{MountKind, TrickCat};
use ball::{BallKind, ThrowParams, BALL_RADIUS};
pub use layout::ArenaId;
use layout::Aabb;

pub type PlayerId = u8;
pub const MAX_PLAYERS: usize = 4;
/// Health in half-hearts: 4 hearts = 8 halves. A normal hit takes one half.
pub const MAX_HP: u8 = 8;
pub const HEARTS: u8 = 4;
/// How long a brick knocks its target flat.
pub const STUN_SECS: f32 = 5.0;
pub const KO_TARGET: u32 = 10;
pub const RESPAWN_SECS: f32 = 3.0;
pub const CATCH_WINDOW: f32 = 0.4;
pub const CATCH_COOLDOWN: f32 = 1.0;
pub const THROW_COOLDOWN: f32 = 0.35;
pub const PICKUP_RADIUS: f32 = 1.3;
/// Player body for hit tests: vertical capsule from feet to head.
pub const BODY_RADIUS: f32 = 0.45;
pub const BODY_HEIGHT: f32 = 1.8;
/// Crouched in cover the body is this tall (hidden behind a 1.2 m low wall).
pub const CROUCH_HEIGHT: f32 = 1.1;
/// Where a held ball sits while hanging from a ledge (cocked behind the head, over the
/// ledge), while crouched in cover, and risen up in cover to throw over the wall.
pub const HANG_HOLD_HEIGHT: f32 = 1.9;
pub const CROUCH_HOLD_HEIGHT: f32 = 0.85;
pub const COVER_HOLD_HEIGHT: f32 = 1.75;
/// Catching a ball pushes the catcher back by this fraction of its speed (m/s), capped.
pub const CATCH_PUSH_SCALE: f32 = 0.22;
pub const CATCH_PUSH_MAX: f32 = 9.0;
/// How quickly a catch's push-back fades (per second).
pub const RECOIL_DECAY: f32 = 6.0;
/// Landing a trick combo worth this much while holding a ball charges it (full power).
pub const STYLE_CHARGE_POINTS: u32 = 1000;
/// Combos worth at least this much show up in the feed.
pub const STYLE_FEED_POINTS: u32 = 500;
/// Where a held ball sits relative to the holder's feet.
pub const HOLD_HEIGHT: f32 = 1.3;
/// The ball must come at the catcher's front half to be caught.
const CATCH_FACING: f32 = 0.0;
/// Total time a slimed player is out of control (flying to the wall + stuck there).
pub const SLIME_SECS: f32 = 3.0;
/// How fast slime knocks a player toward the wall.
pub const SLIME_KNOCK_SPEED: f32 = 18.0;
/// Lock-on only works on enemies within this range.
pub const LOCK_RANGE: f32 = 40.0;
/// A catch pressed this close before the ball arrives is a perfect catch.
pub const PERFECT_SECS: f32 = 0.15;
/// Tackle: how close a dashing player must get, and how hard the target is launched.
pub const TACKLE_RANGE: f32 = 1.2;
pub const TACKLE_PUSH: f32 = 9.0;
pub const TACKLE_LIFT: f32 = 8.0;
/// Knockback from a ricocheting ball.
pub const RICOCHET_PUSH: f32 = 10.0;
pub const RICOCHET_LIFT: f32 = 5.0;
/// Gravity on players launched by a tackle or ricochet.
pub const KNOCK_GRAVITY: f32 = 22.0;
/// Curved throws track an enemy within this cone of the aim (radians).
const TRACK_CONE: f32 = 0.7;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Team {
    Red,
    Blue,
}

impl Team {
    pub fn other(self) -> Team {
        match self {
            Team::Red => Team::Blue,
            Team::Blue => Team::Red,
        }
    }
    pub fn index(self) -> usize {
        self as usize
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Warm-up: players run around and throw, KOs don't count.
    Lobby,
    Playing,
    Ended { winner: Team },
}

/// Something a player does that the host must apply exactly once.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Throw { aim: [f32; 3], charge: f32, curve: f32, lob: bool },
    /// Short catch window (bots). Humans hold the catch button instead, sent with their pose.
    Catch,
    /// Lock on to an enemy (or clear the lock).
    Lock { target: Option<PlayerId> },
    /// Landed a trick combo worth `points` on a mount.
    Style { points: u32 },
}

/// Standard: knocked-out players respawn, first team to `KO_TARGET` wins.
/// Deathmatch: no respawns, the last team with someone standing wins.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GameMode {
    #[default]
    Standard,
    Deathmatch,
}

/// Which look the arena has. Geometry is the same.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MapId {
    #[default]
    Day,
    Night,
}

/// Air tricks. Flips are animations; spins also turn the player 180 degrees.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrickKind {
    FrontFlip,
    BackFlip,
    SpinLeft,
    SpinRight,
}

/// A trick in progress (0..1), reported by the client so others can see it.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub struct TrickState {
    pub kind: TrickKind,
    pub progress: f32,
}

/// What a player is doing on a wall, reported by the client so others can see it.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub enum ParkourPose {
    #[default]
    None,
    /// Running along a wall on the player's left / right.
    WallRunLeft,
    WallRunRight,
    Climb,
    Hang,
    /// Pulling up onto a ledge.
    Mantle,
}

/// Taking cover behind a wall, reported by the client.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub enum CoverPose {
    #[default]
    None,
    /// Ducked behind it: a smaller target.
    Crouched,
    /// Risen up over it to throw.
    Up,
}

/// What a rider is on and doing, reported by the client so others can draw it.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub struct RideView {
    pub mount: MountKind,
    /// Trick in progress: kind, index in the mount's list, progress (1 = done).
    pub trick: Option<(TrickCat, u8, f32)>,
    /// Grind in progress (index in the mount's grinds).
    pub grind: Option<u8>,
    /// Off balance after a bail.
    pub bail: bool,
    pub speed: f32,
    /// Pushing / striding animation phase.
    pub push: f32,
}

impl RideView {
    /// Clamp what a client sent to sane values.
    pub fn sanitized(self) -> Option<RideView> {
        let ok = |v: f32| v.is_finite();
        if !ok(self.speed) || !ok(self.push) || self.trick.is_some_and(|t| !ok(t.2)) {
            return None;
        }
        Some(RideView {
            speed: self.speed.clamp(0.0, 40.0),
            push: self.push % 1000.0,
            trick: self.trick.map(|(c, i, p)| (c, i, p.clamp(0.0, 4.0))),
            ..self
        })
    }
}

/// A slimed player is dragged to `anchor` on the nearest wall and held there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slime {
    pub anchor: Vec3,
    pub timer: f32,
}

#[derive(Clone, Debug)]
pub struct Player {
    pub id: PlayerId,
    pub name: String,
    pub team: Team,
    pub bot: bool,
    /// Feet position.
    pub pos: Vec3,
    pub yaw: f32,
    pub hp: u8,
    pub holding: Option<usize>,
    /// The held ball was caught, so the next throw is powered.
    pub powered: bool,
    /// A boomerang came back: the next throw is full charge plus the speed bonus.
    pub charged: bool,
    pub catch_window: f32,
    pub catch_cooldown: f32,
    pub throw_cooldown: f32,
    /// Enemy this player is locked on to; their throws home in on it.
    pub lock: Option<PlayerId>,
    pub slime: Option<Slime>,
    /// Launched by a tackle or ricochet: the host flies them with this velocity until they land.
    pub knock: Option<Vec3>,
    /// The client is mid-dash (invincible, tackles on contact). `dash_seq` numbers dashes.
    pub dashing: bool,
    pub dash_seq: u32,
    /// Who this dash already tackled.
    pub tackled: Vec<PlayerId>,
    /// > 0 while knocked flat by a brick: no moving, throwing or catching.
    pub stun: f32,
    pub trick: Option<TrickState>,
    pub parkour: ParkourPose,
    /// In cover (reported by the client): crouched is a smaller target.
    pub cover: CoverPose,
    /// Push-back from a catch (bots only; clients move their own body).
    pub recoil: Vec3,
    /// Riding a mount (reported by the client).
    pub ride: Option<RideView>,
    /// Bumped on every hit taken, so clients can flash the model.
    pub hits: u32,
    /// > 0 while knocked out.
    pub respawn_timer: f32,
    /// Bumped every time the host teleports this player (respawn), so clients snap to it.
    pub teleports: u32,
    pub kos: u32,
    pub deaths: u32,
    pub brain: bot::Brain,
}

impl Player {
    pub fn alive(&self) -> bool {
        self.respawn_timer <= 0.0
    }
    pub fn forward(&self) -> Vec3 {
        Vec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos())
    }
    pub fn hold_point(&self) -> Vec3 {
        let h = if self.parkour == ParkourPose::Hang {
            HANG_HOLD_HEIGHT
        } else if self.cover == CoverPose::Crouched {
            CROUCH_HOLD_HEIGHT
        } else if self.cover == CoverPose::Up {
            COVER_HOLD_HEIGHT
        } else {
            HOLD_HEIGHT
        };
        self.pos + Vec3::Y * h + self.forward() * 0.45
    }
    /// How tall the body is for hit tests (shorter while crouched in cover).
    pub fn body_height(&self) -> f32 {
        if self.cover == CoverPose::Crouched {
            CROUCH_HEIGHT
        } else {
            BODY_HEIGHT
        }
    }
    pub fn catching(&self) -> bool {
        self.catch_window > 0.0
    }
    /// Stunned, slimed or flying from a knock: no moving, throwing, catching or turning.
    pub fn helpless(&self) -> bool {
        self.stun > 0.0 || self.slime.is_some() || self.knock.is_some()
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum BallState {
    /// Lying around (or still rolling); anyone can pick it up.
    Free,
    Held { by: PlayerId },
    /// Thrown and dangerous to `team` enemies until it touches something.
    /// `homing` is the locked-on target the ball steers toward; `track` is who a curved ball
    /// goes around cover to reach.
    Live {
        thrower: PlayerId,
        team: Team,
        curve: f32,
        lob: bool,
        powered: bool,
        homing: Option<PlayerId>,
        track: Option<PlayerId>,
    },
    /// A boomerang flying back to its thrower. Harmless; arrives charged.
    Returning { owner: PlayerId },
    /// Bounced off a wall at double speed: knocks back whoever it hits, no damage.
    Ricochet { thrower: PlayerId },
}

#[derive(Clone, Copy, Debug)]
pub struct Ball {
    pub pos: Vec3,
    pub vel: Vec3,
    pub state: BallState,
    pub pad: Vec3,
    pub kind: BallKind,
    /// Perfect catches in a row (speed stacks), reset when the rally ends.
    pub stacks: u8,
    /// A curved ball has started going around cover (it keeps tracking after).
    pub wrapping: bool,
}

impl Ball {
    pub fn new(pad: Vec3, kind: BallKind) -> Self {
        Ball { pos: pad, vel: Vec3::ZERO, state: BallState::Free, pad, kind, stacks: 0, wrapping: false }
    }
    /// The ball stops being part of a rally.
    fn go_free(&mut self) {
        self.state = BallState::Free;
        self.stacks = 0;
        self.wrapping = false;
    }
}

/// Things that happened during a step, for HUD messages and sounds.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum GameEvent {
    Hit { thrower: PlayerId, target: PlayerId },
    Knockout { thrower: PlayerId, target: PlayerId },
    /// `push` is the push-back the catcher feels (along the ball's flight).
    Caught { catcher: PlayerId, thrower: PlayerId, perfect: bool, push: [f32; 3] },
    Tackled { by: PlayerId, target: PlayerId },
    KnockedBack { target: PlayerId },
    Slimed { thrower: PlayerId, target: PlayerId },
    Stunned { thrower: PlayerId, target: PlayerId },
    BoomerangBack { owner: PlayerId },
    /// A big trick combo; `charged` when it charged the ball in hand.
    Combo { player: PlayerId, points: u32, charged: bool },
    MatchStarted,
    MatchEnded { winner: Team },
}

pub struct Match {
    /// Cover blocks only (curved balls go around these).
    pub cover: Vec<Aabb>,
    pub phase: Phase,
    pub mode: GameMode,
    pub map: MapId,
    pub arena: ArenaId,
    pub players: Vec<Player>,
    pub balls: Vec<Ball>,
    pub score: [u32; 2],
    pub solids: Vec<Aabb>,
    pub events: Vec<GameEvent>,
    pub rng: Rng,
    next_id: PlayerId,
}

impl Default for Match {
    fn default() -> Self {
        Self::new()
    }
}

impl Match {
    pub fn new() -> Self {
        let mut m = Match {
            phase: Phase::Lobby,
            mode: GameMode::Standard,
            map: MapId::Day,
            arena: ArenaId::Classic,
            players: Vec::new(),
            balls: Vec::new(),
            score: [0, 0],
            solids: layout::solids(ArenaId::Classic),
            cover: layout::cover(ArenaId::Classic),
            events: Vec::new(),
            rng: Rng(0x9E37_79B9_7F4A_7C15),
            next_id: 0,
        };
        m.reset_balls();
        m
    }

    pub fn reset_balls(&mut self) {
        self.balls = layout::ball_pads(self.arena)
            .into_iter()
            .map(|(pad, kind)| Ball::new(pad, kind))
            .collect();
        for p in &mut self.players {
            p.holding = None;
            p.powered = false;
        }
    }

    pub fn is_full(&self) -> bool {
        self.players.len() >= MAX_PLAYERS
    }

    /// Add a player to the smaller team. Returns `None` when the match is full.
    pub fn add_player(&mut self, name: &str, bot: bool) -> Option<PlayerId> {
        if self.is_full() {
            return None;
        }
        let reds = self.team_count(Team::Red);
        let team = if reds <= self.players.len() - reds { Team::Red } else { Team::Blue };
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let slot = self.team_count(team);
        self.players.push(Player {
            id,
            name: name.to_string(),
            team,
            bot,
            pos: layout::spawn_point(self.arena, team == Team::Red, slot),
            yaw: if team == Team::Red { 0.0 } else { std::f32::consts::PI },
            hp: MAX_HP,
            holding: None,
            powered: false,
            charged: false,
            catch_window: 0.0,
            catch_cooldown: 0.0,
            throw_cooldown: 0.0,
            lock: None,
            slime: None,
            knock: None,
            dashing: false,
            dash_seq: 0,
            tackled: Vec::new(),
            stun: 0.0,
            trick: None,
            parkour: ParkourPose::None,
            cover: CoverPose::None,
            recoil: Vec3::ZERO,
            ride: None,
            hits: 0,
            respawn_timer: 0.0,
            teleports: 0,
            kos: 0,
            deaths: 0,
            brain: bot::Brain::default(),
        });
        Some(id)
    }

    pub fn remove_player(&mut self, id: PlayerId) {
        for b in &mut self.balls {
            if b.state == (BallState::Held { by: id }) || b.state == (BallState::Returning { owner: id }) {
                b.state = BallState::Free;
            }
        }
        self.players.retain(|p| p.id != id);
    }

    /// Remove the most recently added bot. Returns whether one was removed.
    pub fn remove_bot(&mut self) -> bool {
        match self.players.iter().rev().find(|p| p.bot).map(|p| p.id) {
            Some(id) => {
                self.remove_player(id);
                true
            }
            None => false,
        }
    }

    pub fn team_count(&self, team: Team) -> usize {
        self.players.iter().filter(|p| p.team == team).count()
    }

    pub fn player(&self, id: PlayerId) -> Option<&Player> {
        self.players.iter().find(|p| p.id == id)
    }
    fn player_mut(&mut self, id: PlayerId) -> Option<&mut Player> {
        self.players.iter_mut().find(|p| p.id == id)
    }

    /// A human client reports where its body is (movement is client-side).
    /// While slimed or stunned the host owns the position; only the facing is taken.
    pub fn set_pose(&mut self, id: PlayerId, pos: Vec3, yaw: f32) {
        let lim = self.arena.half() + 2.0;
        if let Some(p) = self.player_mut(id) {
            if p.alive() && pos.is_finite() && yaw.is_finite() && !p.helpless() {
                p.yaw = yaw;
                p.pos = pos.clamp(Vec3::new(-lim, -50.0, -lim), Vec3::new(lim, 50.0, lim));
            }
        }
    }

    /// A client reports the air trick it is doing (for everyone else to see).
    pub fn set_trick(&mut self, id: PlayerId, trick: Option<TrickState>) {
        if let Some(p) = self.player_mut(id) {
            p.trick = trick.filter(|t| t.progress.is_finite()).map(|t| TrickState {
                kind: t.kind,
                progress: t.progress.clamp(0.0, 1.0),
            });
        }
    }

    /// A client reports what it is doing on a wall.
    pub fn set_parkour(&mut self, id: PlayerId, pose: ParkourPose) {
        if let Some(p) = self.player_mut(id) {
            p.parkour = pose;
        }
    }

    /// A client reports what it is riding (if anything).
    pub fn set_ride(&mut self, id: PlayerId, ride: Option<RideView>) {
        if let Some(p) = self.player_mut(id) {
            p.ride = if p.alive() { ride.and_then(RideView::sanitized) } else { None };
        }
    }

    /// A client reports whether it is in cover (crouched or risen up).
    pub fn set_cover(&mut self, id: PlayerId, cover: CoverPose) {
        if let Some(p) = self.player_mut(id) {
            p.cover = if p.alive() && !p.helpless() { cover } else { CoverPose::None };
        }
    }

    /// A client reports whether it is mid-dash (`Some(dash number)`) or not.
    pub fn set_dash(&mut self, id: PlayerId, dash: Option<u32>) {
        if let Some(p) = self.player_mut(id) {
            let dashing = dash.is_some() && p.alive() && !p.helpless();
            if let Some(seq) = dash {
                if seq != p.dash_seq {
                    p.dash_seq = seq;
                    p.tackled.clear();
                }
            }
            p.dashing = dashing;
        }
    }

    /// Mode and map can only change between matches.
    pub fn set_mode(&mut self, mode: GameMode) -> bool {
        if self.phase == Phase::Playing {
            return false;
        }
        self.mode = mode;
        true
    }
    pub fn set_map(&mut self, map: MapId) -> bool {
        if self.phase == Phase::Playing {
            return false;
        }
        self.map = map;
        true
    }

    /// Switch arena geometry (between matches): new walls and pads, everyone back to spawn.
    pub fn set_arena(&mut self, arena: ArenaId) -> bool {
        if self.phase == Phase::Playing {
            return false;
        }
        if self.arena != arena {
            self.arena = arena;
            self.solids = layout::solids(arena);
            self.cover = layout::cover(arena);
            self.reset_balls();
            let ids: Vec<PlayerId> = self.players.iter().map(|p| p.id).collect();
            for id in ids {
                self.respawn(id);
            }
        }
        true
    }

    pub fn half(&self) -> f32 {
        self.arena.half()
    }

    /// Can a match start? Needs two players; teams are rebalanced on start.
    pub fn can_start(&self) -> bool {
        self.players.len() >= 2
    }

    /// Start (or restart) a match from the lobby or end screen.
    pub fn start(&mut self) -> bool {
        if !self.can_start() || self.phase == Phase::Playing {
            return false;
        }
        self.rebalance_teams();
        self.score = [0, 0];
        self.reset_balls();
        let ids: Vec<PlayerId> = self.players.iter().map(|p| p.id).collect();
        for id in ids {
            if let Some(p) = self.player_mut(id) {
                p.kos = 0;
                p.deaths = 0;
            }
            self.respawn(id);
        }
        self.phase = Phase::Playing;
        self.events.push(GameEvent::MatchStarted);
        true
    }

    /// Back to the lobby after a match. Everyone still out comes back.
    pub fn to_lobby(&mut self) {
        self.phase = Phase::Lobby;
        self.score = [0, 0];
        let out: Vec<PlayerId> = self.players.iter().filter(|p| !p.alive()).map(|p| p.id).collect();
        for id in out {
            self.respawn(id);
        }
    }

    /// Deathmatch: is anyone on `team` still standing?
    fn team_alive(&self, team: Team) -> bool {
        self.players.iter().any(|p| p.team == team && p.alive())
    }

    /// Deathmatch ends when a team has nobody standing.
    fn check_elimination(&mut self) {
        if self.mode != GameMode::Deathmatch || self.phase != Phase::Playing {
            return;
        }
        let (red, blue) = (self.team_alive(Team::Red), self.team_alive(Team::Blue));
        let winner = match (red, blue) {
            (true, false) => Team::Red,
            (false, true) => Team::Blue,
            _ => return,
        };
        self.phase = Phase::Ended { winner };
        self.events.push(GameEvent::MatchEnded { winner });
    }

    /// Respawns are off during a deathmatch.
    fn respawns_enabled(&self) -> bool {
        !(self.mode == GameMode::Deathmatch && self.phase == Phase::Playing)
    }

    /// Alternate players across teams so sizes differ by at most one.
    fn rebalance_teams(&mut self) {
        let reds = self.team_count(Team::Red) as isize;
        let blues = self.players.len() as isize - reds;
        if (reds - blues).abs() <= 1 {
            return;
        }
        let (from, to) = if reds > blues { (Team::Red, Team::Blue) } else { (Team::Blue, Team::Red) };
        let mut to_move = ((reds - blues).abs() / 2) as usize;
        // Prefer moving bots so humans keep their team.
        for bots_first in [true, false] {
            for p in self.players.iter_mut().rev() {
                if to_move > 0 && p.team == from && p.bot == bots_first {
                    p.team = to;
                    to_move -= 1;
                }
            }
        }
    }

    fn respawn(&mut self, id: PlayerId) {
        let Some(idx) = self.players.iter().position(|p| p.id == id) else { return };
        let team = self.players[idx].team;
        let slot = self.players[..idx].iter().filter(|p| p.team == team).count();
        let p = &mut self.players[idx];
        p.pos = layout::spawn_point(self.arena, team == Team::Red, slot);
        p.yaw = if team == Team::Red { 0.0 } else { std::f32::consts::PI };
        p.hp = MAX_HP;
        p.respawn_timer = 0.0;
        p.catch_window = 0.0;
        p.catch_cooldown = 0.0;
        p.slime = None;
        p.knock = None;
        p.dashing = false;
        p.stun = 0.0;
        p.trick = None;
        p.lock = None;
        p.cover = CoverPose::None;
        p.recoil = Vec3::ZERO;
        p.teleports = p.teleports.wrapping_add(1);
    }

    /// Apply a player action. Invalid actions (dead, no ball, on cooldown) are ignored.
    pub fn apply(&mut self, id: PlayerId, action: Action) {
        let Some(pi) = self.players.iter().position(|p| p.id == id) else { return };
        let p = &self.players[pi];
        if !p.alive() || matches!(self.phase, Phase::Ended { .. }) {
            return;
        }
        // Stunned, slimed or launched: can't do anything but wait (letting go of a lock is fine).
        if p.helpless() && !matches!(action, Action::Lock { target: None }) {
            return;
        }
        match action {
            Action::Catch => {
                let p = &mut self.players[pi];
                if p.holding.is_none() && p.catch_cooldown <= 0.0 {
                    p.catch_window = CATCH_WINDOW;
                    p.catch_cooldown = CATCH_COOLDOWN;
                }
            }
            Action::Throw { aim, charge, curve, lob } => {
                let p = &mut self.players[pi];
                let Some(bi) = p.holding else { return };
                if p.throw_cooldown > 0.0 {
                    return;
                }
                let aim = Vec3::from_array(aim);
                if !aim.is_finite() || aim.length_squared() < 1e-6 {
                    return;
                }
                let (powered, charged) = (p.powered, p.charged);
                let charge = if charged { 1.0 } else { charge };
                let params = ThrowParams { charge, curve, lob, powered: powered || charged };
                let (mut vel, curve) = ball::launch(aim, params);
                p.holding = None;
                p.powered = false;
                p.charged = false;
                p.throw_cooldown = THROW_COOLDOWN;
                let (team, origin, lock) = (p.team, p.hold_point(), p.lock);
                let homing = lock.filter(|t| self.valid_lock(team, *t));
                // Who the throw is "at": the lock, else the enemy closest to the aim.
                let aimed_at = homing.or_else(|| self.enemy_near_aim(team, origin, aim));
                if lob {
                    // Lobs drop onto the very top of the target's head.
                    if let Some(t) = aimed_at.and_then(|t| self.player(t)) {
                        vel = ball::lob_to(origin, t.pos + Vec3::Y * (t.body_height() + 0.1));
                    }
                } else {
                    vel = (vel * ball::stack_multiplier(self.balls[bi].stacks)).clamp_length_max(ball::MAX_BALL_SPEED);
                }
                // Curves and lobs track their target (a dash shakes them off).
                let track = if curve != 0.0 || lob { aimed_at } else { None };
                let b = &mut self.balls[bi];
                b.pos = origin;
                b.vel = vel;
                b.wrapping = false;
                b.state = BallState::Live { thrower: id, team, curve, lob, powered, homing, track };
            }
            Action::Lock { target } => {
                let team = self.players[pi].team;
                self.players[pi].lock = target.filter(|t| self.valid_lock(team, *t));
            }
            Action::Style { points } => {
                let p = &mut self.players[pi];
                let charged = points >= STYLE_CHARGE_POINTS && p.holding.is_some() && !p.charged;
                if charged {
                    p.charged = true;
                }
                if points >= STYLE_FEED_POINTS {
                    self.events.push(GameEvent::Combo { player: id, points, charged });
                }
            }
        }
    }

    /// The living enemy of `team` closest to the aim direction, within a cone and range.
    fn enemy_near_aim(&self, team: Team, from: Vec3, aim: Vec3) -> Option<PlayerId> {
        let aim = Vec3::new(aim.x, 0.0, aim.z).normalize_or_zero();
        self.players
            .iter()
            .filter(|p| p.team != team && p.alive())
            .filter_map(|p| {
                let to = Vec3::new(p.pos.x - from.x, 0.0, p.pos.z - from.z);
                let angle = aim.angle_between(to.normalize_or_zero());
                (to.length() <= LOCK_RANGE && angle <= TRACK_CONE).then_some((angle, p.id))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, id)| id)
    }

    /// Can someone on `team` lock on to `target`? It must be a living enemy.
    fn valid_lock(&self, team: Team, target: PlayerId) -> bool {
        self.player(target).is_some_and(|t| t.team != team && t.alive())
    }

    pub fn step(&mut self, dt: f32) {
        self.tick_players(dt);
        self.tackles();
        self.run_bots(dt);
        self.step_balls(dt);
        self.pickups();
        self.check_elimination();
    }

    fn tick_players(&mut self, dt: f32) {
        // Drop locks on targets that died, left or switched team.
        let locks: Vec<Option<PlayerId>> =
            self.players.iter().map(|p| p.lock.filter(|t| self.valid_lock(p.team, *t))).collect();
        for (p, lock) in self.players.iter_mut().zip(locks) {
            p.lock = lock;
        }
        let respawns = self.respawns_enabled();
        let mut to_respawn = Vec::new();
        let solids = &self.solids;
        let half = self.arena.half();
        for p in &mut self.players {
            p.stun = (p.stun - dt).max(0.0);
            if p.recoil != Vec3::ZERO {
                if p.bot && !p.helpless() {
                    p.pos = bot::collide_feet(p.pos + p.recoil * dt, solids, half);
                }
                p.recoil *= (-RECOIL_DECAY * dt).exp();
                if p.recoil.length() < 0.2 {
                    p.recoil = Vec3::ZERO;
                }
            }
            if let Some(v) = p.knock.as_mut() {
                v.y -= KNOCK_GRAVITY * dt;
                p.pos += *v * dt;
                p.pos = bot::collide_feet(Vec3::new(p.pos.x, p.pos.y, p.pos.z), solids, half).with_y(p.pos.y);
                if p.pos.y <= 0.0 && v.y < 0.0 {
                    p.pos.y = 0.0;
                    p.knock = None;
                }
            }
            if let Some(sl) = p.slime.as_mut() {
                let to = Vec3::new(sl.anchor.x - p.pos.x, 0.0, sl.anchor.z - p.pos.z);
                let step = SLIME_KNOCK_SPEED * dt;
                if to.length() <= step {
                    p.pos.x = sl.anchor.x;
                    p.pos.z = sl.anchor.z;
                } else {
                    p.pos += to.normalize() * step;
                }
                sl.timer -= dt;
                if sl.timer <= 0.0 {
                    p.slime = None;
                }
            }
            p.catch_window = (p.catch_window - dt).max(0.0);
            p.catch_cooldown = (p.catch_cooldown - dt).max(0.0);
            p.throw_cooldown = (p.throw_cooldown - dt).max(0.0);
            if p.respawn_timer > 0.0 {
                if respawns {
                    p.respawn_timer -= dt;
                    if p.respawn_timer <= 0.0 {
                        to_respawn.push(p.id);
                    }
                }
            } else if p.pos.y < -10.0 {
                // Fell out of the world: counts as a knockout with no thrower credit.
                p.respawn_timer = RESPAWN_SECS;
                p.deaths += 1;
            }
        }
        for id in to_respawn {
            self.respawn(id);
        }
    }

    /// Dashing into an enemy launches them (and any ball they hold) into the air.
    fn tackles(&mut self) {
        let dashers: Vec<(usize, Vec3, Vec3, Team)> = self
            .players
            .iter()
            .enumerate()
            .filter(|(_, p)| p.dashing && p.alive())
            .map(|(i, p)| (i, p.pos, p.forward(), p.team))
            .collect();
        for (di, dpos, dfwd, dteam) in dashers {
            let victims: Vec<usize> = self
                .players
                .iter()
                .enumerate()
                .filter(|(_, e)| e.team != dteam && e.alive() && e.knock.is_none() && !e.dashing)
                .filter(|(_, e)| !self.players[di].tackled.contains(&e.id))
                .filter(|(_, e)| {
                    let flat = Vec3::new(e.pos.x - dpos.x, 0.0, e.pos.z - dpos.z).length();
                    flat < TACKLE_RANGE && (e.pos.y - dpos.y).abs() < 1.6
                })
                .map(|(i, _)| i)
                .collect();
            for vi in victims {
                let (by, target) = (self.players[di].id, self.players[vi].id);
                self.players[di].tackled.push(target);
                let push = Vec3::new(self.players[vi].pos.x - dpos.x, 0.0, self.players[vi].pos.z - dpos.z)
                    .normalize_or(dfwd);
                self.launch(vi, push * TACKLE_PUSH + Vec3::Y * TACKLE_LIFT);
                self.events.push(GameEvent::Tackled { by, target });
            }
        }
    }

    /// Throw a player through the air (host-owned until they land); their ball pops up too.
    fn launch(&mut self, pi: usize, vel: Vec3) {
        let p = &mut self.players[pi];
        p.knock = Some(vel);
        p.catch_window = 0.0;
        p.trick = None;
        p.dashing = false;
        if let Some(held) = p.holding.take() {
            p.powered = false;
            p.charged = false;
            let b = &mut self.balls[held];
            b.go_free();
            b.vel = Vec3::Y * 10.0 + Vec3::new(vel.x, 0.0, vel.z) * 0.2;
        }
    }

    fn run_bots(&mut self, dt: f32) {
        let ids: Vec<PlayerId> =
            self.players.iter().filter(|p| p.bot && p.alive() && !p.helpless()).map(|p| p.id).collect();
        for id in ids {
            let actions = bot::think(self, id, dt);
            for a in actions {
                self.apply(id, a);
            }
        }
    }

    fn step_balls(&mut self, dt: f32) {
        for bi in 0..self.balls.len() {
            let ball = self.balls[bi];
            match ball.state {
                BallState::Held { by } => {
                    match self.player(by).filter(|p| p.alive()) {
                        Some(p) => {
                            let at = p.hold_point();
                            let b = &mut self.balls[bi];
                            b.pos = at;
                            b.vel = Vec3::ZERO;
                        }
                        None => self.balls[bi].go_free(),
                    }
                }
                BallState::Free => {
                    let b = &mut self.balls[bi];
                    if b.vel != Vec3::ZERO || b.pos.y > BALL_RADIUS + 0.01 {
                        let g = ball::gravity(false, false);
                        ball::step(&mut b.pos, &mut b.vel, 0.0, g, dt, &self.solids);
                    }
                }
                BallState::Live { thrower, team, curve, lob, powered, mut homing, mut track } => {
                    let g = ball::gravity(lob, true);
                    // Dashing out of the way shakes off a tracking ball for good.
                    let dashing = |id: Option<PlayerId>| id.and_then(|t| self.player(t)).is_some_and(|t| t.dashing);
                    if dashing(track) {
                        track = None;
                    }
                    if dashing(homing) {
                        homing = None;
                    }
                    let alive_pos =
                        |id: Option<PlayerId>| id.and_then(|t| self.player(t)).filter(|t| t.alive()).map(|t| t.pos);
                    let (aim_at, track_pos) = (alive_pos(homing).map(|p| p + Vec3::Y * 1.0), alive_pos(track));
                    let head = track.and_then(|t| self.player(t)).map_or(BODY_HEIGHT, |t| t.body_height());
                    let b = &mut self.balls[bi];
                    b.state = BallState::Live { thrower, team, curve, lob, powered, homing, track };
                    let mut bend = curve;
                    match (lob, track_pos) {
                        // A lob re-solves its arc every step to come down on top of the head.
                        (true, Some(feet)) => {
                            ball::guide_lob(&mut b.vel, b.pos, feet + Vec3::Y * (head + 0.1), g);
                        }
                        // A curve bends out, then steers itself in (around cover if needed).
                        (false, Some(feet)) => {
                            let target = feet + Vec3::Y * 1.0;
                            let side = -curve.signum();
                            if let Some(wp) = ball::wrap_waypoint(b.pos, target, side, &self.cover) {
                                b.wrapping = true;
                                ball::steer_flat(&mut b.vel, wp - b.pos, ball::WRAP_TURN_RATE, dt);
                                bend = 0.0;
                            } else {
                                let to = target - b.pos;
                                let eta = to.length() / b.vel.length().max(1.0);
                                if b.wrapping || eta < ball::CURVE_HOME_ETA {
                                    ball::guide_to(&mut b.vel, to, eta, dt);
                                    bend = 0.0;
                                }
                            }
                        }
                        _ => {
                            if let Some(target) = aim_at {
                                ball::home(&mut b.vel, target - b.pos, dt, lob);
                            }
                        }
                    }
                    let prev = b.pos;
                    let r = ball::step(&mut b.pos, &mut b.vel, bend, g, dt, &self.solids);
                    let now = b.pos;
                    if let Some(target) = self.first_enemy_hit(team, prev, now) {
                        self.resolve_hit(bi, thrower, target, powered);
                    } else if r.touched {
                        let b = &mut self.balls[bi];
                        match (b.kind, r.wall) {
                            (BallKind::Boomerang, _) => {
                                b.state = BallState::Returning { owner: thrower };
                                b.stacks = 0;
                            }
                            // Off a wall: double speed, harmless knockback ball.
                            (_, Some(n)) => {
                                b.vel = ball::ricochet(r.vel_before_wall, n);
                                b.state = BallState::Ricochet { thrower };
                                b.stacks = 0;
                            }
                            _ => b.go_free(),
                        }
                    }
                }
                BallState::Ricochet { thrower } => {
                    let b = &mut self.balls[bi];
                    let prev = b.pos;
                    let r = ball::step(&mut b.pos, &mut b.vel, 0.0, ball::THROW_GRAVITY, dt, &self.solids);
                    let now = b.pos;
                    if let Some(pi) = self.first_player_hit(prev, now) {
                        self.ricochet_hit(bi, pi, thrower);
                    } else if r.touched {
                        self.balls[bi].go_free();
                    }
                }
                BallState::Returning { owner } => {
                    let home =
                        self.player(owner).filter(|p| p.alive()).map(|p| (p.hold_point(), p.holding.is_some()));
                    let Some((at, hands_full)) = home else {
                        self.balls[bi].go_free();
                        continue;
                    };
                    let b = &mut self.balls[bi];
                    if ball::fly_back(&mut b.pos, &mut b.vel, at, dt) {
                        if hands_full {
                            // Owner grabbed another ball meanwhile: drop it at their feet.
                            b.go_free();
                        } else {
                            b.state = BallState::Held { by: owner };
                            if let Some(p) = self.player_mut(owner) {
                                p.holding = Some(bi);
                                p.charged = true;
                            }
                            self.events.push(GameEvent::BoomerangBack { owner });
                        }
                    }
                }
            }
            // Escaped the arena somehow: put it back on its pad.
            let b = &mut self.balls[bi];
            let lim = self.arena.half() + 3.0;
            if b.pos.x.abs() > lim || b.pos.z.abs() > lim || b.pos.y < -5.0 || !b.pos.is_finite() {
                if let BallState::Held { by } = b.state {
                    if let Some(p) = self.players.iter_mut().find(|p| p.id == by) {
                        p.holding = None;
                    }
                }
                let b = &mut self.balls[bi];
                b.pos = b.pad;
                b.vel = Vec3::ZERO;
                b.go_free();
            }
        }
    }

    /// Any living, non-dashing player the path a->b crosses first (ricochets hit anyone).
    fn first_player_hit(&self, a: Vec3, b: Vec3) -> Option<usize> {
        self.players
            .iter()
            .enumerate()
            .filter(|(_, p)| p.alive() && !p.dashing)
            .filter_map(|(i, p)| segment_hits_body(a, b, p.pos, p.body_height()).map(|t| (t, i)))
            .min_by(|x, y| x.0.total_cmp(&y.0))
            .map(|(_, i)| i)
    }

    /// A ricochet hits someone: caught if they are catching and facing it, otherwise
    /// they are knocked back with no damage and no points.
    fn ricochet_hit(&mut self, bi: usize, pi: usize, thrower: PlayerId) {
        let vel = self.balls[bi].vel;
        let p = &self.players[pi];
        let facing = Vec3::new(-vel.x, 0.0, -vel.z).normalize_or_zero().dot(p.forward());
        if p.catching() && p.holding.is_none() && facing > CATCH_FACING {
            self.catch_ball(bi, pi, thrower);
            return;
        }
        let push = Vec3::new(vel.x, 0.0, vel.z).normalize_or_zero();
        let target = self.players[pi].id;
        self.players[pi].hits = self.players[pi].hits.wrapping_add(1);
        if !self.players[pi].catching() {
            self.launch(pi, push * RICOCHET_PUSH + Vec3::Y * RICOCHET_LIFT);
        }
        let b = &mut self.balls[bi];
        b.go_free();
        b.vel = Vec3::new(-vel.x * 0.1, 3.0, -vel.z * 0.1);
        self.events.push(GameEvent::KnockedBack { target });
    }

    /// `pi` catches ball `bi`. Pressed just before it arrived: a perfect catch.
    fn catch_ball(&mut self, bi: usize, pi: usize, thrower: PlayerId) {
        let v = self.balls[bi].vel;
        let push = (Vec3::new(v.x, 0.0, v.z) * CATCH_PUSH_SCALE).clamp_length_max(CATCH_PUSH_MAX);
        let t = &mut self.players[pi];
        t.recoil = push;
        let perfect = CATCH_WINDOW - t.catch_window <= PERFECT_SECS;
        t.holding = Some(bi);
        t.powered = true;
        t.charged = perfect;
        t.catch_window = 0.0;
        t.catch_cooldown = 0.0;
        let catcher = t.id;
        let b = &mut self.balls[bi];
        b.state = BallState::Held { by: catcher };
        b.wrapping = false;
        b.stacks = if perfect { (b.stacks + 1).min(ball::MAX_STACKS) } else { 0 };
        self.events.push(GameEvent::Caught { catcher, thrower, perfect, push: push.to_array() });
    }

    /// The enemy (of `team`) whose body the ball's path from `a` to `b` crosses first.
    fn first_enemy_hit(&self, team: Team, a: Vec3, b: Vec3) -> Option<PlayerId> {
        self.players
            .iter()
            // Dashing players are invincible: balls pass through them.
            .filter(|p| p.team != team && p.alive() && !p.dashing)
            .filter_map(|p| segment_hits_body(a, b, p.pos, p.body_height()).map(|t| (t, p.id)))
            .min_by(|x, y| x.0.total_cmp(&y.0))
            .map(|(_, id)| id)
    }

    fn resolve_hit(&mut self, bi: usize, thrower: PlayerId, target: PlayerId, powered: bool) {
        let ball_vel = self.balls[bi].vel;
        let Some(ti) = self.players.iter().position(|p| p.id == target) else { return };
        let t = &self.players[ti];
        let incoming = (-ball_vel).normalize_or_zero();
        let facing = Vec3::new(incoming.x, 0.0, incoming.z).normalize_or_zero().dot(t.forward());
        if t.catching() && t.holding.is_none() && facing > CATCH_FACING {
            self.catch_ball(bi, ti, thrower);
            return;
        }

        // Hit: a boomerang heads home, anything else drops dead and bounces off.
        let b = &mut self.balls[bi];
        let kind = b.kind;
        b.stacks = 0;
        b.wrapping = false;
        b.state = match kind {
            BallKind::Boomerang => BallState::Returning { owner: thrower },
            _ => BallState::Free,
        };
        b.vel = Vec3::new(-ball_vel.x * 0.15, 3.0, -ball_vel.z * 0.15);
        // Mid-catch, nothing hurts: the ball just bounces off.
        if t.catching() {
            return;
        }

        let dmg = kind.damage(powered);
        let t = &mut self.players[ti];
        t.hp = t.hp.saturating_sub(dmg);
        t.hits = t.hits.wrapping_add(1);
        if t.hp > 0 {
            self.events.push(GameEvent::Hit { thrower, target });
            if kind == BallKind::Brick {
                let t = &mut self.players[ti];
                t.stun = STUN_SECS;
                t.catch_window = 0.0;
                t.trick = None;
                t.pos.y = 0.0; // knocked to the floor
                // Knocked flat: drop whatever was in hand.
                if let Some(held) = t.holding.take() {
                    t.powered = false;
                    t.charged = false;
                    self.balls[held].go_free();
                    self.balls[held].vel = Vec3::Y * 2.0;
                }
                self.events.push(GameEvent::Stunned { thrower, target });
            }
            if kind == BallKind::Slime {
                let anchor = layout::nearest_wall_anchor(self.players[ti].pos, &self.solids, BODY_RADIUS);
                self.players[ti].cover = CoverPose::None;
                self.players[ti].slime = Some(Slime { anchor, timer: SLIME_SECS });
                self.events.push(GameEvent::Slimed { thrower, target });
            }
            return;
        }
        t.respawn_timer = RESPAWN_SECS;
        t.catch_window = 0.0;
        t.slime = None;
        t.knock = None;
        t.stun = 0.0;
        t.trick = None;
        let counts = self.phase == Phase::Playing;
        if counts {
            t.deaths += 1;
        }
        let victim_team = t.team;
        if let Some(held) = t.holding.take() {
            self.balls[held].go_free();
            self.balls[held].vel = Vec3::Y * 2.0;
        }
        t.powered = false;
        t.charged = false;
        self.events.push(GameEvent::Knockout { thrower, target });
        if counts {
            if let Some(k) = self.player_mut(thrower) {
                k.kos += 1;
            }
            let scorer = victim_team.other();
            self.score[scorer.index()] += 1;
            if self.score[scorer.index()] >= KO_TARGET {
                self.phase = Phase::Ended { winner: scorer };
                self.events.push(GameEvent::MatchEnded { winner: scorer });
            }
        }
    }

    fn pickups(&mut self) {
        for pi in 0..self.players.len() {
            let p = &self.players[pi];
            if !p.alive() || p.holding.is_some() {
                continue;
            }
            let reach = p.pos + Vec3::Y * 0.9;
            let found = self.balls.iter().position(|b| {
                b.state == BallState::Free && b.pos.distance(reach) < PICKUP_RADIUS + 0.6
                    && Vec3::new(b.pos.x - p.pos.x, 0.0, b.pos.z - p.pos.z).length() < PICKUP_RADIUS
            });
            if let Some(bi) = found {
                let id = p.id;
                self.players[pi].holding = Some(bi);
                self.players[pi].powered = false;
                self.players[pi].charged = false;
                self.balls[bi].state = BallState::Held { by: id };
                self.balls[bi].stacks = 0;
            }
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            phase: self.phase,
            mode: self.mode,
            map: self.map,
            arena: self.arena,
            score: self.score,
            players: self
                .players
                .iter()
                .map(|p| PlayerView {
                    id: p.id,
                    name: p.name.clone(),
                    team: p.team,
                    bot: p.bot,
                    pos: p.pos.to_array(),
                    yaw: p.yaw,
                    hp: p.hp,
                    holding: p.holding.is_some(),
                    powered: p.powered,
                    charged: p.charged,
                    catching: p.catching(),
                    lock: p.lock,
                    slimed: p.slime.map_or(0.0, |s| s.timer),
                    stunned: p.stun,
                    knocked: p.knock.is_some(),
                    dashing: p.dashing,
                    trick: p.trick,
                    parkour: if p.alive() && !p.helpless() { p.parkour } else { ParkourPose::None },
                    cover: if p.alive() && !p.helpless() { p.cover } else { CoverPose::None },
                    ride: if p.alive() { p.ride } else { None },
                    hits: p.hits,
                    catch_cooldown: p.catch_cooldown,
                    respawn_timer: p.respawn_timer.max(0.0),
                    teleports: p.teleports,
                    kos: p.kos,
                    deaths: p.deaths,
                    last_action: 0,
                })
                .collect(),
            balls: self
                .balls
                .iter()
                .map(|b| BallView {
                    pos: b.pos.to_array(),
                    vel: b.vel.to_array(),
                    state: b.state,
                    kind: b.kind,
                    stacks: b.stacks,
                })
                .collect(),
            events: self.events.clone(),
        }
    }
}

/// Where along segment a->b (0..1) the ball first touches a body `height` tall at `feet`.
pub fn segment_hits_body(a: Vec3, b: Vec3, feet: Vec3, height: f32) -> Option<f32> {
    let r = BODY_RADIUS + BALL_RADIUS;
    let bottom = feet + Vec3::Y * BODY_RADIUS;
    let top = feet + Vec3::Y * (height - BODY_RADIUS).max(BODY_RADIUS);
    // Sample the segment; steps are short (sub-frame), so 8 samples is plenty.
    (0..=8).map(|i| i as f32 / 8.0).find(|&t| {
        let p = a.lerp(b, t);
        let axis = top - bottom;
        let s = ((p - bottom).dot(axis) / axis.length_squared()).clamp(0.0, 1.0);
        p.distance(bottom + axis * s) <= r
    })
}

/// What clients need to draw the match. Plain arrays so it serialises without glam features.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub phase: Phase,
    pub mode: GameMode,
    pub map: MapId,
    pub arena: ArenaId,
    pub score: [u32; 2],
    pub players: Vec<PlayerView>,
    pub balls: Vec<BallView>,
    pub events: Vec<GameEvent>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlayerView {
    pub id: PlayerId,
    pub name: String,
    pub team: Team,
    pub bot: bool,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub hp: u8,
    pub holding: bool,
    pub powered: bool,
    pub charged: bool,
    pub catching: bool,
    pub catch_cooldown: f32,
    pub lock: Option<PlayerId>,
    /// Seconds of slime left (0 = free).
    pub slimed: f32,
    /// Seconds left knocked flat by a brick (0 = up).
    pub stunned: f32,
    /// Flying from a tackle or ricochet (the host moves them).
    pub knocked: bool,
    /// Mid-dash: invincible, shown glowing blue.
    pub dashing: bool,
    pub trick: Option<TrickState>,
    pub parkour: ParkourPose,
    pub cover: CoverPose,
    pub ride: Option<RideView>,
    pub hits: u32,
    pub respawn_timer: f32,
    pub teleports: u32,
    pub kos: u32,
    pub deaths: u32,
    /// Highest action sequence number the host has applied for this player.
    pub last_action: u32,
}

impl PlayerView {
    pub fn alive(&self) -> bool {
        self.respawn_timer <= 0.0
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct BallView {
    pub pos: [f32; 3],
    pub vel: [f32; 3],
    pub state: BallState,
    pub kind: BallKind,
    pub stacks: u8,
}

/// Small deterministic RNG (xorshift64) so the sim has no extra dependencies.
#[derive(Clone, Debug)]
pub struct Rng(pub u64);

impl Rng {
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    /// Uniform in 0..1.
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;
    /// An open lane with no cover between z=0 and z=-10.
    const LANE: Vec3 = Vec3::new(14.0, 0.0, 0.0);

    /// Red player at z=0 facing -Z, blue player 10 m in front facing +Z (toward red).
    fn duel() -> (Match, PlayerId, PlayerId) {
        let mut m = Match::new();
        let red = m.add_player("red", false).unwrap();
        let blue = m.add_player("blue", false).unwrap();
        assert_eq!(m.player(red).unwrap().team, Team::Red);
        assert_eq!(m.player(blue).unwrap().team, Team::Blue);
        m.start();
        m.balls.clear();
        m.balls.push(Ball::new(LANE, BallKind::Normal));
        m.set_pose(red, LANE, 0.0);
        m.set_pose(blue, LANE + Vec3::new(0.0, 0.0, -10.0), std::f32::consts::PI);
        m.step(DT); // picks up the ball at its feet
        assert_eq!(m.player(red).unwrap().holding, Some(0));
        (m, red, blue)
    }

    fn throw_at(m: &mut Match, from: PlayerId, aim: Vec3) {
        m.apply(from, Action::Throw { aim: aim.to_array(), charge: 1.0, curve: 0.0, lob: false });
    }

    fn run(m: &mut Match, secs: f32) {
        for _ in 0..(secs / DT) as usize {
            m.step(DT);
        }
    }

    #[test]
    fn teams_alternate_and_cap_at_four() {
        let mut m = Match::new();
        let teams: Vec<Team> = (0..4).map(|i| {
            let id = m.add_player(&format!("p{i}"), false).unwrap();
            m.player(id).unwrap().team
        }).collect();
        assert_eq!(teams, [Team::Red, Team::Blue, Team::Red, Team::Blue]);
        assert!(m.add_player("extra", false).is_none());
    }

    #[test]
    fn needs_both_teams_to_start() {
        let mut m = Match::new();
        m.add_player("solo", false);
        assert!(!m.start());
        m.add_player("bot", true);
        assert!(m.start());
        assert_eq!(m.phase, Phase::Playing);
    }

    #[test]
    fn hit_costs_one_hp() {
        let (mut m, red, blue) = duel();
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        run(&mut m, 1.0);
        assert_eq!(m.player(blue).unwrap().hp, MAX_HP - 1);
        assert!(m.events.contains(&GameEvent::Hit { thrower: red, target: blue }));
        assert!(!matches!(m.balls[0].state, BallState::Live { .. }), "a ball that hit is dead");
    }

    #[test]
    fn two_hits_knock_out_and_score() {
        let (mut m, red, blue) = duel();
        m.players.iter_mut().find(|p| p.id == blue).unwrap().hp = 1;
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        run(&mut m, 0.6);
        let b = m.player(blue).unwrap();
        assert!(!b.alive());
        assert_eq!(b.deaths, 1);
        assert_eq!(m.player(red).unwrap().kos, 1);
        assert_eq!(m.score, [1, 0]);
    }

    #[test]
    fn knocked_out_player_respawns_with_full_hp() {
        let (mut m, red, blue) = duel();
        m.players.iter_mut().find(|p| p.id == blue).unwrap().hp = 1;
        let tp = m.player(blue).unwrap().teleports;
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        run(&mut m, 0.6 + RESPAWN_SECS);
        let b = m.player(blue).unwrap();
        assert!(b.alive());
        assert_eq!(b.hp, MAX_HP);
        assert_eq!(b.teleports, tp + 1);
    }

    #[test]
    fn catch_when_facing_and_timed() {
        let (mut m, red, blue) = duel();
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        m.apply(blue, Action::Catch);
        run(&mut m, 0.5);
        let b = m.player(blue).unwrap();
        assert_eq!(b.hp, MAX_HP);
        assert_eq!(b.holding, Some(0));
        assert!(b.powered, "caught ball is powered");
        assert!(m.events.iter().any(|e| matches!(e, GameEvent::Caught { catcher, thrower, .. } if *catcher == blue && *thrower == red)));
    }

    #[test]
    fn catch_window_expires() {
        let (mut m, red, blue) = duel();
        m.apply(blue, Action::Catch);
        run(&mut m, CATCH_WINDOW + 0.1);
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        run(&mut m, 0.5);
        assert_eq!(m.player(blue).unwrap().hp, MAX_HP - 1);
    }

    #[test]
    fn powered_throw_does_double_damage() {
        let (mut m, red, blue) = duel();
        m.players.iter_mut().find(|p| p.id == red).unwrap().powered = true;
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        run(&mut m, 0.5);
        assert_eq!(m.player(blue).unwrap().hp, MAX_HP - 2);
    }

    #[test]
    fn no_friendly_fire() {
        let mut m = Match::new();
        let a = m.add_player("a", false).unwrap();
        m.add_player("enemy", true).unwrap();
        let c = m.add_player("c", false).unwrap();
        m.start();
        m.players.retain(|p| !p.bot);
        m.balls = vec![Ball::new(LANE, BallKind::Normal)];
        m.set_pose(a, LANE, 0.0);
        m.set_pose(c, LANE + Vec3::new(0.0, 0.0, -6.0), std::f32::consts::PI);
        m.step(DT);
        throw_at(&mut m, a, Vec3::new(0.0, -0.05, -1.0));
        run(&mut m, 0.5);
        assert_eq!(m.player(c).unwrap().hp, MAX_HP);
    }

    #[test]
    fn cannot_throw_without_ball() {
        let (mut m, _red, blue) = duel();
        throw_at(&mut m, blue, Vec3::Z);
        assert!(m.balls.iter().all(|b| !matches!(b.state, BallState::Live { .. })));
    }

    #[test]
    fn lobby_kos_do_not_score() {
        let mut m = Match::new();
        let red = m.add_player("red", false).unwrap();
        let blue = m.add_player("blue", false).unwrap();
        m.balls = vec![Ball::new(LANE, BallKind::Normal)];
        m.set_pose(red, LANE, 0.0);
        m.set_pose(blue, LANE + Vec3::new(0.0, 0.0, -10.0), 0.0);
        m.step(DT);
        m.players.iter_mut().find(|p| p.id == blue).unwrap().hp = 1;
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        run(&mut m, 0.6);
        assert!(!m.player(blue).unwrap().alive());
        assert_eq!(m.score, [0, 0]);
    }

    #[test]
    fn reaching_target_ends_match() {
        let (mut m, red, blue) = duel();
        m.score = [KO_TARGET - 1, 0];
        m.players.iter_mut().find(|p| p.id == blue).unwrap().hp = 1;
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        run(&mut m, 0.6);
        assert_eq!(m.phase, Phase::Ended { winner: Team::Red });
    }

    #[test]
    fn rebalance_moves_bots_first() {
        let mut m = Match::new();
        m.add_player("h1", false);
        m.add_player("h2", false);
        m.add_player("b1", true);
        // Force a lopsided 3-0 split.
        for p in &mut m.players {
            p.team = Team::Red;
        }
        m.players.push(Player { bot: false, team: Team::Red, ..m.players[0].clone() });
        m.players[3].id = 99;
        m.start();
        assert_eq!(m.team_count(Team::Red), 2);
        assert_eq!(m.player(2).unwrap().team, Team::Blue, "the bot moves");
    }

    #[test]
    fn leaving_player_drops_ball() {
        let (mut m, red, _blue) = duel();
        m.remove_player(red);
        assert_eq!(m.balls[0].state, BallState::Free);
    }

    /// Duel where red holds a ball of `kind`.
    fn duel_with(kind: BallKind) -> (Match, PlayerId, PlayerId) {
        let (mut m, red, blue) = duel();
        m.balls[0].kind = kind;
        (m, red, blue)
    }

    #[test]
    fn catch_too_early_misses_and_goes_on_cooldown() {
        let (mut m, red, blue) = duel();
        m.apply(blue, Action::Catch);
        run(&mut m, 0.5); // window over, still on cooldown
        m.apply(blue, Action::Catch); // ignored: cooldown
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        run(&mut m, 0.5);
        assert_eq!(m.player(blue).unwrap().hp, MAX_HP - 1);
    }

    #[test]
    fn hearts_are_eight_halves() {
        assert_eq!(MAX_HP, HEARTS * 2);
    }

    #[test]
    fn brick_takes_two_hearts_and_knocks_flat() {
        let (mut m, red, blue) = duel();
        m.balls[0].kind = BallKind::Brick;
        let hits_before = m.player(blue).unwrap().hits;
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        run(&mut m, 0.5);
        let b = m.player(blue).unwrap();
        assert_eq!(b.hp, MAX_HP - 4, "two full hearts");
        assert!(b.stun > STUN_SECS - 0.5);
        assert_eq!(b.hits, hits_before + 1);
        assert!(m.events.contains(&GameEvent::Stunned { thrower: red, target: blue }));
        // Can't move, throw or catch while flat.
        let at = b.pos;
        m.set_pose(blue, at + Vec3::X * 3.0, 0.0);
        assert_eq!(m.player(blue).unwrap().pos, at);
        m.apply(blue, Action::Catch);
        assert_eq!(m.player(blue).unwrap().catch_window, 0.0);
        run(&mut m, STUN_SECS);
        assert_eq!(m.player(blue).unwrap().stun, 0.0, "gets back up");
        m.set_pose(blue, at + Vec3::X * 3.0, 0.0);
        assert_eq!(m.player(blue).unwrap().pos, at + Vec3::X * 3.0);
    }

    #[test]
    fn deathmatch_has_no_respawns_and_ends_on_elimination() {
        let (mut m, red, blue) = duel();
        m.phase = Phase::Lobby;
        assert!(m.set_mode(GameMode::Deathmatch));
        m.start();
        m.balls = vec![Ball::new(LANE, BallKind::Normal)];
        m.set_pose(red, LANE, 0.0);
        m.set_pose(blue, LANE + Vec3::new(0.0, 0.0, -10.0), std::f32::consts::PI);
        m.step(DT);
        m.players.iter_mut().find(|p| p.id == blue).unwrap().hp = 1;
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        run(&mut m, 0.6);
        assert!(!m.player(blue).unwrap().alive());
        assert_eq!(m.phase, Phase::Ended { winner: Team::Red }, "blue team wiped out");
        // Back in the lobby everyone is up again.
        m.to_lobby();
        assert!(m.player(blue).unwrap().alive());
    }

    #[test]
    fn deathmatch_keeps_out_players_out() {
        let mut m = Match::new();
        for i in 0..4 {
            m.add_player(&format!("p{i}"), false);
        }
        m.set_mode(GameMode::Deathmatch);
        m.start();
        // Knock out one of two reds: match goes on, they stay out.
        let red = m.players.iter().find(|p| p.team == Team::Red).unwrap().id;
        m.players.iter_mut().find(|p| p.id == red).unwrap().respawn_timer = RESPAWN_SECS;
        run(&mut m, RESPAWN_SECS + 1.0);
        assert!(!m.player(red).unwrap().alive());
        assert_eq!(m.phase, Phase::Playing);
    }

    #[test]
    fn mode_and_map_only_change_between_matches() {
        let (mut m, _, _) = duel();
        assert!(!m.set_mode(GameMode::Deathmatch));
        assert!(!m.set_map(MapId::Night));
        m.to_lobby();
        assert!(m.set_map(MapId::Night));
        assert_eq!(m.snapshot().map, MapId::Night);
    }

    #[test]
    fn trick_is_reported_and_cleared() {
        let (mut m, red, _) = duel();
        m.set_trick(red, Some(TrickState { kind: TrickKind::BackFlip, progress: 0.4 }));
        let snap = m.snapshot();
        let r = snap.players.iter().find(|p| p.id == red).unwrap();
        assert_eq!(r.trick.map(|t| t.kind), Some(TrickKind::BackFlip));
        m.set_trick(red, None);
        assert!(m.player(red).unwrap().trick.is_none());
    }

    #[test]
    fn lock_on_homes_onto_target() {
        // Aim 15 degrees off: misses without a lock, hits with one.
        let off = Vec3::new(-(0.26_f32).sin(), -0.05, -(0.26_f32).cos());
        let (mut m, red, blue) = duel();
        throw_at(&mut m, red, off);
        run(&mut m, 1.0);
        assert_eq!(m.player(blue).unwrap().hp, MAX_HP, "no lock: miss");

        let (mut m, red, blue) = duel();
        m.apply(red, Action::Lock { target: Some(blue) });
        assert_eq!(m.player(red).unwrap().lock, Some(blue));
        throw_at(&mut m, red, off);
        run(&mut m, 1.0);
        assert_eq!(m.player(blue).unwrap().hp, MAX_HP - 1, "lock: homes in and hits");
    }

    #[test]
    fn cannot_lock_on_teammate_and_lock_drops_on_knockout() {
        let (mut m, red, blue) = duel();
        m.apply(red, Action::Lock { target: Some(red) });
        assert_eq!(m.player(red).unwrap().lock, None);
        m.apply(red, Action::Lock { target: Some(blue) });
        m.players.iter_mut().find(|p| p.id == blue).unwrap().hp = 1;
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        run(&mut m, 0.6);
        assert!(!m.player(blue).unwrap().alive());
        assert_eq!(m.player(red).unwrap().lock, None);
    }

    #[test]
    fn boomerang_returns_charged_after_hitting_opponent() {
        let (mut m, red, blue) = duel_with(BallKind::Boomerang);
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        run(&mut m, 1.5);
        assert_eq!(m.player(blue).unwrap().hp, MAX_HP - 1, "it still hits");
        let r = m.player(red).unwrap();
        assert_eq!(r.holding, Some(0), "came back to the thrower");
        assert!(r.charged, "and is charged");
        assert!(!r.powered, "charged is not a one-hit knockout");
        assert!(m.events.contains(&GameEvent::BoomerangBack { owner: red }));
    }

    #[test]
    fn boomerang_returns_after_hitting_a_wall() {
        let (mut m, red, blue) = duel_with(BallKind::Boomerang);
        m.set_pose(blue, LANE + Vec3::new(5.0, 0.0, -10.0), 0.0); // out of the way
        throw_at(&mut m, red, Vec3::new(0.0, 0.0, -1.0)); // at the far wall (z = -20)
        let mut saw_return = false;
        for _ in 0..240 {
            m.step(DT);
            saw_return |= matches!(m.balls[0].state, BallState::Returning { owner } if owner == red);
        }
        assert!(saw_return);
        assert_eq!(m.player(red).unwrap().holding, Some(0));
        assert!(m.player(red).unwrap().charged);
    }

    #[test]
    fn charged_throw_is_full_speed_but_normal_damage() {
        let (mut m, red, blue) = duel();
        m.players.iter_mut().find(|p| p.id == red).unwrap().charged = true;
        m.apply(red, Action::Throw { aim: [0.0, -0.05, -1.0], charge: 0.0, curve: 0.0, lob: false });
        let speed = m.balls[0].vel.length();
        assert!((speed - (ball::MAX_THROW_SPEED + ball::POWER_BONUS)).abs() < 0.5, "speed {speed}");
        assert!(!m.player(red).unwrap().charged, "used up");
        run(&mut m, 0.5);
        assert_eq!(m.player(blue).unwrap().hp, MAX_HP - 1);
    }

    #[test]
    fn normal_ball_does_not_return() {
        let (mut m, red, _blue) = duel();
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        run(&mut m, 1.5);
        assert!(!m.events.contains(&GameEvent::BoomerangBack { owner: red }));
        assert!(!m.player(red).unwrap().charged);
    }

    #[test]
    fn slime_sticks_target_to_nearest_wall() {
        let (mut m, red, blue) = duel_with(BallKind::Slime);
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        run(&mut m, 0.5);
        let b = m.player(blue).unwrap();
        assert_eq!(b.hp, MAX_HP - 1);
        let slime = b.slime.expect("slimed");
        assert!(m.events.contains(&GameEvent::Slimed { thrower: red, target: blue }));
        // Blue stood at x=14, z=-10: the cover block at (9, -12) is nearer than the x=20 wall.
        let expected = layout::nearest_wall_anchor(LANE + Vec3::new(0.0, 0.0, -10.0), &m.solids, BODY_RADIUS);
        assert_eq!(slime.anchor, expected);
        assert!(expected.distance(LANE + Vec3::new(0.0, 0.0, -10.0)) < layout::ARENA_HALF - LANE.x, "{expected}");
        run(&mut m, 0.6);
        let b = m.player(blue).unwrap();
        assert!((b.pos.x - slime.anchor.x).abs() < 1e-3, "dragged to the wall");
        // Client pose is ignored while stuck.
        m.set_pose(blue, Vec3::new(0.0, 0.0, -5.0), 0.0);
        assert!((m.player(blue).unwrap().pos.x - slime.anchor.x).abs() < 1e-3);
        run(&mut m, SLIME_SECS);
        assert!(m.player(blue).unwrap().slime.is_none(), "wears off");
        m.set_pose(blue, Vec3::new(0.0, 0.0, -5.0), 0.0);
        assert_eq!(m.player(blue).unwrap().pos.x, 0.0, "free to move again");
    }

    #[test]
    fn slime_knockout_does_not_stick() {
        let (mut m, red, blue) = duel_with(BallKind::Slime);
        m.players.iter_mut().find(|p| p.id == blue).unwrap().hp = 1;
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        run(&mut m, 0.5);
        let b = m.player(blue).unwrap();
        assert!(!b.alive());
        assert!(b.slime.is_none());
    }

    #[test]
    fn match_has_one_boomerang_and_one_slime() {
        let m = Match::new();
        assert_eq!(m.balls.len(), 7);
        assert_eq!(m.balls.iter().filter(|b| b.kind == BallKind::Brick).count(), 1);
        assert_eq!(m.balls.iter().filter(|b| b.kind == BallKind::Boomerang).count(), 1);
        assert_eq!(m.balls.iter().filter(|b| b.kind == BallKind::Slime).count(), 1);
    }

    /// Move the sim until ball 0 is about `secs` from reaching `target`'s chest.
    fn run_until_eta(m: &mut Match, target: PlayerId, secs: f32) {
        for _ in 0..120 {
            let chest = m.player(target).unwrap().pos + Vec3::Y * 1.1;
            let b = m.balls[0];
            if b.pos.distance(chest) - BODY_RADIUS <= b.vel.length() * secs {
                return;
            }
            m.step(DT);
        }
        panic!("ball never got close");
    }

    #[test]
    fn lob_drops_onto_the_head() {
        let (mut m, red, blue) = duel();
        m.apply(red, Action::Throw { aim: [0.0, 0.0, -1.0], charge: 0.0, curve: 0.0, lob: true });
        run(&mut m, 2.5);
        assert_eq!(m.player(blue).unwrap().hp, MAX_HP - 1, "the lob came down on blue");
    }

    #[test]
    fn catching_blocks_damage_even_from_behind() {
        let (mut m, red, blue) = duel();
        m.set_pose(blue, LANE + Vec3::new(0.0, 0.0, -10.0), 0.0); // back to the thrower
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        run_until_eta(&mut m, blue, 0.2);
        m.apply(blue, Action::Catch);
        run(&mut m, 0.5);
        assert_eq!(m.player(blue).unwrap().hp, MAX_HP, "no damage while catching");
        assert!(
            !m.events.iter().any(|e| matches!(e, GameEvent::Caught { .. })),
            "can't catch what you don't face"
        );
    }

    #[test]
    fn slimed_player_can_do_nothing() {
        let (mut m, red, blue) = duel();
        m.players.iter_mut().find(|p| p.id == red).unwrap().slime = Some(Slime { anchor: LANE, timer: 2.0 });
        let yaw = m.player(red).unwrap().yaw;
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        assert_eq!(m.player(red).unwrap().holding, Some(0), "can't throw");
        m.apply(red, Action::Catch);
        assert_eq!(m.player(red).unwrap().catch_window, 0.0, "can't catch");
        m.set_pose(red, LANE, yaw + 1.0);
        assert_eq!(m.player(red).unwrap().yaw, yaw, "can't even turn");
        let _ = blue;
    }

    #[test]
    fn perfect_catch_charges_and_stacks_speed() {
        let (mut m, red, blue) = duel();
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        run_until_eta(&mut m, blue, 0.08);
        m.apply(blue, Action::Catch);
        run(&mut m, 0.3);
        let b = m.player(blue).unwrap();
        assert_eq!(b.holding, Some(0));
        assert!(b.charged, "perfect catch keeps it fully charged");
        assert_eq!(m.balls[0].stacks, 1);
        assert!(m.events.iter().any(|e| matches!(e, GameEvent::Caught { perfect: true, .. })));
        // Thrown back: full charge + bonus, then doubled by the stack.
        m.apply(blue, Action::Throw { aim: [0.0, -0.05, 1.0], charge: 0.0, curve: 0.0, lob: false });
        let speed = m.balls[0].vel.length();
        let base = ball::MAX_THROW_SPEED + ball::POWER_BONUS;
        assert!((speed - base * 2.0).abs() < 1.0, "speed {speed}");
    }

    #[test]
    fn early_catch_is_not_perfect() {
        let (mut m, red, blue) = duel();
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        run_until_eta(&mut m, blue, 0.3);
        m.apply(blue, Action::Catch);
        run(&mut m, 0.4);
        assert_eq!(m.player(blue).unwrap().holding, Some(0));
        assert!(!m.player(blue).unwrap().charged);
        assert_eq!(m.balls[0].stacks, 0);
    }

    #[test]
    fn stacks_cap_at_four() {
        let (mut m, red, blue) = duel();
        m.balls[0].stacks = ball::MAX_STACKS;
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        run_until_eta(&mut m, blue, 0.08);
        m.apply(blue, Action::Catch);
        run(&mut m, 0.2);
        assert_eq!(m.balls[0].stacks, ball::MAX_STACKS);
    }

    #[test]
    fn wall_ricochet_knocks_back_without_damage() {
        let (mut m, red, _blue) = duel();
        // Red throws at the +X arena wall 6 m away; it comes back twice as fast at red.
        throw_at(&mut m, red, Vec3::new(1.0, -0.02, 0.0));
        let thrown = m.balls[0].vel.length();
        let mut saw = None;
        for _ in 0..60 {
            m.step(DT);
            if matches!(m.balls[0].state, BallState::Ricochet { .. }) && saw.is_none() {
                saw = Some(m.balls[0].vel.length());
            }
        }
        let back = saw.expect("ricocheted");
        assert!(back > thrown * 1.7, "about double speed: {thrown} -> {back}");
        let r = m.player(red).unwrap();
        assert_eq!(r.hp, MAX_HP, "no damage");
        assert!(m.events.contains(&GameEvent::KnockedBack { target: red }));
        assert_eq!(m.score, [0, 0], "no points");
    }

    #[test]
    fn knocked_player_flies_then_lands() {
        let (mut m, red, _) = duel();
        let i = m.players.iter().position(|p| p.id == red).unwrap();
        m.launch(i, Vec3::new(-5.0, 6.0, 0.0));
        let start = m.player(red).unwrap().pos;
        assert!(m.player(red).unwrap().holding.is_none(), "dropped the ball");
        run(&mut m, 0.2);
        assert!(m.player(red).unwrap().pos.y > 0.5, "in the air");
        run(&mut m, 1.5);
        let r = m.player(red).unwrap();
        assert!(r.knock.is_none() && r.pos.y == 0.0);
        assert!(r.pos.x < start.x - 2.0, "pushed along");
    }

    #[test]
    fn dash_tackle_launches_enemy_and_their_ball_once() {
        let (mut m, red, blue) = duel();
        // Give blue the ball, put red right next to them, mid-dash.
        m.players.iter_mut().find(|p| p.id == red).unwrap().holding = None;
        m.balls[0].state = BallState::Held { by: blue };
        m.players.iter_mut().find(|p| p.id == blue).unwrap().holding = Some(0);
        let bpos = m.player(blue).unwrap().pos;
        m.set_pose(red, bpos + Vec3::new(0.0, 0.0, 0.8), 0.0);
        m.set_dash(red, Some(7));
        m.step(DT);
        let b = m.player(blue).unwrap();
        assert!(b.knock.is_some(), "launched");
        assert!(b.holding.is_none());
        assert!(m.balls[0].vel.y > 5.0, "ball pops up");
        assert!(m.events.contains(&GameEvent::Tackled { by: red, target: blue }));
        // Same dash can't tackle them again after they land.
        run(&mut m, 2.0);
        m.set_pose(blue, bpos, 0.0);
        m.set_pose(red, bpos + Vec3::new(0.0, 0.0, 0.8), 0.0);
        m.set_dash(red, Some(7));
        m.step(DT);
        assert!(m.player(blue).unwrap().knock.is_none());
    }

    #[test]
    fn teammates_are_not_tackled() {
        let mut m = Match::new();
        let a = m.add_player("a", false).unwrap();
        m.add_player("enemy", true).unwrap();
        let c = m.add_player("c", false).unwrap();
        m.players.retain(|p| !p.bot);
        m.set_pose(a, LANE, 0.0);
        m.set_pose(c, LANE + Vec3::Z * 0.8, 0.0);
        m.set_dash(c, Some(1));
        m.step(DT);
        assert!(m.player(a).unwrap().knock.is_none());
    }

    #[test]
    fn dashing_is_invincible() {
        let (mut m, red, blue) = duel();
        m.set_dash(blue, Some(1));
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        run(&mut m, 0.5);
        assert_eq!(m.player(blue).unwrap().hp, MAX_HP);
    }

    /// Red at z=+8 behind... blue hiding behind the x=9 pillar (x 8..10, z -1..1).
    fn hide_and_seek() -> (Match, PlayerId, PlayerId) {
        let (mut m, red, blue) = duel();
        m.set_pose(red, Vec3::new(9.0, 0.0, 8.0), 0.0);
        m.set_pose(blue, Vec3::new(9.0, 0.0, -6.0), 0.0);
        m.step(DT);
        (m, red, blue)
    }

    #[test]
    fn curve_wraps_around_cover_to_hit_the_hider() {
        let (mut m, red, blue) = hide_and_seek();
        m.apply(red, Action::Throw { aim: [0.0, -0.02, -1.0], charge: 0.5, curve: 1.0, lob: false });
        assert!(matches!(m.balls[0].state, BallState::Live { track: Some(t), .. } if t == blue));
        run(&mut m, 1.5);
        assert_eq!(m.player(blue).unwrap().hp, MAX_HP - 1, "went around the pillar");
    }

    /// Blue keeps running sideways at sprint speed while a ball is in the air.
    fn run_strafing(m: &mut Match, blue: PlayerId, speed: f32, secs: f32) {
        for _ in 0..(secs / DT) as usize {
            let p = m.player(blue).unwrap().pos;
            m.set_pose(blue, p + Vec3::X * speed * DT, 0.0);
            m.step(DT);
        }
    }

    #[test]
    fn lob_tracks_a_runner_onto_the_head() {
        let (mut m, red, blue) = duel();
        m.apply(red, Action::Throw { aim: [0.0, 0.0, -1.0], charge: 0.0, curve: 0.0, lob: true });
        assert!(matches!(m.balls[0].state, BallState::Live { track: Some(t), .. } if t == blue));
        run_strafing(&mut m, blue, -9.8, 1.5);
        assert_eq!(m.player(blue).unwrap().hp, MAX_HP - 1, "the lob followed blue");
    }

    #[test]
    fn curve_tracks_a_runner() {
        let (mut m, red, blue) = duel();
        m.apply(red, Action::Throw { aim: [0.0, -0.02, -1.0], charge: 0.3, curve: -1.0, lob: false });
        run_strafing(&mut m, blue, -9.8, 1.5);
        assert_eq!(m.player(blue).unwrap().hp, MAX_HP - 1, "the curve followed blue");
    }

    #[test]
    fn dashing_shakes_off_a_tracking_lob() {
        let (mut m, red, blue) = duel();
        m.apply(red, Action::Throw { aim: [0.0, 0.0, -1.0], charge: 0.0, curve: 0.0, lob: true });
        run(&mut m, 0.3);
        // Blue dashes 6 m sideways mid-flight.
        m.set_dash(blue, Some(1));
        let p = m.player(blue).unwrap().pos;
        m.set_pose(blue, p + Vec3::X * 6.0, 0.0);
        m.step(DT);
        m.set_dash(blue, None);
        assert!(matches!(m.balls[0].state, BallState::Live { track: None, .. }), "tracking lost");
        run(&mut m, 2.0);
        assert_eq!(m.player(blue).unwrap().hp, MAX_HP, "dodged");
    }

    #[test]
    fn straight_throw_at_cover_does_not_hit_the_hider() {
        let (mut m, red, blue) = hide_and_seek();
        m.apply(red, Action::Throw { aim: [0.0, -0.02, -1.0], charge: 0.5, curve: 0.0, lob: false });
        run(&mut m, 1.5);
        assert_eq!(m.player(blue).unwrap().hp, MAX_HP);
    }

    #[test]
    fn segment_body_test() {
        let feet = Vec3::new(0.0, 0.0, -5.0);
        assert!(segment_hits_body(Vec3::new(0.0, 1.0, -4.0), Vec3::new(0.0, 1.0, -6.0), feet, BODY_HEIGHT).is_some());
        assert!(segment_hits_body(Vec3::new(2.0, 1.0, -4.0), Vec3::new(2.0, 1.0, -6.0), feet, BODY_HEIGHT).is_none());
        assert!(segment_hits_body(Vec3::new(0.0, 3.0, -4.0), Vec3::new(0.0, 3.0, -6.0), feet, BODY_HEIGHT).is_none());
        // Crouched: a ball skimming a 1.2 m wall (centre at 1.6 m) flies over.
        let high = (Vec3::new(0.0, 1.6, -4.0), Vec3::new(0.0, 1.6, -6.0));
        assert!(segment_hits_body(high.0, high.1, feet, BODY_HEIGHT).is_some());
        assert!(segment_hits_body(high.0, high.1, feet, CROUCH_HEIGHT).is_none());
    }

    #[test]
    fn snapshot_round_trips_through_bincode() {
        let (m, _, _) = duel();
        let s = m.snapshot();
        let bytes = bincode::serialize(&s).unwrap();
        let back: Snapshot = bincode::deserialize(&bytes).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn a_big_combo_charges_the_ball_in_hand() {
        let (mut m, red, _blue) = duel();
        let ri = m.players.iter().position(|p| p.id == red).unwrap();
        m.players[ri].holding = None;
        m.balls[0].state = BallState::Free;
        m.apply(red, Action::Style { points: 1500 });
        assert!(!m.players[ri].charged, "no ball, nothing to charge");
        assert!(m.events.iter().any(|e| matches!(e, GameEvent::Combo { points: 1500, charged: false, .. })));
        m.players[ri].holding = Some(0);
        m.balls[0].state = BallState::Held { by: red };
        m.apply(red, Action::Style { points: 600 });
        assert!(!m.players[ri].charged, "too small");
        m.apply(red, Action::Style { points: 1200 });
        assert!(m.players[ri].charged);
        assert!(m.events.iter().any(|e| matches!(e, GameEvent::Combo { charged: true, .. })));
    }

    #[test]
    fn ride_reports_are_sanitized() {
        let (mut m, red, _) = duel();
        let v = RideView { mount: MountKind::Bmx, trick: Some((TrickCat::Flip, 2, 0.5)), grind: None, bail: false, speed: 999.0, push: 1.0 };
        m.set_ride(red, Some(v));
        assert_eq!(m.snapshot().players.iter().find(|p| p.id == red).unwrap().ride.unwrap().speed, 40.0);
        m.set_ride(red, Some(RideView { speed: f32::NAN, ..v }));
        assert!(m.player(red).unwrap().ride.is_none());
    }
}
