//! Skill-based matchmaking using win percentage and K/D ratio.
use crate::ranked::MAX_LADDER_INDEX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RankedMode {
    FourVFour,
    FiveVFive,
    SixVSix,
}

impl RankedMode {
    pub fn team_size(self) -> usize {
        match self {
            RankedMode::FourVFour => 4,
            RankedMode::FiveVFive => 5,
            RankedMode::SixVSix => 6,
        }
    }
    pub fn players_needed(self) -> usize {
        self.team_size() * 2
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerStats {
    pub id: u64,
    pub wins: u32,
    pub losses: u32,
    pub kills: u32,
    pub deaths: u32,
    pub ladder_index: u8,
}

impl PlayerStats {
    pub fn games(&self) -> u32 {
        self.wins + self.losses
    }
    /// Win rate with Bayesian smoothing toward 50% (prior = 10 games).
    pub fn win_rate(&self) -> f32 {
        (self.wins as f32 + 5.0) / (self.games() as f32 + 10.0)
    }
    /// K/D smoothed toward 1.0 (prior = 10 kills / 10 deaths).
    pub fn kd(&self) -> f32 {
        (self.kills as f32 + 10.0) / (self.deaths as f32 + 10.0)
    }
    /// Combined skill score in 0..1 (50% win rate, 50% squashed K/D).
    pub fn skill_score(&self) -> f32 {
        let kd_norm = self.kd() / (self.kd() + 1.0);
        0.5 * self.win_rate() + 0.5 * kd_norm
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Ticket {
    pub stats: PlayerStats,
    pub enqueued_at: f32,
}

#[derive(Clone, Debug)]
pub struct Match {
    pub mode: RankedMode,
    pub team_a: Vec<PlayerStats>,
    pub team_b: Vec<PlayerStats>,
}

pub const BASE_SCORE_WINDOW: f32 = 0.05;
pub const SCORE_WINDOW_GROWTH_PER_SEC: f32 = 0.002;
pub const MAX_SCORE_WINDOW: f32 = 1.0; // never starve: eventually match anyone
pub const BASE_LADDER_WINDOW: u8 = 3;
pub const LADDER_GROWTH_SECS: f32 = 20.0;

fn score_window(wait: f32) -> f32 {
    (BASE_SCORE_WINDOW + SCORE_WINDOW_GROWTH_PER_SEC * wait.max(0.0)).min(MAX_SCORE_WINDOW)
}
fn ladder_window(wait: f32) -> u8 {
    let extra = (wait.max(0.0) / LADDER_GROWTH_SECS) as u32;
    (BASE_LADDER_WINDOW as u32 + extra).min(MAX_LADDER_INDEX as u32) as u8
}

#[derive(Default)]
pub struct Queue {
    pub tickets: Vec<Ticket>,
}

impl Queue {
    pub fn enqueue(&mut self, stats: PlayerStats, now: f32) {
        if self.tickets.iter().any(|t| t.stats.id == stats.id) {
            return;
        }
        self.tickets.push(Ticket { stats, enqueued_at: now });
    }
    pub fn dequeue(&mut self, id: u64) {
        self.tickets.retain(|t| t.stats.id != id);
    }

    /// Try to form one match; on success the matched tickets are removed.
    pub fn try_form_match(&mut self, mode: RankedMode, now: f32) -> Option<Match> {
        let n = mode.players_needed();
        if self.tickets.len() < n {
            return None;
        }
        let mut sorted: Vec<Ticket> = self.tickets.clone();
        sorted.sort_by(|a, b| {
            a.stats
                .skill_score()
                .partial_cmp(&b.stats.skill_score())
                .unwrap()
                .then(a.stats.id.cmp(&b.stats.id))
        });

        for start in 0..=(sorted.len() - n) {
            let group = &sorted[start..start + n];
            // Longest waiter dictates how loose the window is.
            let longest_wait = group
                .iter()
                .map(|t| now - t.enqueued_at)
                .fold(0.0_f32, f32::max);
            let spread = group.last().unwrap().stats.skill_score()
                - group.first().unwrap().stats.skill_score();
            let (lmin, lmax) = group.iter().fold((u8::MAX, 0u8), |(lo, hi), t| {
                (lo.min(t.stats.ladder_index), hi.max(t.stats.ladder_index))
            });
            if spread <= score_window(longest_wait) && (lmax - lmin) <= ladder_window(longest_wait) {
                let players: Vec<PlayerStats> = group.iter().map(|t| t.stats).collect();
                let (a, b) = balance_teams(&players);
                let ids: Vec<u64> = players.iter().map(|p| p.id).collect();
                self.tickets.retain(|t| !ids.contains(&t.stats.id));
                return Some(Match { mode, team_a: a, team_b: b });
            }
        }
        None
    }
}

/// Snake-draft by skill (A,B,B,A,A,B,...) for evenly balanced teams.
pub fn balance_teams(players: &[PlayerStats]) -> (Vec<PlayerStats>, Vec<PlayerStats>) {
    let mut ps = players.to_vec();
    ps.sort_by(|a, b| {
        b.skill_score()
            .partial_cmp(&a.skill_score())
            .unwrap()
            .then(a.id.cmp(&b.id))
    });
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for (i, p) in ps.into_iter().enumerate() {
        // pattern index within each pair of picks: 0,1 -> A,B  then 2,3 -> B,A
        if matches!(i % 4, 0 | 3) {
            a.push(p);
        } else {
            b.push(p);
        }
    }
    (a, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(id: u64, wins: u32, losses: u32, k: u32, d: u32, idx: u8) -> PlayerStats {
        PlayerStats { id, wins, losses, kills: k, deaths: d, ladder_index: idx }
    }

    #[test]
    fn new_player_is_neutral() {
        let s = p(1, 0, 0, 0, 0, 0);
        assert!((s.win_rate() - 0.5).abs() < 1e-6);
        assert!((s.kd() - 1.0).abs() < 1e-6);
        assert!((s.skill_score() - 0.5).abs() < 1e-6);
    }
    #[test]
    fn better_stats_score_higher() {
        let good = p(1, 40, 10, 800, 400, 5);
        let bad = p(2, 10, 40, 300, 700, 5);
        assert!(good.skill_score() > bad.skill_score());
    }
    #[test]
    fn modes_need_correct_player_counts() {
        assert_eq!(RankedMode::FourVFour.players_needed(), 8);
        assert_eq!(RankedMode::FiveVFive.players_needed(), 10);
        assert_eq!(RankedMode::SixVSix.players_needed(), 12);
    }
    #[test]
    fn forms_match_when_close() {
        let mut q = Queue::default();
        for i in 0..8 {
            q.enqueue(p(i, 10, 10, 100, 100, 3), 0.0);
        }
        let m = q.try_form_match(RankedMode::FourVFour, 0.0).expect("match");
        assert_eq!(m.team_a.len(), 4);
        assert_eq!(m.team_b.len(), 4);
        assert!(q.tickets.is_empty());
    }
    #[test]
    fn not_enough_players() {
        let mut q = Queue::default();
        for i in 0..7 {
            q.enqueue(p(i, 10, 10, 100, 100, 3), 0.0);
        }
        assert!(q.try_form_match(RankedMode::FourVFour, 0.0).is_none());
    }
    #[test]
    fn mismatched_skill_waits_then_relaxes() {
        let mut q = Queue::default();
        for i in 0..4 {
            q.enqueue(p(i, 45, 5, 900, 300, 3), 0.0); // strong
        }
        for i in 4..8 {
            q.enqueue(p(i, 5, 45, 300, 900, 3), 0.0); // weak
        }
        assert!(q.try_form_match(RankedMode::FourVFour, 0.0).is_none());
        assert!(q.try_form_match(RankedMode::FourVFour, 300.0).is_some());
    }
    #[test]
    fn ladder_gap_blocks_match_initially() {
        let mut q = Queue::default();
        for i in 0..4 {
            q.enqueue(p(i, 10, 10, 100, 100, 0), 0.0);
        }
        for i in 4..8 {
            q.enqueue(p(i, 10, 10, 100, 100, 12), 0.0);
        }
        assert!(q.try_form_match(RankedMode::FourVFour, 0.0).is_none());
    }
    #[test]
    fn duplicate_enqueue_ignored() {
        let mut q = Queue::default();
        q.enqueue(p(1, 0, 0, 0, 0, 0), 0.0);
        q.enqueue(p(1, 0, 0, 0, 0, 0), 1.0);
        assert_eq!(q.tickets.len(), 1);
    }
    #[test]
    fn teams_are_balanced() {
        let players: Vec<_> = (0..10)
            .map(|i| p(i, 10 + i as u32 * 2, 20 - i as u32, 100 + i as u32 * 20, 150, 3))
            .collect();
        let (a, b) = balance_teams(&players);
        assert_eq!(a.len(), 5);
        assert_eq!(b.len(), 5);
        let sa: f32 = a.iter().map(|x| x.skill_score()).sum();
        let sb: f32 = b.iter().map(|x| x.skill_score()).sum();
        assert!((sa - sb).abs() < 0.05);
    }
}
