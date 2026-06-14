//! Append-only typing session history (`history.jsonl`).
//!
//! One JSON object per line per finished test. Records carry the full raw
//! keystroke stream so any future analysis (bigram latency, per-key trends,
//! consistency variance) stays possible. Writes are best-effort and must
//! never disturb the typing flow.

use crate::test::results::Results;
use crate::test::Test;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub const SCHEMA_VERSION: u32 = 1;
pub const KEYSTROKE_CAP: usize = 50_000;
pub const TAIL_READ_LIMIT: usize = 200;
const TAIL_CHUNK_BYTES: u64 = 64 * 1024;
const TAIL_MAX_SCAN_BYTES: u64 = 4 * 1024 * 1024;

pub const MOD_SHIFT: u8 = 1 << 0;
pub const MOD_CTRL: u8 = 1 << 1;
pub const MOD_ALT: u8 = 1 << 2;
pub const MOD_SUPER: u8 = 1 << 3;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryRecord {
    pub schema_version: u32,
    pub session_id: String,
    pub started_at_unix_ms: u64,
    pub ended_at_unix_ms: u64,
    /// Local UTC offset; 0 in schema v1 (timestamps are UTC-based).
    pub utc_offset_minutes: i32,
    pub local_hour: u8,
    pub ttyper_version: String,
    /// "words" | "time" | "race"
    pub mode: String,
    pub time_limit_secs: Option<u64>,
    pub word_count_requested: Option<u32>,
    pub corpus: Corpus,
    pub rank: Option<String>,
    pub level: Option<u32>,
    #[serde(default)]
    pub qualifying: bool,
    pub promotion_event: Option<String>,
    pub raw_wpm: f64,
    pub adjusted_wpm: f64,
    pub accuracy: f64,
    pub mistakes: u32,
    pub correct_words: u32,
    pub total_words_typed: u32,
    pub completed: bool,
    pub end_reason: Option<String>,
    pub gameplay_features: Vec<String>,
    pub chaos_modes: Vec<String>,
    pub gameplay_multiplier: f64,
    /// Phoenix Protocol active: every mistake burned the corpus and respawned
    /// a fresh one; this record is the surviving (mistake-free) run.
    #[serde(default)]
    pub phoenix: bool,
    /// Phoenix deaths preceding this surviving run since the last record.
    /// Non-zero means the consistency streak was broken just before this run.
    #[serde(default)]
    pub deaths_before: u32,
    #[serde(default)]
    pub keystrokes: Vec<Keystroke>,
    pub per_key_accuracy: HashMap<String, [u32; 2]>,
    pub per_key_mean_ms: HashMap<String, f64>,
    pub keystrokes_truncated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Corpus {
    /// "language" | "file" | "stdin" | "rank" | "race_synced"
    pub kind: String,
    pub name: Option<String>,
    pub language: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Keystroke {
    /// Milliseconds since the first keystroke of the test.
    pub t_ms: u32,
    pub word_index: u32,
    pub key: String,
    pub mods: u8,
    /// 1 = correct, 0 = incorrect, -1 = neutral (no correctness meaning).
    pub correct: i8,
}

/// Test-start context carried to the end-of-test record build.
#[derive(Clone, Debug)]
pub struct SessionMeta {
    pub mode: String,
    pub time_limit_secs: Option<u64>,
    pub word_count_requested: Option<u32>,
    pub corpus: Corpus,
    pub rank: Option<String>,
    pub level: Option<u32>,
    pub qualifying: bool,
    pub phoenix: bool,
    pub chaos_modes: Vec<String>,
}

/// One wall-clock/monotonic pair captured at record build; all event
/// timestamps are back-computed from monotonic deltas so mid-session
/// clock jumps cannot corrupt them.
#[derive(Clone, Copy)]
struct ClockAnchor {
    system: SystemTime,
    instant: Instant,
}

impl ClockAnchor {
    fn now() -> Self {
        Self {
            system: SystemTime::now(),
            instant: Instant::now(),
        }
    }

    fn unix_ms_at(&self, when: Instant) -> u64 {
        let base = self
            .system
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        if when >= self.instant {
            base.saturating_add(when.duration_since(self.instant).as_millis() as u64)
        } else {
            base.saturating_sub(self.instant.duration_since(when).as_millis() as u64)
        }
    }
}

pub fn history_path(config_dir: &Path) -> PathBuf {
    config_dir.join("history.jsonl")
}

/// Builds a record from a finished test. Returns `None` when the test saw
/// no keystrokes (an empty abort is not a meaningful session).
pub fn build_record(
    test: &Test,
    results: &Results,
    meta: &SessionMeta,
    end_reason_hint: Option<&str>,
) -> Option<HistoryRecord> {
    let events: Vec<(u32, &crate::test::TestEvent)> = test
        .words
        .iter()
        .enumerate()
        .flat_map(|(index, word)| word.events.iter().map(move |e| (index as u32, e)))
        .collect();
    let (_, first) = events.first()?;
    let first_instant = first.time;

    let anchor = ClockAnchor::now();
    let started_at_unix_ms = anchor.unix_ms_at(first_instant);
    let ended_at_unix_ms = anchor.unix_ms_at(anchor.instant);
    let local_hour = ((started_at_unix_ms / 3_600_000) % 24) as u8;

    let truncated = events.len() > KEYSTROKE_CAP;
    let keystrokes: Vec<Keystroke> = if truncated {
        Vec::new()
    } else {
        events
            .iter()
            .map(|(word_index, event)| {
                let (key, mods) = normalize_key(&event.key);
                Keystroke {
                    t_ms: event
                        .time
                        .saturating_duration_since(first_instant)
                        .as_millis()
                        .min(u128::from(u32::MAX)) as u32,
                    word_index: *word_index,
                    key,
                    mods,
                    correct: match event.correct {
                        Some(true) => 1,
                        Some(false) => 0,
                        None => -1,
                    },
                }
            })
            .collect()
    };

    // Aggregates are always computed (even when keystrokes are truncated) so
    // per-key stats survive marathon sessions.
    let mut per_key_accuracy: HashMap<String, [u32; 2]> = HashMap::new();
    let mut key_timing: HashMap<String, (f64, u32)> = HashMap::new();
    for (_, event) in &events {
        if let Some(correct) = event.correct {
            let (key, _) = normalize_key(&event.key);
            let entry = per_key_accuracy.entry(key).or_insert([0, 0]);
            entry[1] += 1;
            if correct {
                entry[0] += 1;
            }
        }
    }
    for window in events.windows(2) {
        let (_, previous) = window[0];
        let (_, current) = window[1];
        if let Some(duration) = current.time.checked_duration_since(previous.time) {
            let (key, _) = normalize_key(&current.key);
            let entry = key_timing.entry(key).or_insert((0.0, 0));
            entry.0 += duration.as_secs_f64() * 1000.0;
            entry.1 += 1;
        }
    }
    let per_key_mean_ms = key_timing
        .into_iter()
        .map(|(key, (total, count))| (key, total / f64::from(count)))
        .collect();

    let end_reason = test
        .gameplay
        .end_reason
        .clone()
        .or_else(|| end_reason_hint.map(String::from));

    Some(HistoryRecord {
        schema_version: SCHEMA_VERSION,
        session_id: session_id(),
        started_at_unix_ms,
        ended_at_unix_ms,
        utc_offset_minutes: 0,
        local_hour,
        ttyper_version: env!("CARGO_PKG_VERSION").to_string(),
        mode: meta.mode.clone(),
        time_limit_secs: meta.time_limit_secs,
        word_count_requested: meta.word_count_requested,
        corpus: meta.corpus.clone(),
        rank: meta.rank.clone(),
        level: meta.level,
        qualifying: meta.qualifying && test.complete,
        promotion_event: None,
        raw_wpm: results.timing.overall_cps * 12.0,
        adjusted_wpm: results.adjusted_wpm(),
        accuracy: f64::from(results.accuracy.overall),
        mistakes: test.gameplay.mistakes as u32,
        correct_words: test.gameplay.correct_words as u32,
        total_words_typed: test.completed_word_count() as u32,
        completed: test.complete,
        end_reason,
        gameplay_features: feature_names(test),
        chaos_modes: meta.chaos_modes.clone(),
        gameplay_multiplier: test.gameplay_multiplier(),
        phoenix: meta.phoenix,
        deaths_before: test.phoenix_deaths,
        keystrokes,
        per_key_accuracy,
        per_key_mean_ms,
        keystrokes_truncated: truncated,
    })
}

/// Appends one record as a single JSONL line, creating the file and parent
/// directories when missing.
pub fn append_record(path: &Path, record: &HistoryRecord) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut line = serde_json::to_vec(record)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    line.push(b'\n');
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(&line)
}

/// Best-effort append; failures surface only on stderr when `debug` is set.
pub fn append_record_best_effort(path: &Path, record: &HistoryRecord, debug: bool) {
    if let Err(error) = append_record(path, record) {
        if debug {
            eprintln!("[ttyper] history write failed: {error}");
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct TailQuery {
    /// 0 means the default limit; capped at [`TAIL_READ_LIMIT`].
    pub limit: usize,
    pub rank: Option<String>,
    pub level: Option<u32>,
}

/// Reads up to `limit` most-recent records (chronological order, oldest
/// first) by scanning backward from EOF. Corrupt or torn lines are skipped.
/// Never loads the whole file.
pub fn read_tail(path: &Path, query: &TailQuery) -> io::Result<Vec<HistoryRecord>> {
    let limit = match query.limit {
        0 => TAIL_READ_LIMIT,
        n => n.min(TAIL_READ_LIMIT),
    };
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let file_len = file.metadata()?.len();

    let mut pos = file_len;
    let mut carry: Vec<u8> = Vec::new();
    let mut lines_rev: Vec<Vec<u8>> = Vec::new();
    let mut scanned: u64 = 0;
    let raw_line_headroom = limit.saturating_mul(4);

    while pos > 0 && scanned < TAIL_MAX_SCAN_BYTES && lines_rev.len() < raw_line_headroom {
        let read_size = TAIL_CHUNK_BYTES.min(pos);
        pos -= read_size;
        scanned += read_size;
        file.seek(SeekFrom::Start(pos))?;
        let mut combined = vec![0u8; read_size as usize];
        file.read_exact(&mut combined)?;
        combined.extend_from_slice(&carry);

        match combined.iter().position(|&byte| byte == b'\n') {
            None => {
                carry = combined;
            }
            Some(first_newline) => {
                let mut segments: Vec<Vec<u8>> = combined[first_newline + 1..]
                    .split(|&byte| byte == b'\n')
                    .map(<[u8]>::to_vec)
                    .collect();
                segments.reverse();
                for segment in segments {
                    if !segment.is_empty() && lines_rev.len() < raw_line_headroom {
                        lines_rev.push(segment);
                    }
                }
                carry = combined[..first_newline].to_vec();
            }
        }
    }
    if pos == 0 && !carry.is_empty() && lines_rev.len() < raw_line_headroom {
        lines_rev.push(carry);
    }

    let mut records: Vec<HistoryRecord> = Vec::with_capacity(limit);
    for line in lines_rev {
        if records.len() >= limit {
            break;
        }
        let Ok(record) = serde_json::from_slice::<HistoryRecord>(&line) else {
            continue;
        };
        if let Some(rank) = &query.rank {
            if record.rank.as_deref() != Some(rank.as_str()) {
                continue;
            }
        }
        if let Some(level) = query.level {
            if record.level != Some(level) {
                continue;
            }
        }
        records.push(record);
    }
    records.reverse();
    Ok(records)
}

fn feature_names(test: &Test) -> Vec<String> {
    test.gameplay
        .enabled
        .iter()
        .filter_map(|feature| {
            serde_json::to_value(feature)
                .ok()
                .and_then(|value| value.as_str().map(String::from))
        })
        .collect()
}

fn session_id() -> String {
    format!("{:032x}", rand::random::<u128>())
}

fn normalize_mods(modifiers: KeyModifiers) -> u8 {
    let mut bits = 0u8;
    if modifiers.contains(KeyModifiers::SHIFT) {
        bits |= MOD_SHIFT;
    }
    if modifiers.contains(KeyModifiers::CONTROL) {
        bits |= MOD_CTRL;
    }
    if modifiers.contains(KeyModifiers::ALT) {
        bits |= MOD_ALT;
    }
    if modifiers.contains(KeyModifiers::SUPER) {
        bits |= MOD_SUPER;
    }
    bits
}

/// Letters are lowercased so per-key aggregates treat `a` and `Shift+A` as
/// one logical key; the SHIFT distinction stays in the mods bitflags.
fn normalize_key(key: &KeyEvent) -> (String, u8) {
    let mods = normalize_mods(key.modifiers);
    let name = match key.code {
        KeyCode::Char(' ') => "Space".to_string(),
        KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => format!("Ctrl-{c}"),
        KeyCode::Char(c) => c.to_lowercase().collect(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        other => format!("{other:?}"),
    };
    (name, mods)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::Test;
    use std::collections::BTreeSet;

    fn meta() -> SessionMeta {
        SessionMeta {
            mode: "words".into(),
            time_limit_secs: None,
            word_count_requested: Some(2),
            corpus: Corpus {
                kind: "language".into(),
                name: Some("english200".into()),
                language: Some("english200".into()),
            },
            rank: None,
            level: None,
            qualifying: false,
            phoenix: false,
            chaos_modes: Vec::new(),
        }
    }

    fn typed_test() -> Test {
        let mut test = Test::new(vec!["ab".into(), "cd".into()], true, false, true);
        for c in ['a', 'b', ' ', 'c', 'd'] {
            test.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        test
    }

    #[test]
    fn normalize_key_handles_letters_space_and_chords() {
        let (name, mods) = normalize_key(&KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT));
        assert_eq!(name, "a");
        assert_eq!(mods, MOD_SHIFT);

        let (name, _) = normalize_key(&KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert_eq!(name, "Space");

        let (name, mods) = normalize_key(&KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(name, "Ctrl-w");
        assert_eq!(mods, MOD_CTRL);

        let (name, _) = normalize_key(&KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(name, "Backspace");
    }

    #[test]
    fn build_record_captures_all_keystrokes_in_order() {
        let test = typed_test();
        let results = Results::from(&test);
        let record = build_record(&test, &results, &meta(), None).unwrap();

        assert_eq!(record.keystrokes.len(), 5);
        assert!(record
            .keystrokes
            .windows(2)
            .all(|pair| pair[0].t_ms <= pair[1].t_ms));
        assert_eq!(record.keystrokes[2].key, "Space");
        assert!(record.completed);
        assert!(!record.keystrokes_truncated);
        assert_eq!(record.schema_version, SCHEMA_VERSION);
        assert_eq!(record.session_id.len(), 32);
        assert!(record.started_at_unix_ms > 0);
        assert!(record.ended_at_unix_ms >= record.started_at_unix_ms);
    }

    #[test]
    fn build_record_returns_none_without_events() {
        let test = Test::new_prepared(vec![], true, false, true, None, BTreeSet::new(), None);
        let results = Results::from(&test);
        assert!(build_record(&test, &results, &meta(), None).is_none());
    }

    #[test]
    fn append_and_tail_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        let test = typed_test();
        let results = Results::from(&test);
        let record = build_record(&test, &results, &meta(), None).unwrap();

        append_record(&path, &record).unwrap();
        append_record(&path, &record).unwrap();

        let records = read_tail(&path, &TailQuery::default()).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].keystrokes.len(), 5);
    }

    #[test]
    fn read_tail_skips_corrupt_and_torn_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        let test = typed_test();
        let results = Results::from(&test);
        let record = build_record(&test, &results, &meta(), None).unwrap();

        append_record(&path, &record).unwrap();
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"{garbage line}\n").unwrap();
        append_record(&path, &record).unwrap();
        file.write_all(b"{\"torn\":").unwrap();

        let records = read_tail(&path, &TailQuery::default()).unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn read_tail_filters_by_rank_and_level() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        let test = typed_test();
        let results = Results::from(&test);
        let mut record = build_record(&test, &results, &meta(), None).unwrap();

        append_record(&path, &record).unwrap();
        record.rank = Some("D".into());
        record.level = Some(3);
        append_record(&path, &record).unwrap();

        let query = TailQuery {
            limit: 0,
            rank: Some("D".into()),
            level: Some(3),
        };
        let records = read_tail(&path, &query).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].rank.as_deref(), Some("D"));
    }

    #[test]
    fn read_tail_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.jsonl");
        assert!(read_tail(&path, &TailQuery::default()).unwrap().is_empty());
    }

    #[test]
    fn read_tail_respects_limit_and_returns_newest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        let test = typed_test();
        let results = Results::from(&test);
        let mut record = build_record(&test, &results, &meta(), None).unwrap();

        for index in 0..10u32 {
            record.total_words_typed = index;
            append_record(&path, &record).unwrap();
        }

        let query = TailQuery {
            limit: 3,
            ..Default::default()
        };
        let records = read_tail(&path, &query).unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(
            records
                .iter()
                .map(|r| r.total_words_typed)
                .collect::<Vec<_>>(),
            vec![7, 8, 9]
        );
    }
}
