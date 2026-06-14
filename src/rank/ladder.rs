//! Promotion thresholds and per-level specs.
//!
//! Within a rank, level N's WPM threshold interpolates from the rank's floor
//! toward the next rank's floor, so level 10 of rank X lands just under
//! level 1 of rank X+1 — a smooth ramp across all 80 levels.

use super::recipe::{recipe_for, CorpusRecipe};
use super::{LevelId, LEVELS_PER_RANK};

/// (adjusted WPM, accuracy fraction) required at level 1 of each rank, G..S.
pub const RANK_FLOORS: [(f64, f64); 8] = [
    (25.0, 0.90),
    (35.0, 0.92),
    (40.0, 0.93),
    (45.0, 0.94),
    (50.0, 0.95),
    (55.0, 0.95),
    (60.0, 0.96),
    (70.0, 0.97),
];

/// Most-recent-N qualifying sessions that must all meet the thresholds, G..S.
pub const CONSISTENCY_N: [u8; 8] = [1, 1, 2, 2, 3, 3, 4, 5];

/// Prescribed word count at level 1 of each rank, G..S. Higher ranks demand
/// sustained concentration: a longer text is harder to keep mistake-free
/// (and brutally so under Phoenix Protocol). Interpolated across levels like
/// the WPM threshold, so length grows smoothly across all 80 rungs.
pub const RANK_WORD_COUNT_FLOORS: [usize; 8] = [25, 35, 50, 65, 85, 110, 140, 180];

#[derive(Clone, Debug)]
pub struct LevelSpec {
    pub id: LevelId,
    pub wpm_threshold: f64,
    pub accuracy_threshold: f64,
    pub consistency_n: u8,
    pub word_count_min: usize,
    pub recipe: CorpusRecipe,
}

pub fn level_spec(id: LevelId) -> LevelSpec {
    let rank_index = id.rank.index();
    let (floor_wpm, floor_accuracy) = RANK_FLOORS[rank_index];
    // S has no successor: extend with +2 WPM and +0.3% accuracy per level.
    let (next_wpm, next_accuracy) = RANK_FLOORS
        .get(rank_index + 1)
        .copied()
        .unwrap_or((floor_wpm + 20.0, floor_accuracy + 0.03));

    let floor_words = RANK_WORD_COUNT_FLOORS[rank_index];
    // S extends with +20 words per level beyond its floor.
    let next_words = RANK_WORD_COUNT_FLOORS
        .get(rank_index + 1)
        .copied()
        .unwrap_or(floor_words + 200);

    let step = (id.level - 1) as f64 / f64::from(LEVELS_PER_RANK);
    let wpm_threshold = floor_wpm + (next_wpm - floor_wpm) * step;
    let accuracy_threshold = (floor_accuracy + (next_accuracy - floor_accuracy) * step).min(0.999);
    let word_count_min = floor_words + ((next_words - floor_words) as f64 * step).round() as usize;

    LevelSpec {
        id,
        wpm_threshold,
        accuracy_threshold,
        consistency_n: CONSISTENCY_N[rank_index],
        word_count_min,
        recipe: recipe_for(id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rank::{Rank, ALL_RANKS};

    #[test]
    fn rank_floors_match_level_one() {
        for (rank, (wpm, accuracy)) in ALL_RANKS.into_iter().zip(RANK_FLOORS) {
            let spec = level_spec(LevelId::new(rank, 1));
            assert_eq!(spec.wpm_threshold, wpm);
            assert_eq!(spec.accuracy_threshold, accuracy);
        }
    }

    #[test]
    fn ramp_is_smooth_across_rank_boundaries() {
        for rank in ALL_RANKS.into_iter().take(7) {
            let top = level_spec(LevelId::new(rank, 10));
            let next_floor = level_spec(LevelId::new(rank.next().unwrap(), 1));
            assert!(
                top.wpm_threshold < next_floor.wpm_threshold,
                "{:?} level 10 should sit just below the next rank floor",
                rank
            );
            assert!(next_floor.wpm_threshold - top.wpm_threshold <= 2.0 + f64::EPSILON);
        }
    }

    #[test]
    fn thresholds_increase_monotonically_within_rank() {
        for rank in ALL_RANKS {
            for level in 1..10 {
                let lower = level_spec(LevelId::new(rank, level));
                let higher = level_spec(LevelId::new(rank, level + 1));
                assert!(higher.wpm_threshold > lower.wpm_threshold);
                assert!(higher.accuracy_threshold >= lower.accuracy_threshold);
            }
        }
    }

    #[test]
    fn word_count_grows_with_rank() {
        let g1 = level_spec(LevelId::new(Rank::G, 1));
        let s1 = level_spec(LevelId::new(Rank::S, 1));
        assert_eq!(g1.word_count_min, 25);
        assert_eq!(s1.word_count_min, 180);
        // Monotonic non-decreasing across the whole 80-rung ladder.
        let mut previous = 0;
        for rank in ALL_RANKS {
            for level in 1..=10 {
                let count = level_spec(LevelId::new(rank, level)).word_count_min;
                assert!(count >= previous, "{:?} L{level} dropped word count", rank);
                previous = count;
            }
        }
    }

    #[test]
    fn s_rank_caps_accuracy_below_one() {
        let spec = level_spec(LevelId::new(Rank::S, 10));
        assert!(spec.accuracy_threshold <= 0.999);
        assert_eq!(spec.consistency_n, 5);
    }
}
