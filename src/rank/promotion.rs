//! Promotion evaluation and commit.
//!
//! Evaluation runs at test end, before the session record is appended: the
//! history tail plus the in-memory current record form the consistency
//! window. Advancement commits only on the user's keypress and the profile
//! frontier never demotes.

use super::ladder::LevelSpec;
use super::{LevelId, RankProfile};
use crate::history::{self, HistoryRecord, TailQuery};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromotionOutcome {
    None,
    LevelCleared {
        cleared: LevelId,
        next: LevelId,
    },
    RankUp {
        cleared: LevelId,
        next: LevelId,
    },
    /// S10 cleared: the end of the ladder.
    Mastery {
        cleared: LevelId,
    },
}

impl PromotionOutcome {
    pub fn advances(&self) -> bool {
        !matches!(self, PromotionOutcome::None)
    }
}

#[derive(Clone, Debug)]
pub struct PromotionBanner {
    pub outcome: PromotionOutcome,
    /// Action line, empty when there is nothing to commit.
    pub message: String,
    /// Always-present rank progress summary for the results screen.
    pub progress_line: String,
}

fn meets(spec: &LevelSpec, wpm: f64, accuracy: f64) -> bool {
    wpm >= spec.wpm_threshold && accuracy >= spec.accuracy_threshold
}

/// (adjusted WPM, accuracy, phoenix deaths immediately before this run).
struct SessionOutcome {
    wpm: f64,
    accuracy: f64,
    deaths_before: u32,
}

fn qualifying_metrics(record: &HistoryRecord) -> Option<SessionOutcome> {
    (record.qualifying && record.completed && record.corpus.kind == "rank").then_some(
        SessionOutcome {
            wpm: record.adjusted_wpm,
            accuracy: record.accuracy,
            deaths_before: record.deaths_before,
        },
    )
}

/// Most-recent qualifying sessions (history tail plus optionally the current
/// record), chronological. Unreadable history counts as zero sessions.
fn recent_sessions(
    spec: &LevelSpec,
    history_path: &Path,
    current: Option<&HistoryRecord>,
) -> Vec<SessionOutcome> {
    let query = TailQuery {
        limit: 0,
        rank: Some(spec.id.rank.as_str().to_string()),
        level: Some(spec.id.level),
    };
    let mut sessions: Vec<SessionOutcome> = history::read_tail(history_path, &query)
        .unwrap_or_default()
        .iter()
        .filter_map(qualifying_metrics)
        .collect();
    if let Some(metrics) = current.and_then(qualifying_metrics) {
        sessions.push(metrics);
    }
    sessions
}

/// Trailing streak of sessions meeting the thresholds, walking newest-first.
/// A Phoenix run that burned before surviving (`deaths_before > 0`) caps the
/// streak there: the chain back to the previous survival is broken, so
/// "consecutive" means clean survivals with no deaths between them.
pub fn recent_streak(
    spec: &LevelSpec,
    history_path: &Path,
    current: Option<&HistoryRecord>,
) -> usize {
    let sessions = recent_sessions(spec, history_path, current);
    let mut streak = 0usize;
    for outcome in sessions.iter().rev() {
        if !meets(spec, outcome.wpm, outcome.accuracy) {
            break;
        }
        streak += 1;
        if streak >= spec.consistency_n as usize {
            break;
        }
        // Deaths preceding this survival break the chain to the older one.
        if outcome.deaths_before > 0 {
            break;
        }
    }
    streak.min(spec.consistency_n as usize)
}

/// Evaluates the just-finished session. Returns the outcome and the current
/// qualifying streak (for the progress line).
pub fn evaluate(
    profile: &RankProfile,
    spec: &LevelSpec,
    history_path: &Path,
    current: &HistoryRecord,
) -> (PromotionOutcome, usize) {
    let streak = recent_streak(spec, history_path, Some(current));

    if profile.cleared(spec.id) {
        return (PromotionOutcome::None, streak);
    }
    let Some(outcome) = qualifying_metrics(current) else {
        return (PromotionOutcome::None, streak);
    };
    if !meets(spec, outcome.wpm, outcome.accuracy) || streak < spec.consistency_n as usize {
        return (PromotionOutcome::None, streak);
    }

    let outcome = match spec.id.next() {
        Some(next) if next.rank == spec.id.rank => PromotionOutcome::LevelCleared {
            cleared: spec.id,
            next,
        },
        Some(next) => PromotionOutcome::RankUp {
            cleared: spec.id,
            next,
        },
        None => PromotionOutcome::Mastery { cleared: spec.id },
    };
    (outcome, streak)
}

/// Commits a suggested advancement (the `N` keypress). Marks the level
/// cleared, raises the frontier, and re-targets the selection at the next
/// level. Monotonic: never demotes.
pub fn commit_advance(profile: &mut RankProfile, outcome: &PromotionOutcome) {
    let (cleared, next) = match outcome {
        PromotionOutcome::None => return,
        PromotionOutcome::LevelCleared { cleared, next }
        | PromotionOutcome::RankUp { cleared, next } => (*cleared, Some(*next)),
        PromotionOutcome::Mastery { cleared } => (*cleared, None),
    };
    profile.mark_cleared(cleared);
    if let Some(next) = next {
        profile.unlock(next);
        profile.selected_rank = Some(next.rank);
        profile.selected_level = Some(next.level);
    }
}

pub fn banner(
    spec: &LevelSpec,
    outcome: PromotionOutcome,
    current: &HistoryRecord,
    streak: usize,
) -> PromotionBanner {
    let progress_line = format!(
        "Rank {} · Level {} — {:.1} WPM / {:.1}% — need {:.0} WPM @ {:.0}% ({}/{} qualifying)",
        spec.id.rank.as_str(),
        spec.id.level,
        current.adjusted_wpm,
        current.accuracy * 100.0,
        spec.wpm_threshold,
        spec.accuracy_threshold * 100.0,
        streak,
        spec.consistency_n,
    );

    let message = match &outcome {
        PromotionOutcome::None => {
            if current.qualifying {
                String::new()
            } else {
                "Non-qualifying session (preview, overrides, or incomplete run)".into()
            }
        }
        PromotionOutcome::LevelCleared { next, .. } => {
            format!("Level cleared! Press N to advance to Level {}", next.level)
        }
        PromotionOutcome::RankUp { next, .. } => {
            format!("Rank up! Press N to advance to Rank {}", next.rank.as_str())
        }
        PromotionOutcome::Mastery { .. } => {
            "S10 cleared — true typing Mastery. Press N to record it.".into()
        }
    };

    PromotionBanner {
        outcome,
        message,
        progress_line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::{append_record, Corpus, HistoryRecord};
    use crate::rank::ladder::level_spec;
    use crate::rank::Rank;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn record(rank: &str, level: u32, wpm: f64, accuracy: f64, qualifying: bool) -> HistoryRecord {
        HistoryRecord {
            schema_version: 1,
            session_id: "test".into(),
            started_at_unix_ms: 0,
            ended_at_unix_ms: 0,
            utc_offset_minutes: 0,
            local_hour: 0,
            ttyper_version: "test".into(),
            mode: "words".into(),
            time_limit_secs: None,
            word_count_requested: Some(50),
            corpus: Corpus {
                kind: "rank".into(),
                name: Some(format!("{rank}{level}")),
                language: None,
            },
            rank: Some(rank.into()),
            level: Some(level),
            qualifying,
            promotion_event: None,
            raw_wpm: wpm,
            adjusted_wpm: wpm,
            accuracy,
            mistakes: 0,
            correct_words: 50,
            total_words_typed: 50,
            completed: true,
            end_reason: None,
            gameplay_features: Vec::new(),
            chaos_modes: Vec::new(),
            gameplay_multiplier: 1.0,
            phoenix: false,
            deaths_before: 0,
            keystrokes: Vec::new(),
            per_key_accuracy: HashMap::new(),
            per_key_mean_ms: HashMap::new(),
            keystrokes_truncated: false,
        }
    }

    fn temp_history() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        (dir, path)
    }

    #[test]
    fn g_level_clears_on_single_qualifying_session() {
        let (_dir, path) = temp_history();
        let profile = RankProfile::default();
        let spec = level_spec(LevelId::new(Rank::G, 1));
        let current = record("G", 1, 30.0, 0.95, true);

        let (outcome, streak) = evaluate(&profile, &spec, &path, &current);
        assert_eq!(streak, 1);
        assert_eq!(
            outcome,
            PromotionOutcome::LevelCleared {
                cleared: LevelId::new(Rank::G, 1),
                next: LevelId::new(Rank::G, 2),
            }
        );
    }

    #[test]
    fn consistency_requires_most_recent_n_sessions() {
        let (_dir, path) = temp_history();
        let mut profile = RankProfile::default();
        profile.unlock(LevelId::new(Rank::E, 1));
        let spec = level_spec(LevelId::new(Rank::E, 1));

        // One qualifying pass in history, then a failed one resets the streak.
        append_record(&path, &record("E", 1, 60.0, 0.99, true)).unwrap();
        append_record(&path, &record("E", 1, 10.0, 0.50, true)).unwrap();
        let current = record("E", 1, 60.0, 0.99, true);

        let (outcome, streak) = evaluate(&profile, &spec, &path, &current);
        assert_eq!(streak, 1, "failed middle session breaks the streak");
        assert_eq!(outcome, PromotionOutcome::None);

        // A second consecutive pass clears it (E needs 2).
        append_record(&path, &record("E", 1, 60.0, 0.99, true)).unwrap();
        let (outcome, streak) = evaluate(&profile, &spec, &path, &current);
        assert_eq!(streak, 2);
        assert!(outcome.advances());
    }

    #[test]
    fn phoenix_deaths_break_the_consecutive_streak() {
        let (_dir, path) = temp_history();
        let mut profile = RankProfile::default();
        profile.unlock(LevelId::new(Rank::E, 1));
        let spec = level_spec(LevelId::new(Rank::E, 1));

        // A clean first survival, then a survival that burned twice before
        // succeeding. E needs 2 consecutive with no deaths between → no clear.
        append_record(&path, &record("E", 1, 60.0, 1.0, true)).unwrap();
        let mut burned = record("E", 1, 60.0, 1.0, true);
        burned.deaths_before = 2;

        let (outcome, streak) = evaluate(&profile, &spec, &path, &burned);
        assert_eq!(streak, 1, "deaths before the run break the chain back");
        assert_eq!(outcome, PromotionOutcome::None);

        // A first-try survival (no deaths) right after a clean run clears it.
        append_record(&path, &record("E", 1, 60.0, 1.0, true)).unwrap();
        let first_try = record("E", 1, 60.0, 1.0, true); // deaths_before = 0
        let (outcome, streak) = evaluate(&profile, &spec, &path, &first_try);
        assert_eq!(streak, 2);
        assert!(outcome.advances());
    }

    #[test]
    fn non_qualifying_sessions_never_promote() {
        let (_dir, path) = temp_history();
        let profile = RankProfile::default();
        let spec = level_spec(LevelId::new(Rank::G, 1));
        let current = record("G", 1, 120.0, 1.0, false);

        let (outcome, _) = evaluate(&profile, &spec, &path, &current);
        assert_eq!(outcome, PromotionOutcome::None);
    }

    #[test]
    fn cleared_level_yields_no_banner_action() {
        let (_dir, path) = temp_history();
        let mut profile = RankProfile::default();
        profile.mark_cleared(LevelId::new(Rank::G, 1));
        let spec = level_spec(LevelId::new(Rank::G, 1));
        let current = record("G", 1, 120.0, 1.0, true);

        let (outcome, _) = evaluate(&profile, &spec, &path, &current);
        assert_eq!(outcome, PromotionOutcome::None);
    }

    #[test]
    fn rank_boundary_promotes_to_next_rank() {
        let (_dir, path) = temp_history();
        let mut profile = RankProfile::default();
        profile.unlock(LevelId::new(Rank::G, 10));
        let spec = level_spec(LevelId::new(Rank::G, 10));
        let current = record("G", 10, 60.0, 0.99, true);

        let (outcome, _) = evaluate(&profile, &spec, &path, &current);
        assert_eq!(
            outcome,
            PromotionOutcome::RankUp {
                cleared: LevelId::new(Rank::G, 10),
                next: LevelId::new(Rank::F, 1),
            }
        );
    }

    #[test]
    fn commit_advance_raises_frontier_and_selection() {
        let mut profile = RankProfile::default();
        let outcome = PromotionOutcome::LevelCleared {
            cleared: LevelId::new(Rank::G, 1),
            next: LevelId::new(Rank::G, 2),
        };
        commit_advance(&mut profile, &outcome);
        assert!(profile.cleared(LevelId::new(Rank::G, 1)));
        assert_eq!(profile.frontier(), LevelId::new(Rank::G, 2));
        assert_eq!(profile.selected_rank, Some(Rank::G));
        assert_eq!(profile.selected_level, Some(2));

        // Replaying a lower level never demotes.
        commit_advance(
            &mut profile,
            &PromotionOutcome::LevelCleared {
                cleared: LevelId::new(Rank::G, 1),
                next: LevelId::new(Rank::G, 2),
            },
        );
        assert_eq!(profile.frontier(), LevelId::new(Rank::G, 2));
    }

    #[test]
    fn unreadable_history_is_conservative() {
        let profile = RankProfile::default();
        let spec = level_spec(LevelId::new(Rank::E, 1));
        let current = record("E", 1, 60.0, 0.99, true);
        // Nonexistent directory: read fails, zero prior sessions; E needs 2.
        let path = PathBuf::from("/nonexistent/dir/history.jsonl");
        let (outcome, streak) = evaluate(&profile, &spec, &path, &current);
        assert_eq!(streak, 1);
        assert_eq!(outcome, PromotionOutcome::None);
    }
}
