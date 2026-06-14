//! Rank ladder: 8 ranks (G lowest .. S highest) x 10 levels of escalating
//! corpus difficulty, with promotion driven by session history.

pub mod generate;
pub mod ladder;
pub mod promotion;
pub mod recipe;
pub mod status;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize, ValueEnum,
)]
#[clap(rename_all = "UPPER")]
pub enum Rank {
    G,
    F,
    E,
    D,
    C,
    B,
    A,
    S,
}

pub const ALL_RANKS: [Rank; 8] = [
    Rank::G,
    Rank::F,
    Rank::E,
    Rank::D,
    Rank::C,
    Rank::B,
    Rank::A,
    Rank::S,
];

pub const LEVELS_PER_RANK: u32 = 10;

impl Rank {
    pub fn as_str(self) -> &'static str {
        match self {
            Rank::G => "G",
            Rank::F => "F",
            Rank::E => "E",
            Rank::D => "D",
            Rank::C => "C",
            Rank::B => "B",
            Rank::A => "A",
            Rank::S => "S",
        }
    }

    pub fn from_letter(letter: &str) -> Option<Rank> {
        ALL_RANKS
            .into_iter()
            .find(|rank| rank.as_str().eq_ignore_ascii_case(letter))
    }

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn next(self) -> Option<Rank> {
        ALL_RANKS.get(self.index() + 1).copied()
    }
}

/// One rung on the 80-level ladder. `level` is 1-based within the rank.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LevelId {
    pub rank: Rank,
    pub level: u32,
}

impl LevelId {
    pub fn new(rank: Rank, level: u32) -> Self {
        Self {
            rank,
            level: level.clamp(1, LEVELS_PER_RANK),
        }
    }

    pub fn parse(rank: &str, level: u32) -> Option<Self> {
        if !(1..=LEVELS_PER_RANK).contains(&level) {
            return None;
        }
        Rank::from_letter(rank).map(|rank| Self { rank, level })
    }

    /// Map key like "D3" used in the persisted profile.
    pub fn key(self) -> String {
        format!("{}{}", self.rank.as_str(), self.level)
    }

    /// 0..=79 across the whole ladder.
    pub fn global_index(self) -> u32 {
        self.rank.index() as u32 * LEVELS_PER_RANK + (self.level - 1)
    }

    pub fn next(self) -> Option<LevelId> {
        if self.level < LEVELS_PER_RANK {
            Some(LevelId {
                rank: self.rank,
                level: self.level + 1,
            })
        } else {
            self.rank.next().map(|rank| LevelId { rank, level: 1 })
        }
    }
}

/// Persisted rank progress, nested in `settings.json`. The frontier
/// (`current_rank`/`current_level`) is monotonic: it never demotes.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct RankProfile {
    pub current_rank: Rank,
    pub current_level: u32,
    pub selected_rank: Option<Rank>,
    pub selected_level: Option<u32>,
    pub per_level_best_wpm: BTreeMap<String, f64>,
    pub cleared_levels: BTreeSet<String>,
}

impl Default for RankProfile {
    fn default() -> Self {
        Self {
            current_rank: Rank::G,
            current_level: 1,
            selected_rank: None,
            selected_level: None,
            per_level_best_wpm: BTreeMap::new(),
            cleared_levels: BTreeSet::new(),
        }
    }
}

impl RankProfile {
    pub fn frontier(&self) -> LevelId {
        LevelId::new(self.current_rank, self.current_level)
    }

    pub fn is_unlocked(&self, id: LevelId) -> bool {
        id.global_index() <= self.frontier().global_index()
    }

    /// Raises the frontier to `id` if it is ahead; never lowers it.
    pub fn unlock(&mut self, id: LevelId) {
        if id.global_index() > self.frontier().global_index() {
            self.current_rank = id.rank;
            self.current_level = id.level;
        }
    }

    /// Highest unlocked level within `rank` (1 when the rank is locked).
    pub fn default_level_for(&self, rank: Rank) -> u32 {
        match rank.index().cmp(&self.current_rank.index()) {
            std::cmp::Ordering::Less => LEVELS_PER_RANK,
            std::cmp::Ordering::Equal => self.current_level,
            std::cmp::Ordering::Greater => 1,
        }
    }

    pub fn best_wpm(&self, id: LevelId) -> Option<f64> {
        self.per_level_best_wpm.get(&id.key()).copied()
    }

    /// Returns true when the stored best improved.
    pub fn record_best(&mut self, id: LevelId, wpm: f64) -> bool {
        if !wpm.is_finite() || wpm <= 0.0 {
            return false;
        }
        let entry = self.per_level_best_wpm.entry(id.key()).or_insert(0.0);
        if wpm > *entry {
            *entry = wpm;
            true
        } else {
            false
        }
    }

    pub fn cleared(&self, id: LevelId) -> bool {
        self.cleared_levels.contains(&id.key())
    }

    pub fn mark_cleared(&mut self, id: LevelId) {
        self.cleared_levels.insert(id.key());
    }

    pub fn normalize(&mut self) {
        self.current_level = self.current_level.clamp(1, LEVELS_PER_RANK);
        if let Some(level) = self.selected_level {
            self.selected_level = Some(level.clamp(1, LEVELS_PER_RANK));
        }
        self.per_level_best_wpm
            .retain(|_, wpm| wpm.is_finite() && *wpm >= 0.0);
    }
}

/// Difficulty-altering context of the launch, used to decide whether the
/// session can count toward promotion.
#[derive(Clone, Copy, Debug, Default)]
pub struct SessionOverrides {
    pub custom_corpus: bool,
    pub language_override: bool,
    pub punctuation_override: bool,
    pub numbers_override: bool,
    pub length_override: bool,
    pub time_mode: bool,
    pub race: bool,
    pub gameplay_features: bool,
    /// Explicit `-w` count. `None` means the level prescribes its own count,
    /// which always satisfies the word-count gate.
    pub word_count: Option<usize>,
}

/// Resolved rank context for one test session.
#[derive(Clone, Debug)]
pub struct RankSession {
    pub spec: ladder::LevelSpec,
    /// Practicing a not-yet-unlocked level.
    pub preview: bool,
    /// Can this session count toward promotion?
    pub qualifying: bool,
    /// False when an explicit corpus (file/stdin/--language) wins.
    pub use_rank_corpus: bool,
    /// Explicit CLI difficulty flags layered on top of the rank corpus.
    pub cli_punctuation: bool,
    pub cli_numbers: bool,
}

/// Resolves the active rank session. Returns `None` for the legacy
/// (rank-less) path, which keeps default behavior byte-identical.
pub fn resolve_session(
    cli_rank: Option<Rank>,
    cli_level: Option<u32>,
    profile: &RankProfile,
    overrides: &SessionOverrides,
) -> Option<RankSession> {
    let rank = cli_rank.or(profile.selected_rank).or_else(|| {
        // `--level` alone targets the current frontier rank.
        cli_level.map(|_| profile.current_rank)
    })?;

    let level = cli_level
        .or_else(|| {
            if cli_rank.is_some() && cli_rank != profile.selected_rank {
                None
            } else {
                profile.selected_level
            }
        })
        .unwrap_or_else(|| profile.default_level_for(rank))
        .clamp(1, LEVELS_PER_RANK);

    let id = LevelId::new(rank, level);
    let spec = ladder::level_spec(id);
    let preview = !profile.is_unlocked(id);
    let use_rank_corpus = !overrides.custom_corpus && !overrides.language_override;

    let qualifying = use_rank_corpus
        && !preview
        && !overrides.punctuation_override
        && !overrides.numbers_override
        && !overrides.length_override
        && !overrides.time_mode
        && !overrides.race
        && !overrides.gameplay_features
        // None = prescribed count (always qualifies); Some(n) must meet the floor.
        && overrides.word_count.is_none_or(|count| count >= spec.word_count_min);

    Some(RankSession {
        spec,
        preview,
        qualifying,
        use_rank_corpus,
        cli_punctuation: overrides.punctuation_override,
        cli_numbers: overrides.numbers_override,
    })
}

/// One-line welcome-screen summary for the active rank session.
pub fn welcome_line(profile: &RankProfile, session: &RankSession) -> String {
    let id = session.spec.id;
    let best = profile
        .best_wpm(id)
        .map(|wpm| format!("best {wpm:.0} WPM"))
        .unwrap_or_else(|| "no attempts yet".into());
    format!(
        "Rank {} · Level {}  ({}, need {:.0} WPM @ {:.0}%){}",
        id.rank.as_str(),
        id.level,
        best,
        session.spec.wpm_threshold,
        session.spec.accuracy_threshold * 100.0,
        if session.preview { "  [preview]" } else { "" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_id_key_and_global_index_round_trip() {
        let id = LevelId::new(Rank::D, 3);
        assert_eq!(id.key(), "D3");
        assert_eq!(id.global_index(), 32);
        assert_eq!(LevelId::parse("d", 3), Some(id));
        assert_eq!(LevelId::parse("D", 11), None);
        assert_eq!(
            LevelId::new(Rank::D, 10).next(),
            Some(LevelId::new(Rank::C, 1))
        );
        assert_eq!(LevelId::new(Rank::S, 10).next(), None);
    }

    #[test]
    fn profile_frontier_is_monotonic() {
        let mut profile = RankProfile::default();
        profile.unlock(LevelId::new(Rank::E, 4));
        assert_eq!(profile.frontier(), LevelId::new(Rank::E, 4));
        profile.unlock(LevelId::new(Rank::G, 9));
        assert_eq!(profile.frontier(), LevelId::new(Rank::E, 4));
        assert!(profile.is_unlocked(LevelId::new(Rank::F, 10)));
        assert!(!profile.is_unlocked(LevelId::new(Rank::E, 5)));
    }

    #[test]
    fn resolve_session_legacy_path_without_rank() {
        let profile = RankProfile::default();
        let session = resolve_session(None, None, &profile, &SessionOverrides::default());
        assert!(session.is_none());
    }

    #[test]
    fn resolve_session_preview_for_locked_rank() {
        let profile = RankProfile::default();
        let overrides = SessionOverrides {
            word_count: Some(50),
            ..Default::default()
        };
        let session = resolve_session(Some(Rank::S), None, &profile, &overrides).unwrap();
        assert!(session.preview);
        assert!(!session.qualifying);
        assert!(session.use_rank_corpus);
    }

    #[test]
    fn resolve_session_qualifying_on_clean_launch() {
        let profile = RankProfile::default();
        let overrides = SessionOverrides {
            word_count: Some(50),
            ..Default::default()
        };
        let session = resolve_session(Some(Rank::G), None, &profile, &overrides).unwrap();
        assert!(!session.preview);
        assert!(session.qualifying);
        assert_eq!(session.spec.id, LevelId::new(Rank::G, 1));
    }

    #[test]
    fn resolve_session_disqualified_by_overrides() {
        let profile = RankProfile::default();
        for overrides in [
            SessionOverrides {
                word_count: Some(50),
                time_mode: true,
                ..Default::default()
            },
            SessionOverrides {
                word_count: Some(50),
                gameplay_features: true,
                ..Default::default()
            },
            SessionOverrides {
                word_count: Some(10),
                ..Default::default()
            },
            SessionOverrides {
                word_count: Some(50),
                punctuation_override: true,
                ..Default::default()
            },
        ] {
            let session = resolve_session(Some(Rank::G), None, &profile, &overrides).unwrap();
            assert!(!session.qualifying);
        }
    }

    #[test]
    fn resolve_session_custom_corpus_wins_over_rank() {
        let profile = RankProfile::default();
        let overrides = SessionOverrides {
            word_count: Some(50),
            custom_corpus: true,
            ..Default::default()
        };
        let session = resolve_session(Some(Rank::E), None, &profile, &overrides).unwrap();
        assert!(!session.use_rank_corpus);
        assert!(!session.qualifying);
    }
}
