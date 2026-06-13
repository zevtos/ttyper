pub mod results;

use crate::gameplay::{contains, GameplayFeature, PreparedWord, WordKind};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::collections::BTreeSet;
use std::fmt;
use std::time::{Duration, Instant};

const STARTING_LIVES: u8 = 5;
const WORD_RUSH_INTERVAL: Duration = Duration::from_secs(1);
const PER_WORD_LIMIT: Duration = Duration::from_secs(3);
const PRECISION_FREEZE: Duration = Duration::from_secs(2);
const FREEZE_POWERUP_BONUS: Duration = Duration::from_secs(3);
const DOUBLE_TIME_BONUS: Duration = Duration::from_secs(5);
const BONUS_WORD_TIME: Duration = Duration::from_secs(5);
const RANDOM_RESTART_SAFE_WORD: &str = "safe";
const RANDOM_RESTART_LIMIT: Duration = Duration::from_secs(2);

pub struct TestEvent {
    pub time: Instant,
    pub key: KeyEvent,
    pub correct: Option<bool>,
}

pub fn is_missed_word_event(event: &TestEvent) -> bool {
    event.correct != Some(true)
}

impl fmt::Debug for TestEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TestEvent")
            .field("time", &String::from("Instant { ... }"))
            .field("key", &self.key)
            .finish()
    }
}

#[derive(Debug)]
pub struct TestWord {
    pub text: String,
    pub display: String,
    pub kind: WordKind,
    pub progress: String,
    pub events: Vec<TestEvent>,
}

impl From<String> for TestWord {
    fn from(string: String) -> Self {
        TestWord {
            display: string.clone(),
            text: string,
            kind: WordKind::Normal,
            progress: String::new(),
            events: Vec::new(),
        }
    }
}

impl From<&str> for TestWord {
    fn from(string: &str) -> Self {
        Self::from(string.to_string())
    }
}

impl From<PreparedWord> for TestWord {
    fn from(word: PreparedWord) -> Self {
        Self {
            text: word.text,
            display: word.display,
            kind: word.kind,
            progress: String::new(),
            events: Vec::new(),
        }
    }
}

impl TestWord {
    pub fn prompt(&self) -> &str {
        if self.display.is_empty() {
            &self.text
        } else {
            &self.display
        }
    }
}

/// Outcome shown when a network race ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RaceOutcome {
    Win,
    Lose,
    Tie,
    Disconnected,
}

/// Local and opponent progress shown during and after a race.
#[derive(Clone, Debug)]
pub struct RaceProgress {
    pub you: usize,
    pub opponent: usize,
    pub total: usize,
    pub outcome: Option<RaceOutcome>,
    pub message: Option<String>,
    pub you_wpm: Option<f64>,
    pub opponent_wpm: Option<f64>,
    pub you_accuracy: Option<f64>,
    pub opponent_accuracy: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct RestartThreat {
    pub deadline: Instant,
    pub progress: String,
}

#[derive(Clone, Debug)]
pub struct GameplayState {
    pub enabled: BTreeSet<GameplayFeature>,
    pub lives: Option<u8>,
    pub mistakes: usize,
    pub correct_words: usize,
    pub combo: usize,
    pub max_combo: usize,
    pub streak_savers: usize,
    pub word_shield_available: bool,
    pub score_multiplier: f64,
    pub timer_bonus: Duration,
    pub current_word_started_at: Option<Instant>,
    pub word_rush_next_at: Option<Instant>,
    pub freeze_until: Option<Instant>,
    pub speed_ramp_level: usize,
    pub checkpoint_required_wpm: f64,
    pub end_reason: Option<String>,
    pub final_completed_words: Option<usize>,
    pub ghost_best_wpm: Option<f64>,
    pub restart_threat: Option<RestartThreat>,
}

impl GameplayState {
    pub fn new(enabled: BTreeSet<GameplayFeature>, ghost_best_wpm: Option<f64>) -> Self {
        let lives = (contains(&enabled, GameplayFeature::LivesSystem)
            || contains(&enabled, GameplayFeature::EnduranceMode))
        .then_some(STARTING_LIVES);
        let point_buy = contains(&enabled, GameplayFeature::PointBuyMode);
        let word_shield_available = point_buy || contains(&enabled, GameplayFeature::WordShield);

        Self {
            enabled,
            lives,
            mistakes: 0,
            correct_words: 0,
            combo: 0,
            max_combo: 0,
            streak_savers: usize::from(point_buy) * 2,
            word_shield_available,
            score_multiplier: if point_buy { 1.05 } else { 1.0 },
            timer_bonus: if point_buy {
                Duration::from_secs(5)
            } else {
                Duration::from_secs(0)
            },
            current_word_started_at: None,
            word_rush_next_at: None,
            freeze_until: None,
            speed_ramp_level: 0,
            checkpoint_required_wpm: 30.0,
            end_reason: None,
            final_completed_words: None,
            ghost_best_wpm,
            restart_threat: None,
        }
    }

    pub fn is_enabled(&self, feature: GameplayFeature) -> bool {
        contains(&self.enabled, feature)
    }

    pub fn reset_for_restart(&mut self) {
        let enabled = self.enabled.clone();
        let ghost_best_wpm = self.ghost_best_wpm;
        *self = Self::new(enabled, ghost_best_wpm);
    }
}

#[derive(Debug)]
pub struct Test {
    pub words: Vec<TestWord>,
    pub current_word: usize,
    pub complete: bool,
    pub started_at: Option<Instant>,
    pub time_limit: Option<Duration>,
    pub race_progress: Option<RaceProgress>,
    pub backtracking_enabled: bool,
    pub sudden_death_enabled: bool,
    pub backspace_enabled: bool,
    pub gameplay: GameplayState,
    /// Launch context recorded into history at test end; None skips recording.
    pub session_meta: Option<crate::history::SessionMeta>,
    /// Compact rank/level tag (e.g. "D·L3") shown in the test title.
    pub rank_tag: Option<String>,
    /// Phoenix Protocol: one mistake burns the test; a fresh word set rises.
    pub phoenix_enabled: bool,
    /// Set on a phoenix death; the main loop rebuilds the test with new words.
    pub regen_requested: bool,
}

impl Test {
    #[allow(dead_code)]
    pub fn new(
        words: Vec<String>,
        backtracking_enabled: bool,
        sudden_death_enabled: bool,
        backspace_enabled: bool,
    ) -> Self {
        Self::new_with_time_limit(
            words,
            backtracking_enabled,
            sudden_death_enabled,
            backspace_enabled,
            None,
        )
    }

    pub fn new_prepared(
        words: Vec<PreparedWord>,
        backtracking_enabled: bool,
        sudden_death_enabled: bool,
        backspace_enabled: bool,
        time_limit: Option<Duration>,
        gameplay_features: BTreeSet<GameplayFeature>,
        ghost_best_wpm: Option<f64>,
    ) -> Self {
        Self {
            words: words.into_iter().map(TestWord::from).collect(),
            current_word: 0,
            complete: false,
            started_at: None,
            time_limit,
            race_progress: None,
            backtracking_enabled,
            sudden_death_enabled,
            backspace_enabled,
            gameplay: GameplayState::new(gameplay_features, ghost_best_wpm),
            session_meta: None,
            rank_tag: None,
            phoenix_enabled: false,
            regen_requested: false,
        }
    }

    /// Creates a typing test with an optional timed-mode duration.
    #[allow(dead_code)]
    pub fn new_with_time_limit(
        words: Vec<String>,
        backtracking_enabled: bool,
        sudden_death_enabled: bool,
        backspace_enabled: bool,
        time_limit: Option<Duration>,
    ) -> Self {
        Self::new_prepared(
            words.into_iter().map(PreparedWord::from).collect(),
            backtracking_enabled,
            sudden_death_enabled,
            backspace_enabled,
            time_limit,
            BTreeSet::new(),
            None,
        )
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }

        let event_time = Instant::now();
        if self.is_input_frozen(event_time) {
            return;
        }
        if self.started_at.is_none() && is_typing_key(&key) {
            self.started_at = Some(event_time);
            self.start_current_word_timer(event_time);
        }

        if self.handle_restart_threat_key(key, event_time) {
            return;
        }

        match key.code {
            KeyCode::Char(' ') | KeyCode::Enter => {
                let word = &self.words[self.current_word];
                if word.text.chars().nth(word.progress.len()) == Some(' ') {
                    self.push_character(' ', key, event_time);
                } else if !word.progress.is_empty()
                    || word.text.is_empty()
                    || word.kind == WordKind::Penalty
                {
                    let correct = self.current_word_is_correct_submission();
                    self.finish_current_word(correct, key, event_time);
                }
            }
            KeyCode::Backspace if self.backspace_allowed() => {
                if self.words[self.current_word].progress.is_empty()
                    && self.backtracking_enabled
                    && self.backspace_enabled
                {
                    self.last_word(event_time);
                } else {
                    let correct = {
                        let word = &self.words[self.current_word];
                        !word.text.starts_with(&word.progress[..])
                    };
                    let word = &mut self.words[self.current_word];
                    word.events.push(TestEvent {
                        time: event_time,
                        correct: Some(correct),
                        key,
                    });
                    word.progress.pop();
                }
            }
            // CTRL-BackSpace and CTRL-W
            KeyCode::Char('h') | KeyCode::Char('w')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                if self.words[self.current_word].progress.is_empty() {
                    self.last_word(event_time);
                }

                let word = &mut self.words[self.current_word];

                word.events.push(TestEvent {
                    time: event_time,
                    correct: None,
                    key,
                });
                word.progress.clear();
            }
            KeyCode::Char(c) => self.push_character(c, key, event_time),
            _ => {}
        };
    }

    fn push_character(&mut self, character: char, key: KeyEvent, event_time: Instant) {
        if self.words[self.current_word].kind == WordKind::Penalty {
            self.words[self.current_word].progress.push(character);
            self.words[self.current_word].events.push(TestEvent {
                time: event_time,
                correct: Some(false),
                key,
            });
            self.fail_current_word(key, event_time);
            return;
        }

        let is_last_word = self.current_word == self.words.len().saturating_sub(1);
        let word = &mut self.words[self.current_word];
        word.progress.push(character);
        let correct = word.text.starts_with(&word.progress[..]);

        if correct {
            let completed_last_word = is_last_word && word.progress == word.text;
            let word_kind = word.kind;
            word.events.push(TestEvent {
                time: event_time,
                correct: Some(true),
                key,
            });
            if completed_last_word {
                self.record_correct_word(word_kind, false, event_time);
                self.complete = true;
                self.current_word = 0;
            }
            return;
        }

        if self.consume_streak_saver() {
            self.words[self.current_word].progress.pop();
            self.words[self.current_word].events.push(TestEvent {
                time: event_time,
                correct: None,
                key,
            });
            return;
        }

        self.words[self.current_word].events.push(TestEvent {
            time: event_time,
            correct: Some(false),
            key,
        });

        if self.gameplay.is_enabled(GameplayFeature::RicochetMode) {
            self.words[self.current_word].progress.clear();
        }
        if self.gameplay.is_enabled(GameplayFeature::PrecisionMode) {
            self.gameplay.freeze_until = Some(event_time + PRECISION_FREEZE);
        }
        if self.gameplay.is_enabled(GameplayFeature::SuddenDeathPlus) {
            self.end_test("Sudden death mistake");
        } else {
            self.trigger_death_modes();
        }
    }

    /// Applies classic sudden-death and Phoenix Protocol on any error.
    /// Returns true when the test was reset or queued for a phoenix respawn,
    /// so callers can halt further word advancement.
    fn trigger_death_modes(&mut self) -> bool {
        if self.phoenix_enabled && self.race_progress.is_none() {
            // Phoenix death: the whole word set burns and respawns fresh.
            self.regen_requested = true;
            true
        } else if self.sudden_death_enabled || self.phoenix_enabled {
            // Phoenix inside a race can't regenerate synced words; classic reset.
            self.reset();
            true
        } else {
            false
        }
    }

    fn finish_current_word(&mut self, correct: bool, key: KeyEvent, event_time: Instant) {
        let word_kind = self.words[self.current_word].kind;
        let was_penalty_skip = word_kind == WordKind::Penalty
            && self.words[self.current_word].progress.is_empty()
            && correct;

        self.words[self.current_word].events.push(TestEvent {
            time: event_time,
            correct: Some(correct),
            key,
        });

        let died = if correct {
            self.record_correct_word(word_kind, was_penalty_skip, event_time);
            false
        } else {
            self.record_failed_word(word_kind, event_time)
        };

        // A death mode reset or queued a respawn; don't advance past it.
        if self.complete || died {
            return;
        }

        self.next_word(event_time);
    }

    fn fail_current_word(&mut self, key: KeyEvent, event_time: Instant) {
        self.finish_current_word(false, key, event_time);
    }

    fn record_correct_word(
        &mut self,
        word_kind: WordKind,
        was_penalty_skip: bool,
        event_time: Instant,
    ) {
        if !was_penalty_skip {
            self.gameplay.correct_words += 1;
            self.gameplay.combo += 1;
            self.gameplay.max_combo = self.gameplay.max_combo.max(self.gameplay.combo);
        }

        if self.gameplay.is_enabled(GameplayFeature::ComboMultiplier) {
            self.gameplay.score_multiplier = self
                .gameplay
                .score_multiplier
                .max(combo_multiplier(self.gameplay.max_combo));
        }
        if word_kind == WordKind::DoublePoints {
            self.gameplay.score_multiplier *= 2.0;
        }
        if word_kind == WordKind::Bonus {
            self.gameplay.timer_bonus += BONUS_WORD_TIME;
        }
        if word_kind == WordKind::Boss {
            self.gameplay.score_multiplier += 0.25;
        }
        if self.gameplay.is_enabled(GameplayFeature::StreakSaver)
            && self.gameplay.correct_words > 0
            && self.gameplay.correct_words.is_multiple_of(10)
        {
            self.gameplay.streak_savers += 1;
        }
        if self.gameplay.is_enabled(GameplayFeature::FreezePowerUp)
            && self.gameplay.correct_words > 0
            && self.gameplay.correct_words.is_multiple_of(20)
        {
            self.gameplay.timer_bonus += FREEZE_POWERUP_BONUS;
        }
        if self.gameplay.is_enabled(GameplayFeature::DoubleTimePowerUp)
            && self.gameplay.correct_words > 0
            && self.gameplay.correct_words.is_multiple_of(30)
        {
            self.gameplay.timer_bonus += DOUBLE_TIME_BONUS;
        }
        if self.gameplay.is_enabled(GameplayFeature::CheckpointMode)
            && self.gameplay.correct_words > 0
            && self.gameplay.correct_words.is_multiple_of(10)
        {
            self.check_checkpoint(event_time);
        }
        if self.gameplay.is_enabled(GameplayFeature::AdaptiveSpeed) {
            self.harden_upcoming_words(event_time);
        }
        if self.gameplay.is_enabled(GameplayFeature::CrescendoMode)
            && self.gameplay.correct_words > 0
            && self.gameplay.correct_words.is_multiple_of(25)
        {
            self.extend_crescendo_round();
        }
    }

    /// Returns true when a death mode reset the test or queued a respawn.
    fn record_failed_word(&mut self, word_kind: WordKind, event_time: Instant) -> bool {
        if self.gameplay.word_shield_available {
            self.gameplay.word_shield_available = false;
            self.gameplay.combo = 0;
            return false;
        }

        self.gameplay.mistakes += 1;
        self.gameplay.combo = 0;
        if word_kind == WordKind::ComboBreaker {
            self.gameplay.score_multiplier = 1.0;
        }
        if self.gameplay.is_enabled(GameplayFeature::PracticeWordLock) {
            let word = self.words[self.current_word].text.clone();
            self.words.push(TestWord::from(word));
            if let Some(race) = &mut self.race_progress {
                race.total = self.words.len();
            }
        }
        if let Some(lives) = &mut self.gameplay.lives {
            *lives = lives.saturating_sub(1);
            if *lives == 0 {
                self.end_test("Out of lives");
            }
        }
        if self.gameplay.is_enabled(GameplayFeature::SuddenDeathPlus) {
            self.end_test("Sudden death mistake");
        }
        if self.gameplay.is_enabled(GameplayFeature::PrecisionMode) {
            self.gameplay.freeze_until = Some(event_time + PRECISION_FREEZE);
        }
        if self.complete {
            return false;
        }
        self.trigger_death_modes()
    }

    fn current_word_is_correct_submission(&self) -> bool {
        let word = &self.words[self.current_word];
        if word.kind == WordKind::Penalty {
            word.progress.is_empty()
        } else {
            word.text == word.progress
        }
    }

    fn consume_streak_saver(&mut self) -> bool {
        if !self.gameplay.is_enabled(GameplayFeature::StreakSaver)
            || self.gameplay.streak_savers == 0
        {
            return false;
        }

        self.gameplay.streak_savers -= 1;
        true
    }

    fn backspace_allowed(&mut self) -> bool {
        if self.backspace_enabled {
            return true;
        }

        self.consume_streak_saver()
    }

    fn is_input_frozen(&mut self, now: Instant) -> bool {
        if self.gameplay.freeze_until.is_some_and(|until| now >= until) {
            self.gameplay.freeze_until = None;
        }

        self.gameplay.freeze_until.is_some()
    }

    fn handle_restart_threat_key(&mut self, key: KeyEvent, event_time: Instant) -> bool {
        let Some(threat) = &mut self.gameplay.restart_threat else {
            return false;
        };

        match key.code {
            KeyCode::Char(character) => {
                threat.progress.push(character);
                if RANDOM_RESTART_SAFE_WORD.starts_with(&threat.progress) {
                    if threat.progress == RANDOM_RESTART_SAFE_WORD {
                        self.gameplay.restart_threat = None;
                    }
                } else {
                    threat.progress.clear();
                }
                true
            }
            KeyCode::Backspace => {
                threat.progress.pop();
                true
            }
            KeyCode::Esc => {
                self.end_test("Restart threat failed");
                true
            }
            _ => {
                if event_time >= threat.deadline {
                    self.reset();
                }
                true
            }
        }
    }

    pub fn tick(&mut self, now: Instant) {
        if self.complete || self.started_at.is_none() {
            return;
        }

        self.start_current_word_timer(now);
        self.tick_restart_threat(now);

        if self.complete {
            return;
        }

        if self.gameplay.is_enabled(GameplayFeature::TimedPerWord)
            && self
                .gameplay
                .current_word_started_at
                .is_some_and(|started| now.saturating_duration_since(started) >= PER_WORD_LIMIT)
        {
            self.force_fail_current_word(now, "Word timer expired");
            return;
        }

        if self.gameplay.is_enabled(GameplayFeature::WordRush)
            && self
                .gameplay
                .word_rush_next_at
                .is_some_and(|next| now >= next)
        {
            let correct = self.current_word_is_correct_submission();
            self.record_forced_submission_event(now, correct);
            let died = if correct {
                self.record_correct_word(self.words[self.current_word].kind, false, now);
                false
            } else {
                self.record_failed_word(self.words[self.current_word].kind, now)
            };
            if !self.complete && !died {
                self.next_word(now);
            }
            return;
        }

        if self.gameplay.is_enabled(GameplayFeature::SpeedRamp) {
            self.tick_speed_ramp(now);
        }
    }

    fn start_current_word_timer(&mut self, now: Instant) {
        if self.gameplay.current_word_started_at.is_none() {
            self.gameplay.current_word_started_at = Some(now);
        }
        if self.gameplay.word_rush_next_at.is_none() {
            self.gameplay.word_rush_next_at = Some(now + WORD_RUSH_INTERVAL);
        }
    }

    fn tick_restart_threat(&mut self, now: Instant) {
        if !self
            .gameplay
            .is_enabled(GameplayFeature::RandomRestartThreat)
        {
            return;
        }

        if let Some(threat) = &self.gameplay.restart_threat {
            if now >= threat.deadline {
                self.reset();
            }
            return;
        }

        if self.gameplay.correct_words > 0 && self.gameplay.correct_words.is_multiple_of(17) {
            self.gameplay.restart_threat = Some(RestartThreat {
                deadline: now + RANDOM_RESTART_LIMIT,
                progress: String::new(),
            });
        }
    }

    fn tick_speed_ramp(&mut self, now: Instant) {
        let Some(started_at) = self.started_at else {
            return;
        };

        let elapsed = now.saturating_duration_since(started_at);
        let level = (elapsed.as_secs() / 30) as usize;
        if level <= self.gameplay.speed_ramp_level {
            return;
        }

        self.gameplay.speed_ramp_level = level;
        let required = 35.0 + 10.0 * level as f64;
        if self.live_wpm_at(now).unwrap_or_default() < required {
            self.end_test(format!("Speed ramp missed {:.0} WPM", required));
        }
    }

    fn check_checkpoint(&mut self, now: Instant) {
        let required = self.gameplay.checkpoint_required_wpm;
        if self.live_wpm_at(now).unwrap_or_default() < required {
            self.end_test(format!("Checkpoint missed {:.0} WPM", required));
            return;
        }

        self.gameplay.checkpoint_required_wpm += 5.0;
    }

    fn harden_upcoming_words(&mut self, now: Instant) {
        let Some(wpm) = self.live_wpm_at(now) else {
            return;
        };
        if wpm < 60.0 {
            return;
        }

        let minimum = if wpm >= 100.0 { 10 } else { 8 };
        for word in self
            .words
            .iter_mut()
            .skip(self.current_word + 1)
            .take(5)
            .filter(|word| word.kind == WordKind::Normal)
        {
            if word.text.chars().count() < minimum {
                word.text = crate::gameplay::harden_word(&word.text, minimum);
                word.display = word.text.clone();
            }
        }
    }

    fn extend_crescendo_round(&mut self) {
        let next_words = self
            .words
            .iter()
            .skip(self.current_word.saturating_sub(10))
            .take(10)
            .map(|word| crate::gameplay::harden_word(&word.text, word.text.chars().count() + 2))
            .map(TestWord::from)
            .collect::<Vec<_>>();
        self.words.extend(next_words);
        self.gameplay.timer_bonus += Duration::from_secs(10);
        if let Some(race) = &mut self.race_progress {
            race.total = self.words.len();
        }
    }

    fn force_fail_current_word(&mut self, now: Instant, reason: &'static str) {
        self.record_forced_submission_event(now, false);
        let died = self.record_failed_word(self.words[self.current_word].kind, now);
        if self.gameplay.end_reason.is_none() {
            self.gameplay.end_reason = Some(reason.into());
        }
        if !self.complete && !died {
            self.next_word(now);
        }
    }

    fn record_forced_submission_event(&mut self, now: Instant, correct: bool) {
        self.words[self.current_word].events.push(TestEvent {
            time: now,
            key: KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            correct: Some(correct),
        });
    }

    fn end_test(&mut self, reason: impl Into<String>) {
        self.gameplay.end_reason = Some(reason.into());
        self.gameplay.final_completed_words = Some(self.current_word);
        self.complete = true;
        self.current_word = 0;
    }

    pub fn feature_enabled(&self, feature: GameplayFeature) -> bool {
        self.gameplay.is_enabled(feature)
    }

    pub fn gameplay_multiplier(&self) -> f64 {
        self.gameplay.score_multiplier.max(1.0)
    }

    pub fn gameplay_status_parts(&self, now: Instant) -> Vec<String> {
        let mut parts = Vec::new();

        if let Some(lives) = self.gameplay.lives {
            parts.push(format!("Lives {}", lives));
        }
        if self.gameplay.is_enabled(GameplayFeature::PowerMode) {
            parts.push(format!("Combo {}x", self.gameplay.combo));
        } else if self.gameplay.is_enabled(GameplayFeature::ComboMultiplier) {
            parts.push(format!("Combo {}", self.gameplay.combo));
        }
        if self.gameplay.streak_savers > 0 {
            parts.push(format!("Savers {}", self.gameplay.streak_savers));
        }
        if self.gameplay.word_shield_available {
            parts.push("Shield".into());
        }
        if self.gameplay.score_multiplier > 1.0 {
            parts.push(format!("x{:.2}", self.gameplay.score_multiplier));
        }
        if self.gameplay.is_enabled(GameplayFeature::TimedPerWord) {
            if let Some(started) = self.gameplay.current_word_started_at {
                let remaining =
                    PER_WORD_LIMIT.saturating_sub(now.saturating_duration_since(started));
                parts.push(format!(
                    "Word {}s",
                    remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0)
                ));
            }
        }
        if self.gameplay.is_enabled(GameplayFeature::SpeedRamp) {
            parts.push(format!(
                "Ramp {:.0}",
                35.0 + 10.0 * self.gameplay.speed_ramp_level as f64
            ));
        }
        if let Some(best) = self.gameplay.ghost_best_wpm {
            if self.gameplay.is_enabled(GameplayFeature::GhostRace) {
                let ghost = self.ghost_progress_at(now).unwrap_or_default();
                parts.push(format!("Ghost {:.1}/{}", best, ghost));
            }
        }
        if let Some(threat) = &self.gameplay.restart_threat {
            let remaining = threat.deadline.saturating_duration_since(now);
            parts.push(format!(
                "SAFE {}s {}",
                remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0),
                threat.progress
            ));
        }
        if self.phoenix_enabled {
            parts.push("PHOENIX".into());
        }
        if let Some(reason) = &self.gameplay.end_reason {
            parts.push(reason.clone());
        }

        parts
    }

    pub fn effective_time_limit(&self) -> Option<Duration> {
        self.time_limit
            .map(|limit| limit + self.gameplay.timer_bonus)
    }

    pub fn visual_elapsed_multiplier(&self) -> f64 {
        let prestige = self.feature_enabled(GameplayFeature::PrestigeChallenge);
        let accelerating_cursor = self.feature_enabled(GameplayFeature::AcceleratingCursor);
        match (prestige, accelerating_cursor) {
            (true, true) => 2.5,
            (true, false) => 2.0,
            (false, true) => 1.0 + self.gameplay.correct_words as f64 / 50.0,
            (false, false) => 1.0,
        }
    }

    pub fn ghost_progress_at(&self, now: Instant) -> Option<usize> {
        let best_wpm = self.gameplay.ghost_best_wpm?;
        if !self.feature_enabled(GameplayFeature::GhostRace) {
            return None;
        }
        let started_at = self.started_at?;
        let elapsed_minutes = now.saturating_duration_since(started_at).as_secs_f64() / 60.0;
        let estimated_words = (best_wpm * elapsed_minutes).floor() as usize;
        Some(estimated_words.min(self.words.len()))
    }
    /// Counts typed characters that currently match their target positions.
    pub fn correctly_typed_chars(&self) -> usize {
        self.words
            .iter()
            .map(|word| {
                word.progress
                    .chars()
                    .zip(word.text.chars())
                    .filter(|(typed, target)| typed == target)
                    .count()
            })
            .sum()
    }

    /// Calculates live WPM from the first typing keypress through the given time.
    pub fn live_wpm_at(&self, now: Instant) -> Option<f64> {
        let started_at = self.started_at?;
        let elapsed_minutes = now
            .checked_duration_since(started_at)
            .map(|duration| duration.as_secs_f64() / 60.0)
            .unwrap_or_default();

        if elapsed_minutes <= f64::EPSILON {
            return Some(0.0);
        }

        Some((self.correctly_typed_chars() as f64 / 5.0) / elapsed_minutes)
    }

    /// Returns the remaining countdown time for timed tests.
    pub fn time_remaining_at(&self, now: Instant) -> Option<Duration> {
        self.time_remaining_at_with_multiplier(now, 1.0)
    }

    /// Returns countdown time with a visual/effective timer multiplier.
    pub fn time_remaining_at_with_multiplier(
        &self,
        now: Instant,
        multiplier: f64,
    ) -> Option<Duration> {
        let time_limit = self.effective_time_limit()?;
        let Some(started_at) = self.started_at else {
            return Some(time_limit);
        };

        let elapsed = now.checked_duration_since(started_at).unwrap_or_default();
        let elapsed = scale_duration(elapsed, multiplier);

        Some(time_limit.saturating_sub(elapsed))
    }

    /// Returns countdown time after an externally tracked effective elapsed duration.
    pub fn time_remaining_after_elapsed(&self, elapsed: Duration) -> Option<Duration> {
        Some(self.effective_time_limit()?.saturating_sub(elapsed))
    }

    /// Returns true after a timed test has started and consumed its duration.
    pub fn time_expired_at(&self, now: Instant) -> bool {
        self.time_expired_at_with_multiplier(now, 1.0)
    }

    /// Returns true after a scaled timed test has consumed its duration.
    pub fn time_expired_at_with_multiplier(&self, now: Instant, multiplier: f64) -> bool {
        self.started_at.is_some()
            && self
                .time_remaining_at_with_multiplier(now, multiplier)
                .is_some_and(|remaining| remaining.is_zero())
    }

    /// Returns true when an externally tracked effective duration has consumed timed mode.
    pub fn time_expired_after_elapsed(&self, elapsed: Duration) -> bool {
        self.started_at.is_some()
            && self
                .time_remaining_after_elapsed(elapsed)
                .is_some_and(|remaining| remaining.is_zero())
    }

    /// Enables race progress tracking for this test.
    pub fn enable_race(&mut self) {
        self.race_progress = Some(RaceProgress {
            you: 0,
            opponent: 0,
            total: self.words.len(),
            outcome: None,
            message: None,
            you_wpm: None,
            opponent_wpm: None,
            you_accuracy: None,
            opponent_accuracy: None,
        });
    }

    /// Counts completed words for race progress reporting.
    pub fn completed_word_count(&self) -> usize {
        if let Some(completed) = self.gameplay.final_completed_words {
            completed
        } else if self.complete {
            self.words.len()
        } else {
            self.current_word
        }
    }

    /// Updates the locally displayed race progress.
    pub fn update_race_you(&mut self) {
        let completed = self.completed_word_count();
        if let Some(race) = &mut self.race_progress {
            race.you = completed.min(race.total);
        }
    }

    /// Updates the opponent's displayed race progress.
    pub fn update_race_opponent(&mut self, completed: usize) {
        if let Some(race) = &mut self.race_progress {
            race.opponent = completed.min(race.total);
        }
    }

    /// Stores the final race outcome for the results screen.
    pub fn set_race_outcome(&mut self, outcome: RaceOutcome, message: impl Into<String>) {
        if let Some(race) = &mut self.race_progress {
            race.outcome = Some(outcome);
            race.message = Some(message.into());
        }
    }

    pub fn set_race_local_metrics(&mut self, wpm: f64, accuracy: f64) {
        if let Some(race) = &mut self.race_progress {
            race.you_wpm = Some(wpm);
            race.you_accuracy = Some(accuracy);
        }
    }

    pub fn set_race_opponent_metrics(&mut self, wpm: f64, accuracy: f64) {
        if let Some(race) = &mut self.race_progress {
            race.opponent_wpm = Some(wpm);
            race.opponent_accuracy = Some(accuracy);
        }
    }

    fn last_word(&mut self, now: Instant) {
        if self.current_word != 0 {
            self.current_word -= 1;
            self.gameplay.current_word_started_at = Some(now);
            self.gameplay.word_rush_next_at = Some(now + WORD_RUSH_INTERVAL);
        }
    }

    fn next_word(&mut self, now: Instant) {
        if self.current_word == self.words.len() - 1 {
            self.complete = true;
            self.current_word = 0;
        } else {
            self.current_word += 1;
            self.gameplay.current_word_started_at = Some(now);
            self.gameplay.word_rush_next_at = Some(now + WORD_RUSH_INTERVAL);
        }
    }

    pub fn reset(&mut self) {
        self.words.iter_mut().for_each(|word: &mut TestWord| {
            word.progress.clear();
            word.events.clear();
        });
        self.current_word = 0;
        self.complete = false;
        self.started_at = None;
        self.gameplay.reset_for_restart();
        if let Some(race) = &mut self.race_progress {
            race.you = 0;
            race.opponent = 0;
            race.outcome = None;
            race.message = None;
            race.you_wpm = None;
            race.opponent_wpm = None;
            race.you_accuracy = None;
            race.opponent_accuracy = None;
        }
    }
}

/// Returns true for keypresses that should start the live typing timer.
fn is_typing_key(key: &KeyEvent) -> bool {
    matches!(
        key.code,
        KeyCode::Char(_) | KeyCode::Enter | KeyCode::Backspace
    )
}

fn scale_duration(duration: Duration, multiplier: f64) -> Duration {
    if multiplier <= 1.0 {
        return duration;
    }

    Duration::from_secs_f64(duration.as_secs_f64() * multiplier)
}

fn combo_multiplier(combo: usize) -> f64 {
    (1.0 + combo as f64 * 0.05).min(5.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn key_char(character: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)
    }

    fn key_enter() -> KeyEvent {
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
    }

    #[test]
    fn live_wpm_counts_only_correctly_typed_characters() {
        let start = Instant::now();
        let mut test = Test::new(vec!["hello".into(), "world".into()], true, false, true);
        test.started_at = Some(start);
        test.words[0].progress = "hello".into();
        test.words[1].progress = "worxx".into();

        assert_eq!(test.correctly_typed_chars(), 8);
        assert_eq!(test.live_wpm_at(start + Duration::from_secs(60)), Some(1.6));
    }

    #[test]
    fn live_wpm_is_hidden_until_first_typing_keypress() {
        let test = Test::new(vec!["hello".into()], true, false, true);

        assert_eq!(test.live_wpm_at(Instant::now()), None);
    }

    #[test]
    fn timed_test_reports_full_time_before_first_keypress() {
        let test = Test::new_with_time_limit(
            vec!["hello".into()],
            true,
            false,
            true,
            Some(Duration::from_secs(30)),
        );

        assert_eq!(
            test.time_remaining_at(Instant::now()),
            Some(Duration::from_secs(30))
        );
        assert!(!test.time_expired_at(Instant::now()));
    }

    #[test]
    fn timed_test_expires_after_started_duration() {
        let start = Instant::now();
        let mut test = Test::new_with_time_limit(
            vec!["hello".into()],
            true,
            false,
            true,
            Some(Duration::from_secs(30)),
        );
        test.started_at = Some(start);

        assert_eq!(
            test.time_remaining_at(start + Duration::from_secs(10)),
            Some(Duration::from_secs(20))
        );
        assert!(test.time_expired_at(start + Duration::from_secs(30)));
    }

    #[test]
    fn timed_test_uses_multiplier_for_speed_demon_countdown() {
        let start = Instant::now();
        let mut test = Test::new_with_time_limit(
            vec!["hello".into()],
            true,
            false,
            true,
            Some(Duration::from_secs(30)),
        );
        test.started_at = Some(start);

        assert_eq!(
            test.time_remaining_at_with_multiplier(start + Duration::from_secs(5), 2.0),
            Some(Duration::from_secs(20))
        );
        assert!(test.time_expired_at_with_multiplier(start + Duration::from_secs(15), 2.0));
    }

    #[test]
    fn timed_test_can_use_externally_tracked_elapsed_time() {
        let start = Instant::now();
        let mut test = Test::new_with_time_limit(
            vec!["hello".into()],
            true,
            false,
            true,
            Some(Duration::from_secs(30)),
        );
        test.started_at = Some(start);

        assert_eq!(
            test.time_remaining_after_elapsed(Duration::from_secs(12)),
            Some(Duration::from_secs(18))
        );
        assert!(test.time_expired_after_elapsed(Duration::from_secs(30)));
    }

    #[test]
    fn race_progress_tracks_completed_words() {
        let mut test = Test::new(vec!["hello".into(), "world".into()], true, false, true);
        test.enable_race();
        test.current_word = 1;
        test.update_race_you();
        test.update_race_opponent(2);

        let race = test.race_progress.unwrap();
        assert_eq!(race.you, 1);
        assert_eq!(race.opponent, 2);
        assert_eq!(race.total, 2);
    }

    #[test]
    fn lives_system_ends_after_five_wrong_words() {
        let mut test = Test::new_prepared(
            vec!["a".into(), "b".into(), "c".into(), "d".into(), "e".into()],
            true,
            false,
            true,
            None,
            crate::gameplay::feature_set(&[GameplayFeature::LivesSystem]),
            None,
        );

        for _ in 0..5 {
            test.handle_key(key_char('x'));
            test.handle_key(key_enter());
        }

        assert!(test.complete);
        assert_eq!(test.gameplay.lives, Some(0));
        assert_eq!(test.gameplay.end_reason.as_deref(), Some("Out of lives"));
    }

    #[test]
    fn combo_multiplier_increases_score_multiplier() {
        let mut test = Test::new_prepared(
            vec!["a".into(), "b".into()],
            true,
            false,
            true,
            None,
            crate::gameplay::feature_set(&[GameplayFeature::ComboMultiplier]),
            None,
        );

        test.handle_key(key_char('a'));
        test.handle_key(key_enter());
        test.handle_key(key_char('b'));

        assert!(test.complete);
        assert_eq!(test.gameplay.max_combo, 2);
        assert!(test.gameplay_multiplier() > 1.0);
    }

    #[test]
    fn power_mode_combo_advances_per_completed_word() {
        let mut test = Test::new_prepared(
            vec!["ab".into(), "cd".into()],
            true,
            false,
            true,
            None,
            crate::gameplay::feature_set(&[GameplayFeature::PowerMode]),
            None,
        );

        test.handle_key(key_char('a'));
        assert_eq!(test.gameplay.combo, 0);
        assert_eq!(test.gameplay.max_combo, 0);

        test.handle_key(key_char('b'));
        assert_eq!(test.gameplay.combo, 0);
        assert_eq!(test.gameplay.max_combo, 0);

        test.handle_key(key_enter());
        assert_eq!(test.gameplay.combo, 1);
        assert_eq!(test.gameplay.max_combo, 1);
        assert!(test
            .gameplay_status_parts(Instant::now())
            .contains(&"Combo 1x".to_string()));

        test.handle_key(key_char('c'));
        assert_eq!(test.gameplay.combo, 1);

        test.handle_key(key_char('d'));
        assert!(test.complete);
        assert_eq!(test.gameplay.combo, 2);
        assert_eq!(test.gameplay.max_combo, 2);
    }

    #[test]
    fn phoenix_requests_regen_on_first_mistake() {
        let mut test = Test::new(vec!["hello".into()], true, false, true);
        test.phoenix_enabled = true;

        test.handle_key(key_char('h'));
        assert!(!test.regen_requested);

        test.handle_key(key_char('x'));
        assert!(test.regen_requested, "wrong char should burn the test");
        assert!(!test.complete);
    }

    #[test]
    fn phoenix_burns_on_wrong_word_submit() {
        let mut test = Test::new(vec!["hello".into(), "world".into()], true, false, true);
        test.phoenix_enabled = true;

        // Submit the first word incomplete via space; should burn, not skip.
        test.handle_key(key_char('h'));
        test.handle_key(key_enter());

        assert!(test.regen_requested, "submitting a wrong word should burn");
        assert_eq!(test.current_word, 0, "must not advance to the next word");
        assert!(!test.complete);
    }

    #[test]
    fn phoenix_in_race_falls_back_to_classic_reset() {
        let mut test = Test::new(vec!["hello".into()], true, false, true);
        test.phoenix_enabled = true;
        test.enable_race();

        test.handle_key(key_char('h'));
        test.handle_key(key_char('x'));

        assert!(!test.regen_requested, "synced race words cannot regenerate");
        assert!(
            test.words[0].progress.is_empty(),
            "classic reset should clear progress"
        );
    }

    #[test]
    fn timed_per_word_forces_a_miss_after_three_seconds() {
        let start = Instant::now();
        let mut test = Test::new_prepared(
            vec!["slow".into(), "next".into()],
            true,
            false,
            true,
            None,
            crate::gameplay::feature_set(&[GameplayFeature::TimedPerWord]),
            None,
        );
        test.started_at = Some(start);
        test.gameplay.current_word_started_at = Some(start);

        test.tick(start + Duration::from_secs(3));

        assert_eq!(test.current_word, 1);
        assert_eq!(test.gameplay.mistakes, 1);
        assert!(test.words[0].events.iter().any(is_missed_word_event));
    }
}
