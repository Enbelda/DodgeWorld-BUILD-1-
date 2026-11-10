//! Ranked ladder: Copper -> Diamond, 3 tiers per rank, 120 points per tier.
use std::fmt;

pub const POINTS_PER_TIER: i32 = 120;
pub const TIERS_PER_RANK: u8 = 3;
pub const WIN_POINTS: i32 = 20;
pub const KO_BONUS_LOW: i32 = 5; // 5+ knockouts
pub const KO_BONUS_HIGH: i32 = 10; // 15+ knockouts
pub const KO_THRESHOLD_LOW: u32 = 5;
pub const KO_THRESHOLD_HIGH: u32 = 15;
pub const LOSS_POINTS: i32 = 15;
pub const LOSS_DEATH_PENALTY: i32 = 3; // 10+ deaths in a loss
pub const LOSS_LOW_KO_PENALTY: i32 = 2; // 10+ deaths AND fewest knockouts on team
pub const DEATH_THRESHOLD: u32 = 10;
pub const LOSS_BEST_PERFORMER_POINTS: i32 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rank {
    Copper,
    Bronze,
    Silver,
    Gold,
    Platinum,
    Diamond,
}

impl Rank {
    pub const ALL: [Rank; 6] = [
        Rank::Copper,
        Rank::Bronze,
        Rank::Silver,
        Rank::Gold,
        Rank::Platinum,
        Rank::Diamond,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Rank::Copper => "Copper",
            Rank::Bronze => "Bronze",
            Rank::Silver => "Silver",
            Rank::Gold => "Gold",
            Rank::Platinum => "Platinum",
            Rank::Diamond => "Diamond",
        }
    }
}

/// Highest ladder index (Diamond I).
pub const MAX_LADDER_INDEX: u8 = (Rank::ALL.len() as u8) * TIERS_PER_RANK - 1;

/// One player's stats for a single finished match.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlayerLine {
    pub kills: u32,
    pub assists: u32,
    pub deaths: u32,
}

impl PlayerLine {
    pub fn impact(&self) -> u32 {
        self.kills + self.assists
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointBreakdown {
    pub base: i32,
    pub ko_bonus: i32,
    pub death_penalty: i32,
    pub low_ko_penalty: i32,
    /// Loss softened because this player had the best performance in the match.
    pub best_performer_override: bool,
    pub total: i32,
}

/// Compute the ranked point change for one player.
/// `teammates` excludes `me`; `opponents` is the full enemy team.
pub fn evaluate(
    won: bool,
    me: &PlayerLine,
    teammates: &[PlayerLine],
    opponents: &[PlayerLine],
) -> PointBreakdown {
    let mut b = PointBreakdown::default();
    if won {
        b.base = WIN_POINTS;
        b.ko_bonus = if me.kills >= KO_THRESHOLD_HIGH {
            KO_BONUS_HIGH
        } else if me.kills >= KO_THRESHOLD_LOW {
            KO_BONUS_LOW
        } else {
            0
        };
        b.total = b.base + b.ko_bonus;
        return b;
    }

    // Loss. Best performer (most kills + assists in the whole match, ties count).
    let best = teammates
        .iter()
        .chain(opponents.iter())
        .all(|p| me.impact() >= p.impact());
    if best {
        b.base = -LOSS_BEST_PERFORMER_POINTS;
        b.best_performer_override = true;
        b.total = b.base;
        return b;
    }

    b.base = -LOSS_POINTS;
    if me.deaths >= DEATH_THRESHOLD {
        b.death_penalty = -LOSS_DEATH_PENALTY;
        let fewest_ko = !teammates.is_empty() && teammates.iter().all(|t| me.kills <= t.kills);
        if fewest_ko {
            b.low_ko_penalty = -LOSS_LOW_KO_PENALTY;
        }
    }
    b.total = b.base + b.death_penalty + b.low_ko_penalty;
    b
}

/// A player's position on the ladder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RankState {
    /// 0 = Copper III ... 17 = Diamond I.
    pub ladder_index: u8,
    /// 0..POINTS_PER_TIER (Diamond I may sit at exactly POINTS_PER_TIER).
    pub points: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RankChange {
    None,
    Promoted,
    Demoted,
}

impl RankState {
    pub fn rank(&self) -> Rank {
        Rank::ALL[(self.ladder_index / TIERS_PER_RANK) as usize]
    }
    /// 3 = lowest tier in a rank, 1 = highest (shown as III, II, I).
    pub fn tier_number(&self) -> u8 {
        TIERS_PER_RANK - (self.ladder_index % TIERS_PER_RANK)
    }
    pub fn tier_roman(&self) -> &'static str {
        match self.tier_number() {
            3 => "III",
            2 => "II",
            _ => "I",
        }
    }

    /// Apply a point delta, handling promotion (carry-over) and demotion.
    pub fn apply(&mut self, delta: i32) -> RankChange {
        let start = self.ladder_index;
        self.points += delta;
        while self.points >= POINTS_PER_TIER && self.ladder_index < MAX_LADDER_INDEX {
            self.points -= POINTS_PER_TIER;
            self.ladder_index += 1;
        }
        if self.ladder_index == MAX_LADDER_INDEX && self.points > POINTS_PER_TIER {
            self.points = POINTS_PER_TIER;
        }
        while self.points < 0 {
            if self.ladder_index == 0 {
                self.points = 0;
            } else {
                self.ladder_index -= 1;
                self.points += POINTS_PER_TIER;
            }
        }
        use std::cmp::Ordering::*;
        match self.ladder_index.cmp(&start) {
            Greater => RankChange::Promoted,
            Less => RankChange::Demoted,
            Equal => RankChange::None,
        }
    }
}

impl fmt::Display for RankState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} ({}/{})",
            self.rank().name(),
            self.tier_roman(),
            self.points,
            POINTS_PER_TIER
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pl(k: u32, a: u32, d: u32) -> PlayerLine {
        PlayerLine { kills: k, assists: a, deaths: d }
    }
    // Opponents/teammates that are clearly better than "me" in most tests.
    fn strong() -> Vec<PlayerLine> {
        vec![pl(12, 5, 4), pl(10, 3, 5)]
    }

    #[test]
    fn win_no_bonus() {
        let b = evaluate(true, &pl(4, 0, 3), &strong(), &strong());
        assert_eq!(b.total, 20);
    }
    #[test]
    fn win_five_kos() {
        assert_eq!(evaluate(true, &pl(5, 0, 3), &[], &[]).total, 25);
        assert_eq!(evaluate(true, &pl(14, 0, 3), &[], &[]).total, 25);
    }
    #[test]
    fn win_fifteen_kos() {
        assert_eq!(evaluate(true, &pl(15, 0, 3), &[], &[]).total, 30);
        assert_eq!(evaluate(true, &pl(30, 0, 3), &[], &[]).total, 30);
    }
    #[test]
    fn plain_loss() {
        let b = evaluate(false, &pl(3, 0, 9), &strong(), &strong());
        assert_eq!(b.total, -15);
    }
    #[test]
    fn loss_with_ten_deaths_not_lowest_ko() {
        let mates = vec![pl(1, 0, 8), pl(9, 0, 5)];
        let b = evaluate(false, &pl(4, 0, 10), &mates, &strong());
        assert_eq!(b.total, -18);
    }
    #[test]
    fn loss_ten_deaths_and_lowest_ko_is_minus_twenty() {
        let mates = vec![pl(6, 0, 8), pl(9, 0, 5)];
        let b = evaluate(false, &pl(4, 0, 12), &mates, &strong());
        assert_eq!(b.total, -20);
    }
    #[test]
    fn lowest_ko_without_ten_deaths_no_extra() {
        let mates = vec![pl(6, 0, 8), pl(9, 0, 5)];
        let b = evaluate(false, &pl(1, 0, 9), &mates, &strong());
        assert_eq!(b.total, -15);
    }
    #[test]
    fn loss_best_performer_only_minus_ten() {
        let mates = vec![pl(3, 1, 8)];
        let opps = vec![pl(5, 2, 6)];
        let b = evaluate(false, &pl(14, 4, 12), &mates, &opps);
        assert_eq!(b.total, -10);
        assert!(b.best_performer_override);
    }
    #[test]
    fn promotion_carries_over() {
        let mut r = RankState { ladder_index: 0, points: 110 };
        assert_eq!(r.apply(25), RankChange::Promoted);
        assert_eq!((r.ladder_index, r.points), (1, 15));
        assert_eq!(r.to_string(), "Copper II (15/120)");
    }
    #[test]
    fn rank_graduation_copper_to_bronze() {
        let mut r = RankState { ladder_index: 2, points: 115 };
        r.apply(20);
        assert_eq!(r.rank(), Rank::Bronze);
        assert_eq!(r.tier_roman(), "III");
        assert_eq!(r.points, 15);
    }
    #[test]
    fn demotion_wraps_points() {
        let mut r = RankState { ladder_index: 4, points: 5 };
        assert_eq!(r.apply(-20), RankChange::Demoted);
        assert_eq!((r.ladder_index, r.points), (3, 105));
    }
    #[test]
    fn floor_at_copper_three() {
        let mut r = RankState::default();
        r.apply(-20);
        assert_eq!((r.ladder_index, r.points), (0, 0));
    }
    #[test]
    fn diamond_cap() {
        let mut r = RankState { ladder_index: MAX_LADDER_INDEX, points: 115 };
        r.apply(30);
        assert_eq!(r.points, POINTS_PER_TIER);
        assert_eq!(r.rank(), Rank::Diamond);
        assert_eq!(r.tier_roman(), "I");
    }
    #[test]
    fn multi_promotion_in_one_apply() {
        let mut r = RankState { ladder_index: 0, points: 0 };
        r.apply(250);
        assert_eq!((r.ladder_index, r.points), (2, 10));
    }
}
