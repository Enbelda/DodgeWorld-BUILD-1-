//! Authoritative match simulation. Plain Rust, no ECS: the host owns one `Match`,
//! feeds it player poses and actions, steps it, and sends `snapshot()` to everyone.

pub mod ball;
pub mod bot;
pub mod layout;

use bevy::math::Vec3;
use serde::{Deserialize, Serialize};

use ball::{BallKind, ThrowParams, BALL_RADIUS};
use layout::{Aabb, ARENA_HALF};

pub type PlayerId = u8;
pub const MAX_PLAYERS: usize = 4;
pub const MAX_HP: u8 = 2;
pub const KO_TARGET: u32 = 10;
pub const RESPAWN_SECS: f32 = 3.0;
pub const CATCH_WINDOW: f32 = 0.4;
pub const CATCH_COOLDOWN: f32 = 1.0;
pub const THROW_COOLDOWN: f32 = 0.35;
pub const PICKUP_RADIUS: f32 = 1.3;
/// Player body for hit tests: vertical capsule from feet to head.
pub const BODY_RADIUS: f32 = 0.45;
pub const BODY_HEIGHT: f32 = 1.8;
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
    /// Holding the catch button: catches the moment a ball arrives.
    pub catch_held: bool,
    pub throw_cooldown: f32,
    /// Enemy this player is locked on to; their throws home in on it.
    pub lock: Option<PlayerId>,
    pub slime: Option<Slime>,
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
        self.pos + Vec3::Y * HOLD_HEIGHT + self.forward() * 0.45
    }
    pub fn catching(&self) -> bool {
        self.catch_window > 0.0 || self.catch_held
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub enum BallState {
    /// Lying around (or still rolling); anyone can pick it up.
    Free,
    Held { by: PlayerId },
    /// Thrown and dangerous to `team` enemies until it touches something.
    /// `homing` is the locked-on target the ball steers toward.
    Live { thrower: PlayerId, team: Team, curve: f32, lob: bool, powered: bool, homing: Option<PlayerId> },
    /// A boomerang flying back to its thrower. Harmless; arrives charged.
    Returning { owner: PlayerId },
}

#[derive(Clone, Copy, Debug)]
pub struct Ball {
    pub pos: Vec3,
    pub vel: Vec3,
    pub state: BallState,
    pub pad: Vec3,
    pub kind: BallKind,
}

impl Ball {
    pub fn new(pad: Vec3, kind: BallKind) -> Self {
        Ball { pos: pad, vel: Vec3::ZERO, state: BallState::Free, pad, kind }
    }
}

/// Things that happened during a step, for HUD messages and sounds.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum GameEvent {
    Hit { thrower: PlayerId, target: PlayerId },
    Knockout { thrower: PlayerId, target: PlayerId },
    Caught { catcher: PlayerId, thrower: PlayerId },
    Slimed { thrower: PlayerId, target: PlayerId },
    BoomerangBack { owner: PlayerId },
    MatchStarted,
    MatchEnded { winner: Team },
}

pub struct Match {
    pub phase: Phase,
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
            players: Vec::new(),
            balls: Vec::new(),
            score: [0, 0],
            solids: layout::solids(),
            events: Vec::new(),
            rng: Rng(0x9E37_79B9_7F4A_7C15),
            next_id: 0,
        };
        m.reset_balls();
        m
    }

    pub fn reset_balls(&mut self) {
        self.balls = layout::ball_pads()
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
            pos: layout::spawn_point(team == Team::Red, slot),
            yaw: if team == Team::Red { 0.0 } else { std::f32::consts::PI },
            hp: MAX_HP,
            holding: None,
            powered: false,
            charged: false,
            catch_window: 0.0,
            catch_cooldown: 0.0,
            catch_held: false,
            throw_cooldown: 0.0,
            lock: None,
            slime: None,
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
    /// While slimed the host owns the position; only the facing is taken.
    pub fn set_pose(&mut self, id: PlayerId, pos: Vec3, yaw: f32) {
        if let Some(p) = self.player_mut(id) {
            if p.alive() && pos.is_finite() && yaw.is_finite() {
                p.yaw = yaw;
                if p.slime.is_none() {
                    let lim = ARENA_HALF + 2.0;
                    p.pos = pos.clamp(Vec3::new(-lim, -50.0, -lim), Vec3::new(lim, 50.0, lim));
                }
            }
        }
    }

    /// A human holds (or releases) the catch button.
    pub fn set_catch_hold(&mut self, id: PlayerId, held: bool) {
        if let Some(p) = self.player_mut(id) {
            p.catch_held = held && p.alive();
        }
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

    /// Back to the lobby after a match.
    pub fn to_lobby(&mut self) {
        self.phase = Phase::Lobby;
        self.score = [0, 0];
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
        p.pos = layout::spawn_point(team == Team::Red, slot);
        p.yaw = if team == Team::Red { 0.0 } else { std::f32::consts::PI };
        p.hp = MAX_HP;
        p.respawn_timer = 0.0;
        p.catch_window = 0.0;
        p.catch_cooldown = 0.0;
        p.slime = None;
        p.lock = None;
        p.teleports = p.teleports.wrapping_add(1);
    }

    /// Apply a player action. Invalid actions (dead, no ball, on cooldown) are ignored.
    pub fn apply(&mut self, id: PlayerId, action: Action) {
        let Some(pi) = self.players.iter().position(|p| p.id == id) else { return };
        if !self.players[pi].alive() || matches!(self.phase, Phase::Ended { .. }) {
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
                let (vel, curve) = ball::launch(aim, params);
                p.holding = None;
                p.powered = false;
                p.charged = false;
                p.throw_cooldown = THROW_COOLDOWN;
                let (team, origin, lock) = (p.team, p.hold_point(), p.lock);
                let homing = lock.filter(|t| self.valid_lock(team, *t));
                let b = &mut self.balls[bi];
                b.pos = origin;
                b.vel = vel;
                b.state = BallState::Live { thrower: id, team, curve, lob, powered, homing };
            }
            Action::Lock { target } => {
                let team = self.players[pi].team;
                self.players[pi].lock = target.filter(|t| self.valid_lock(team, *t));
            }
        }
    }

    /// Can someone on `team` lock on to `target`? It must be a living enemy.
    fn valid_lock(&self, team: Team, target: PlayerId) -> bool {
        self.player(target).is_some_and(|t| t.team != team && t.alive())
    }

    pub fn step(&mut self, dt: f32) {
        self.tick_players(dt);
        self.run_bots(dt);
        self.step_balls(dt);
        self.pickups();
    }

    fn tick_players(&mut self, dt: f32) {
        // Drop locks on targets that died, left or switched team.
        let locks: Vec<Option<PlayerId>> =
            self.players.iter().map(|p| p.lock.filter(|t| self.valid_lock(p.team, *t))).collect();
        for (p, lock) in self.players.iter_mut().zip(locks) {
            p.lock = lock;
        }
        let mut to_respawn = Vec::new();
        for p in &mut self.players {
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
                p.respawn_timer -= dt;
                if p.respawn_timer <= 0.0 {
                    to_respawn.push(p.id);
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

    fn run_bots(&mut self, dt: f32) {
        let ids: Vec<PlayerId> = self.players.iter().filter(|p| p.bot && p.alive()).map(|p| p.id).collect();
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
                        None => self.balls[bi].state = BallState::Free,
                    }
                }
                BallState::Free => {
                    let b = &mut self.balls[bi];
                    if b.vel != Vec3::ZERO || b.pos.y > BALL_RADIUS + 0.01 {
                        let g = ball::gravity(false, false);
                        ball::step(&mut b.pos, &mut b.vel, 0.0, g, dt, &self.solids);
                    }
                }
                BallState::Live { thrower, team, curve, lob, powered, homing } => {
                    let g = ball::gravity(lob, true);
                    let aim_at =
                        homing.and_then(|t| self.player(t)).filter(|t| t.alive()).map(|t| t.pos + Vec3::Y * 1.0);
                    let b = &mut self.balls[bi];
                    if let Some(target) = aim_at {
                        ball::home(&mut b.vel, target - b.pos, dt, lob);
                    }
                    let prev = b.pos;
                    let r = ball::step(&mut b.pos, &mut b.vel, curve, g, dt, &self.solids);
                    let now = b.pos;
                    if let Some(target) = self.first_enemy_hit(team, prev, now) {
                        self.resolve_hit(bi, thrower, target, powered);
                    } else if r.touched {
                        let b = &mut self.balls[bi];
                        b.state = match b.kind {
                            BallKind::Boomerang => BallState::Returning { owner: thrower },
                            _ => BallState::Free,
                        };
                    }
                }
                BallState::Returning { owner } => {
                    let home =
                        self.player(owner).filter(|p| p.alive()).map(|p| (p.hold_point(), p.holding.is_some()));
                    let Some((at, hands_full)) = home else {
                        self.balls[bi].state = BallState::Free;
                        continue;
                    };
                    let b = &mut self.balls[bi];
                    if ball::fly_back(&mut b.pos, &mut b.vel, at, dt) {
                        if hands_full {
                            // Owner grabbed another ball meanwhile: drop it at their feet.
                            b.state = BallState::Free;
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
            let lim = ARENA_HALF + 3.0;
            if b.pos.x.abs() > lim || b.pos.z.abs() > lim || b.pos.y < -5.0 || !b.pos.is_finite() {
                if let BallState::Held { by } = b.state {
                    if let Some(p) = self.players.iter_mut().find(|p| p.id == by) {
                        p.holding = None;
                    }
                }
                let b = &mut self.balls[bi];
                b.pos = b.pad;
                b.vel = Vec3::ZERO;
                b.state = BallState::Free;
            }
        }
    }

    /// The enemy (of `team`) whose body the ball's path from `a` to `b` crosses first.
    fn first_enemy_hit(&self, team: Team, a: Vec3, b: Vec3) -> Option<PlayerId> {
        self.players
            .iter()
            .filter(|p| p.team != team && p.alive())
            .filter_map(|p| segment_hits_body(a, b, p.pos).map(|t| (t, p.id)))
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
            let t = &mut self.players[ti];
            t.holding = Some(bi);
            t.powered = true;
            t.catch_window = 0.0;
            t.catch_cooldown = 0.0;
            self.balls[bi].state = BallState::Held { by: target };
            self.events.push(GameEvent::Caught { catcher: target, thrower });
            return;
        }

        // Hit: a boomerang heads home, anything else drops dead and bounces off.
        let b = &mut self.balls[bi];
        let kind = b.kind;
        b.state = match kind {
            BallKind::Boomerang => BallState::Returning { owner: thrower },
            _ => BallState::Free,
        };
        b.vel = Vec3::new(-ball_vel.x * 0.15, 3.0, -ball_vel.z * 0.15);

        let dmg = if powered { MAX_HP } else { 1 };
        let t = &mut self.players[ti];
        t.hp = t.hp.saturating_sub(dmg);
        if t.hp > 0 {
            self.events.push(GameEvent::Hit { thrower, target });
            if kind == BallKind::Slime {
                let anchor = layout::nearest_wall_anchor(self.players[ti].pos, &self.solids, BODY_RADIUS);
                self.players[ti].slime = Some(Slime { anchor, timer: SLIME_SECS });
                self.events.push(GameEvent::Slimed { thrower, target });
            }
            return;
        }
        t.respawn_timer = RESPAWN_SECS;
        t.catch_window = 0.0;
        t.catch_held = false;
        t.slime = None;
        let counts = self.phase == Phase::Playing;
        if counts {
            t.deaths += 1;
        }
        let victim_team = t.team;
        if let Some(held) = t.holding.take() {
            self.balls[held].state = BallState::Free;
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
            }
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            phase: self.phase,
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
                .map(|b| BallView { pos: b.pos.to_array(), vel: b.vel.to_array(), state: b.state, kind: b.kind })
                .collect(),
            events: self.events.clone(),
        }
    }
}

/// Where along segment a->b (0..1) the ball first touches a standing body at `feet`.
pub fn segment_hits_body(a: Vec3, b: Vec3, feet: Vec3) -> Option<f32> {
    let r = BODY_RADIUS + BALL_RADIUS;
    let bottom = feet + Vec3::Y * BODY_RADIUS;
    let top = feet + Vec3::Y * (BODY_HEIGHT - BODY_RADIUS);
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
        assert!(m.events.contains(&GameEvent::Caught { catcher: blue, thrower: red }));
    }

    #[test]
    fn no_catch_from_behind() {
        let (mut m, red, blue) = duel();
        m.set_pose(blue, LANE + Vec3::new(0.0, 0.0, -10.0), 0.0); // facing away from red
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        m.apply(blue, Action::Catch);
        run(&mut m, 0.5);
        assert_eq!(m.player(blue).unwrap().hp, MAX_HP - 1);
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
    fn powered_throw_knocks_out_in_one_hit() {
        let (mut m, red, blue) = duel();
        m.players.iter_mut().find(|p| p.id == red).unwrap().powered = true;
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        run(&mut m, 0.5);
        assert!(!m.player(blue).unwrap().alive());
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
    fn held_catch_catches_whenever_the_ball_arrives() {
        let (mut m, red, blue) = duel();
        // Catch held well before the throw: no timing window needed.
        m.set_catch_hold(blue, true);
        run(&mut m, 1.0);
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        run(&mut m, 0.5);
        let b = m.player(blue).unwrap();
        assert_eq!(b.hp, MAX_HP);
        assert_eq!(b.holding, Some(0));
    }

    #[test]
    fn released_catch_does_not_catch() {
        let (mut m, red, blue) = duel();
        m.set_catch_hold(blue, true);
        m.set_catch_hold(blue, false);
        throw_at(&mut m, red, Vec3::new(0.0, -0.05, -1.0));
        run(&mut m, 0.5);
        assert_eq!(m.player(blue).unwrap().hp, MAX_HP - 1);
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
        assert!(expected.distance(LANE + Vec3::new(0.0, 0.0, -10.0)) < ARENA_HALF - LANE.x, "{expected}");
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
        assert_eq!(m.balls.len(), 6);
        assert_eq!(m.balls.iter().filter(|b| b.kind == BallKind::Boomerang).count(), 1);
        assert_eq!(m.balls.iter().filter(|b| b.kind == BallKind::Slime).count(), 1);
    }

    #[test]
    fn segment_body_test() {
        let feet = Vec3::new(0.0, 0.0, -5.0);
        assert!(segment_hits_body(Vec3::new(0.0, 1.0, -4.0), Vec3::new(0.0, 1.0, -6.0), feet).is_some());
        assert!(segment_hits_body(Vec3::new(2.0, 1.0, -4.0), Vec3::new(2.0, 1.0, -6.0), feet).is_none());
        assert!(segment_hits_body(Vec3::new(0.0, 3.0, -4.0), Vec3::new(0.0, 3.0, -6.0), feet).is_none());
    }

    #[test]
    fn snapshot_round_trips_through_bincode() {
        let (m, _, _) = duel();
        let s = m.snapshot();
        let bytes = bincode::serialize(&s).unwrap();
        let back: Snapshot = bincode::deserialize(&bytes).unwrap();
        assert_eq!(s, back);
    }
}
