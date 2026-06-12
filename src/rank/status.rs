//! Plain-text renderer for `--rank-status`.

use super::ladder::level_spec;
use super::{promotion, LevelId, RankProfile, RankSession, ALL_RANKS, LEVELS_PER_RANK};
use std::fmt::Write;
use std::path::Path;

pub fn render(profile: &RankProfile, session: Option<&RankSession>, history_path: &Path) -> String {
    let frontier = profile.frontier();
    let target = session.map(|session| session.spec.id).unwrap_or(frontier);
    let spec = level_spec(target);
    let streak = promotion::recent_streak(&spec, history_path, None);
    let preview = session.is_some_and(|session| session.preview);

    let mut out = String::new();
    let _ = writeln!(out, "ttyper rank status");
    let _ = writeln!(
        out,
        "  Frontier:  {} · Level {} (of {})",
        frontier.rank.as_str(),
        frontier.level,
        LEVELS_PER_RANK
    );
    let _ = writeln!(
        out,
        "  Cleared:   {} / {} levels",
        profile.cleared_levels.len(),
        ALL_RANKS.len() as u32 * LEVELS_PER_RANK
    );
    let _ = writeln!(
        out,
        "  Target:    {}{}",
        target.key(),
        if preview {
            "  (preview: not yet unlocked)"
        } else {
            ""
        }
    );
    let _ = writeln!(
        out,
        "    threshold   {:.0} WPM @ {:.1}% accuracy",
        spec.wpm_threshold,
        spec.accuracy_threshold * 100.0
    );
    let best = profile
        .best_wpm(target)
        .map(|wpm| format!("{wpm:.1} WPM"))
        .unwrap_or_else(|| "—".into());
    let _ = writeln!(out, "    your best   {best}");
    let _ = writeln!(
        out,
        "    consistency {} / {} recent qualifying sessions meet the threshold",
        streak, spec.consistency_n
    );
    let _ = writeln!(
        out,
        "    status      {}",
        if profile.cleared(target) {
            "cleared"
        } else {
            "not cleared"
        }
    );

    match target.next() {
        Some(next) if next.rank == target.rank => {
            let _ = writeln!(
                out,
                "  Next: {}  ·  Rank up at {} -> {}",
                next.key(),
                LevelId::new(target.rank, LEVELS_PER_RANK).key(),
                target.rank.next().map(|r| r.as_str()).unwrap_or("Mastery")
            );
        }
        Some(next) => {
            let _ = writeln!(out, "  Next: Rank {} · Level 1", next.rank.as_str());
        }
        None => {
            let _ = writeln!(out, "  Next: nothing — S10 is the summit.");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rank::Rank;
    use std::path::PathBuf;

    #[test]
    fn render_default_profile() {
        let profile = RankProfile::default();
        let output = render(&profile, None, &PathBuf::from("/nonexistent/history.jsonl"));
        assert!(output.contains("Frontier:  G · Level 1"));
        assert!(output.contains("Target:    G1"));
        assert!(output.contains("threshold   25 WPM @ 90.0% accuracy"));
        assert!(output.contains("0 / 80 levels"));
    }

    #[test]
    fn render_marks_cleared_levels() {
        let mut profile = RankProfile::default();
        profile.mark_cleared(LevelId::new(Rank::G, 1));
        profile.unlock(LevelId::new(Rank::G, 2));
        profile.record_best(LevelId::new(Rank::G, 1), 41.5);
        let output = render(&profile, None, &PathBuf::from("/nonexistent/history.jsonl"));
        assert!(output.contains("1 / 80 levels"));
        assert!(output.contains("Frontier:  G · Level 2"));
    }
}
