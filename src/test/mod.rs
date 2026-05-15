pub mod results;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::fmt;
use std::time::{Duration, Instant};

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
    pub progress: String,
    pub events: Vec<TestEvent>,
}

impl From<String> for TestWord {
    fn from(string: String) -> Self {
        TestWord {
            text: string,
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
}

impl Test {
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

    /// Creates a typing test with an optional timed-mode duration.
    pub fn new_with_time_limit(
        words: Vec<String>,
        backtracking_enabled: bool,
        sudden_death_enabled: bool,
        backspace_enabled: bool,
        time_limit: Option<Duration>,
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
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }

        let event_time = Instant::now();
        if self.started_at.is_none() && is_typing_key(&key) {
            self.started_at = Some(event_time);
        }

        let word = &mut self.words[self.current_word];
        match key.code {
            KeyCode::Char(' ') | KeyCode::Enter => {
                if word.text.chars().nth(word.progress.len()) == Some(' ') {
                    word.progress.push(' ');
                    word.events.push(TestEvent {
                        time: event_time,
                        correct: Some(true),
                        key,
                    })
                } else if !word.progress.is_empty() || word.text.is_empty() {
                    let correct = word.text == word.progress;
                    if self.sudden_death_enabled && !correct {
                        self.reset();
                    } else {
                        word.events.push(TestEvent {
                            time: event_time,
                            correct: Some(correct),
                            key,
                        });
                        self.next_word();
                    }
                }
            }
            KeyCode::Backspace => {
                if word.progress.is_empty() && self.backtracking_enabled && self.backspace_enabled {
                    self.last_word();
                } else if self.backspace_enabled {
                    word.events.push(TestEvent {
                        time: event_time,
                        correct: Some(!word.text.starts_with(&word.progress[..])),
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
                    self.last_word();
                }

                let word = &mut self.words[self.current_word];

                word.events.push(TestEvent {
                    time: event_time,
                    correct: None,
                    key,
                });
                word.progress.clear();
            }
            KeyCode::Char(c) => {
                word.progress.push(c);
                let correct = word.text.starts_with(&word.progress[..]);
                if self.sudden_death_enabled && !correct {
                    self.reset();
                } else {
                    word.events.push(TestEvent {
                        time: event_time,
                        correct: Some(correct),
                        key,
                    });
                    if word.progress == word.text && self.current_word == self.words.len() - 1 {
                        self.complete = true;
                        self.current_word = 0;
                    }
                }
            }
            _ => {}
        };
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
        let time_limit = self.time_limit?;
        let Some(started_at) = self.started_at else {
            return Some(time_limit);
        };

        let elapsed = now.checked_duration_since(started_at).unwrap_or_default();
        let elapsed = scale_duration(elapsed, multiplier);

        Some(time_limit.saturating_sub(elapsed))
    }

    /// Returns countdown time after an externally tracked effective elapsed duration.
    pub fn time_remaining_after_elapsed(&self, elapsed: Duration) -> Option<Duration> {
        Some(self.time_limit?.saturating_sub(elapsed))
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
        });
    }

    /// Counts completed words for race progress reporting.
    pub fn completed_word_count(&self) -> usize {
        if self.complete {
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

    fn last_word(&mut self) {
        if self.current_word != 0 {
            self.current_word -= 1;
        }
    }

    fn next_word(&mut self) {
        if self.current_word == self.words.len() - 1 {
            self.complete = true;
            self.current_word = 0;
        } else {
            self.current_word += 1;
        }
    }

    fn reset(&mut self) {
        self.words.iter_mut().for_each(|word: &mut TestWord| {
            word.progress.clear();
            word.events.clear();
        });
        self.current_word = 0;
        self.complete = false;
        self.started_at = None;
        if let Some(race) = &mut self.race_progress {
            race.you = 0;
            race.opponent = 0;
            race.outcome = None;
            race.message = None;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

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
}
