//! LAN peer-to-peer lobby over UDP. One peer hosts: it runs the `Match` and broadcasts
//! snapshots. Other peers join by address, send their body pose every frame, and
//! resend unacknowledged actions (throw/catch) until the host confirms them.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};

use bevy::math::Vec3;
use serde::{Deserialize, Serialize};

use crate::sim::{Action, Match, PlayerId, Snapshot};

pub const PROTOCOL: u32 = 1;
pub const DEFAULT_PORT: u16 = 47800;
/// A peer that sends nothing for this long is dropped.
pub const TIMEOUT_SECS: f64 = 5.0;
const MAX_PACKET: usize = 8 * 1024;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ToHost {
    Join { protocol: u32, name: String },
    Update { pos: [f32; 3], yaw: f32, actions: Vec<(u32, Action)> },
    Leave,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ToClient {
    Welcome { id: PlayerId },
    Rejected { reason: String },
    Snapshot(Snapshot),
}

fn send<T: Serialize>(sock: &UdpSocket, to: SocketAddr, msg: &T) {
    if let Ok(bytes) = bincode::serialize(msg) {
        // UDP: a dropped packet is fine, the next one carries the same state.
        let _ = sock.send_to(&bytes, to);
    }
}

struct Peer {
    id: PlayerId,
    last_seen: f64,
    last_action: u32,
}

pub struct HostNet {
    sock: UdpSocket,
    peers: HashMap<SocketAddr, Peer>,
}

impl HostNet {
    pub fn bind(port: u16) -> std::io::Result<Self> {
        Self::bind_addr(SocketAddr::from(([0, 0, 0, 0], port)))
    }

    pub fn bind_addr(addr: SocketAddr) -> std::io::Result<Self> {
        let sock = UdpSocket::bind(addr)?;
        sock.set_nonblocking(true)?;
        Ok(HostNet { sock, peers: HashMap::new() })
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.sock.local_addr()
    }

    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Read every waiting packet and apply it to the match. `now` is seconds.
    pub fn receive(&mut self, m: &mut Match, now: f64) {
        let mut buf = [0u8; MAX_PACKET];
        loop {
            let (n, from) = match self.sock.recv_from(&mut buf) {
                Ok(x) => x,
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                // Windows reports ICMP "port unreachable" from a closed peer as an error; skip it.
                Err(_) => continue,
            };
            let Ok(msg) = bincode::deserialize::<ToHost>(&buf[..n]) else { continue };
            self.handle(m, from, msg, now);
        }
        // Drop silent peers.
        let dead: Vec<SocketAddr> =
            self.peers.iter().filter(|(_, p)| now - p.last_seen > TIMEOUT_SECS).map(|(a, _)| *a).collect();
        for addr in dead {
            if let Some(p) = self.peers.remove(&addr) {
                m.remove_player(p.id);
            }
        }
    }

    fn handle(&mut self, m: &mut Match, from: SocketAddr, msg: ToHost, now: f64) {
        match msg {
            ToHost::Join { protocol, name } => {
                if let Some(p) = self.peers.get_mut(&from) {
                    // Our Welcome was lost: repeat it.
                    p.last_seen = now;
                    send(&self.sock, from, &ToClient::Welcome { id: p.id });
                    return;
                }
                if protocol != PROTOCOL {
                    let reason = format!("version mismatch (host {PROTOCOL}, you {protocol})");
                    send(&self.sock, from, &ToClient::Rejected { reason });
                    return;
                }
                let name: String = name.chars().filter(|c| !c.is_control()).take(16).collect();
                match m.add_player(&name, false) {
                    Some(id) => {
                        self.peers.insert(from, Peer { id, last_seen: now, last_action: 0 });
                        send(&self.sock, from, &ToClient::Welcome { id });
                    }
                    None => send(&self.sock, from, &ToClient::Rejected { reason: "lobby is full".into() }),
                }
            }
            ToHost::Update { pos, yaw, actions } => {
                let Some(peer) = self.peers.get_mut(&from) else { return };
                peer.last_seen = now;
                m.set_pose(peer.id, Vec3::from_array(pos), yaw);
                let mut sorted = actions;
                sorted.sort_by_key(|(seq, _)| *seq);
                for (seq, action) in sorted {
                    if seq > peer.last_action {
                        peer.last_action = seq;
                        m.apply(peer.id, action);
                    }
                }
            }
            ToHost::Leave => {
                if let Some(p) = self.peers.remove(&from) {
                    m.remove_player(p.id);
                }
            }
        }
    }

    /// Send the current state to every peer, with each peer's action ack filled in.
    pub fn broadcast(&self, snap: &Snapshot) {
        let mut snap = snap.clone();
        for p in &mut snap.players {
            if let Some(peer) = self.peers.values().find(|peer| peer.id == p.id) {
                p.last_action = peer.last_action;
            }
        }
        let msg = ToClient::Snapshot(snap);
        for addr in self.peers.keys() {
            send(&self.sock, *addr, &msg);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ClientStatus {
    Connecting,
    Connected(PlayerId),
    Rejected(String),
    /// No snapshot for `TIMEOUT_SECS`.
    Lost,
}

pub struct ClientNet {
    sock: UdpSocket,
    host: SocketAddr,
    name: String,
    pub status: ClientStatus,
    pending: Vec<(u32, Action)>,
    next_seq: u32,
    last_join: f64,
    last_heard: f64,
    pub latest: Option<Snapshot>,
}

impl ClientNet {
    pub fn connect(host: &str, name: &str) -> std::io::Result<Self> {
        let with_port = if host.contains(':') { host.to_string() } else { format!("{host}:{DEFAULT_PORT}") };
        let host = with_port
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| std::io::Error::new(ErrorKind::NotFound, "could not resolve host"))?;
        let local: [u8; 4] = if host.ip().is_loopback() { [127, 0, 0, 1] } else { [0, 0, 0, 0] };
        let sock = UdpSocket::bind(SocketAddr::from((local, 0)))?;
        sock.set_nonblocking(true)?;
        Ok(ClientNet {
            sock,
            host,
            name: name.to_string(),
            status: ClientStatus::Connecting,
            pending: Vec::new(),
            next_seq: 1,
            last_join: f64::NEG_INFINITY,
            last_heard: 0.0,
            latest: None,
        })
    }

    pub fn host_addr(&self) -> SocketAddr {
        self.host
    }

    pub fn id(&self) -> Option<PlayerId> {
        match self.status {
            ClientStatus::Connected(id) => Some(id),
            _ => None,
        }
    }

    /// Queue an action; it is resent every update until the host acks it.
    pub fn push_action(&mut self, a: Action) {
        self.pending.push((self.next_seq, a));
        self.next_seq += 1;
    }

    /// Receive packets. Returns true if a new snapshot arrived.
    pub fn receive(&mut self, now: f64) -> bool {
        let mut buf = [0u8; MAX_PACKET];
        let mut fresh = false;
        loop {
            let (n, from) = match self.sock.recv_from(&mut buf) {
                Ok(x) => x,
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(_) => continue,
            };
            if from != self.host {
                continue;
            }
            let Ok(msg) = bincode::deserialize::<ToClient>(&buf[..n]) else { continue };
            self.last_heard = now;
            match msg {
                ToClient::Welcome { id } => self.status = ClientStatus::Connected(id),
                ToClient::Rejected { reason } => self.status = ClientStatus::Rejected(reason),
                ToClient::Snapshot(s) => {
                    if let Some(me) = self.id().and_then(|id| s.players.iter().find(|p| p.id == id)) {
                        let ack = me.last_action;
                        self.pending.retain(|(seq, _)| *seq > ack);
                    }
                    self.latest = Some(s);
                    fresh = true;
                }
            }
        }
        if matches!(self.status, ClientStatus::Connected(_)) && now - self.last_heard > TIMEOUT_SECS {
            self.status = ClientStatus::Lost;
        }
        fresh
    }

    /// Send our pose (and pending actions), or keep knocking while connecting.
    pub fn send_update(&mut self, pos: Vec3, yaw: f32, now: f64) {
        match self.status {
            ClientStatus::Connecting => {
                if now - self.last_join > 0.5 {
                    self.last_join = now;
                    if self.last_heard == 0.0 {
                        self.last_heard = now;
                    }
                    let msg = ToHost::Join { protocol: PROTOCOL, name: self.name.clone() };
                    send(&self.sock, self.host, &msg);
                }
            }
            ClientStatus::Connected(_) => {
                let msg = ToHost::Update { pos: pos.to_array(), yaw, actions: self.pending.clone() };
                send(&self.sock, self.host, &msg);
            }
            _ => {}
        }
    }

    pub fn leave(&self) {
        send(&self.sock, self.host, &ToHost::Leave);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::Phase;

    fn pump(host: &mut HostNet, m: &mut Match, c: &mut ClientNet, now: &mut f64, pos: Vec3) {
        for _ in 0..50 {
            *now += 0.02;
            c.send_update(pos, 0.0, *now);
            std::thread::sleep(std::time::Duration::from_millis(2));
            host.receive(m, *now);
            m.step(0.02);
            host.broadcast(&m.snapshot());
            std::thread::sleep(std::time::Duration::from_millis(2));
            c.receive(*now);
        }
    }

    fn loopback() -> (HostNet, Match, ClientNet) {
        let host = HostNet::bind_addr(SocketAddr::from(([127, 0, 0, 1], 0))).unwrap();
        let port = host.local_addr().unwrap().port();
        let mut m = Match::new();
        m.add_player("host", false);
        let c = ClientNet::connect(&format!("127.0.0.1:{port}"), "guest").unwrap();
        (host, m, c)
    }

    #[test]
    fn client_joins_and_gets_snapshots() {
        let (mut host, mut m, mut c) = loopback();
        let mut now = 0.0;
        pump(&mut host, &mut m, &mut c, &mut now, Vec3::new(3.0, 0.0, -12.0));
        let id = c.id().expect("connected");
        assert_eq!(m.players.len(), 2);
        let snap = c.latest.as_ref().expect("snapshot");
        assert_eq!(snap.phase, Phase::Lobby);
        let me = snap.players.iter().find(|p| p.id == id).unwrap();
        assert_eq!(me.name, "guest");
        assert_eq!(me.pos, [3.0, 0.0, -12.0], "host uses the client's pose");
    }

    #[test]
    fn actions_are_applied_once_and_acked() {
        let (mut host, mut m, mut c) = loopback();
        let mut now = 0.0;
        // Stand on the ball pad so we pick a ball up.
        let pad = m.balls[0].pad;
        let feet = Vec3::new(pad.x, 0.0, pad.z);
        pump(&mut host, &mut m, &mut c, &mut now, feet);
        let id = c.id().unwrap();
        assert!(m.player(id).unwrap().holding.is_some());
        c.push_action(Action::Throw { aim: [0.0, 0.0, -1.0], charge: 1.0, curve: 0.0, lob: false });
        pump(&mut host, &mut m, &mut c, &mut now, feet);
        assert!(c.pending.is_empty(), "host acked the throw");
        assert_eq!(host.peers.values().next().unwrap().last_action, 1, "applied exactly once");
    }

    #[test]
    fn full_lobby_rejects() {
        let (mut host, mut m, mut c) = loopback();
        for i in 0..3 {
            m.add_player(&format!("bot{i}"), true);
        }
        let mut now = 0.0;
        pump(&mut host, &mut m, &mut c, &mut now, Vec3::ZERO);
        assert!(matches!(c.status, ClientStatus::Rejected(_)), "{:?}", c.status);
    }

    #[test]
    fn leave_removes_player() {
        let (mut host, mut m, mut c) = loopback();
        let mut now = 0.0;
        pump(&mut host, &mut m, &mut c, &mut now, Vec3::ZERO);
        assert_eq!(m.players.len(), 2);
        c.leave();
        std::thread::sleep(std::time::Duration::from_millis(5));
        host.receive(&mut m, now);
        assert_eq!(m.players.len(), 1);
        assert_eq!(host.peer_count(), 0);
    }

    #[test]
    fn silent_peer_times_out() {
        let (mut host, mut m, mut c) = loopback();
        let mut now = 0.0;
        pump(&mut host, &mut m, &mut c, &mut now, Vec3::ZERO);
        host.receive(&mut m, now + TIMEOUT_SECS + 1.0);
        assert_eq!(m.players.len(), 1);
    }
}
