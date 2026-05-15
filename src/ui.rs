use crate::config::Theme;

use super::test::{results, RaceOutcome, Test, TestWord};

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    symbols::Marker,
    text::{Line, Span, Text},
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph, Widget},
};
use results::Fraction;
use std::time::{Duration, Instant};

// Convert CPS to WPM (clicks per second)
const WPM_PER_CPS: f64 = 12.0;

// Width of the moving average window for the WPM chart
const WPM_SMA_WIDTH: usize = 10;

#[derive(Clone)]
struct SizedBlock<'a> {
    block: Block<'a>,
    area: Rect,
}

impl SizedBlock<'_> {
    fn render(self, buf: &mut Buffer) {
        self.block.render(self.area, buf)
    }
}

trait UsedWidget: Widget {}
impl UsedWidget for Paragraph<'_> {}

trait DrawInner<T> {
    fn draw_inner(&self, content: T, buf: &mut Buffer);
}

impl DrawInner<&Line<'_>> for SizedBlock<'_> {
    fn draw_inner(&self, content: &Line, buf: &mut Buffer) {
        let inner = self.block.inner(self.area);
        buf.set_line(inner.x, inner.y, content, inner.width);
    }
}

impl<T: UsedWidget> DrawInner<T> for SizedBlock<'_> {
    fn draw_inner(&self, content: T, buf: &mut Buffer) {
        let inner = self.block.inner(self.area);
        content.render(inner, buf);
    }
}

pub trait ThemedWidget {
    fn render(self, area: Rect, buf: &mut Buffer, theme: &Theme);
}

pub struct Themed<'t, W: ?Sized> {
    theme: &'t Theme,
    widget: W,
}
impl<W: ThemedWidget> Widget for Themed<'_, W> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.widget.render(area, buf, self.theme)
    }
}
impl Theme {
    pub fn apply_to<W>(&self, widget: W) -> Themed<'_, W> {
        Themed {
            theme: self,
            widget,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TestRenderEffects {
    pub mirror_prompt: bool,
    pub ghost_mode: bool,
    pub haunted_mode: bool,
    pub flicker_seed: u64,
    pub drunk_prompt_offset: i16,
    pub blackout_prompt: bool,
    pub time_multiplier: f64,
    pub accelerated_elapsed: Option<Duration>,
}

impl Default for TestRenderEffects {
    fn default() -> Self {
        Self {
            mirror_prompt: false,
            ghost_mode: false,
            haunted_mode: false,
            flicker_seed: 0,
            drunk_prompt_offset: 0,
            blackout_prompt: false,
            time_multiplier: 1.0,
            accelerated_elapsed: None,
        }
    }
}

pub struct TestView<'a> {
    pub test: &'a Test,
    pub effects: TestRenderEffects,
}

impl ThemedWidget for &Test {
    fn render(self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        TestView {
            test: self,
            effects: TestRenderEffects::default(),
        }
        .render(area, buf, theme);
    }
}

impl ThemedWidget for TestView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        let test = self.test;
        let effects = self.effects;
        buf.set_style(area, theme.default);

        // Chunks
        let chunks = if test.race_progress.is_some() {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(6),
                    Constraint::Length(4),
                ])
                .split(area)
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(3), Constraint::Length(6)])
                .split(area)
        };

        let mut input_title = vec![Span::styled("Input", theme.title)];
        if let Some(wpm) = test.live_wpm_at(Instant::now()) {
            input_title.push(Span::raw(" "));
            input_title.push(Span::styled(
                format!("WPM {:.1}", wpm),
                theme.results_overview,
            ));
        }
        let now = Instant::now();
        let remaining = if let Some(elapsed) = effects.accelerated_elapsed {
            test.time_remaining_after_elapsed(elapsed)
        } else if effects.time_multiplier <= 1.0 {
            test.time_remaining_at(now)
        } else {
            test.time_remaining_at_with_multiplier(now, effects.time_multiplier)
        };
        if let Some(remaining) = remaining {
            let remaining_seconds = remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0);
            input_title.push(Span::raw(" "));
            input_title.push(Span::styled(
                format!("Time {}s", remaining_seconds),
                theme.results_timer,
            ));
        }

        // Sections
        let input = SizedBlock {
            block: Block::default()
                .title(Line::from(input_title))
                .borders(Borders::ALL)
                .border_type(theme.border_type)
                .border_style(theme.input_border),
            area: chunks[0],
        };
        input.draw_inner(
            &Line::from(test.words[test.current_word].progress.clone()),
            buf,
        );
        input.render(buf);

        let target_block = Block::default()
            .title(Span::styled("Prompt", theme.title))
            .borders(Borders::ALL)
            .border_type(theme.border_type)
            .border_style(theme.prompt_border);
        if effects.blackout_prompt {
            target_block.render(chunks[1], buf);
        } else {
            let target_lines: Vec<Line> = {
                let words = words_to_spans(&test.words, test.current_word, theme, effects);

                let mut lines: Vec<Line> = Vec::new();
                let mut current_line: Vec<Span> = Vec::new();
                let mut current_width = 0;
                let line_width = (chunks[1].width as usize).saturating_sub(2).max(1);
                for word in words {
                    let word_width: usize = word.iter().map(|s| s.width()).sum();

                    if current_width + word_width > line_width {
                        current_line.push(Span::raw("\n"));
                        lines.push(Line::from(current_line.clone()));
                        current_line.clear();
                        current_width = 0;
                    }

                    current_line.extend(word);
                    current_width += word_width;
                }
                lines.push(Line::from(current_line));

                apply_drunk_offset(lines, effects.drunk_prompt_offset)
            };
            let horizontal_scroll = effects.drunk_prompt_offset.saturating_neg().max(0) as u16;
            let target = Paragraph::new(target_lines)
                .scroll((0, horizontal_scroll))
                .block(target_block);
            target.render(chunks[1], buf);
        }

        if let Some(race) = &test.race_progress {
            let race_lines = vec![
                race_progress_line("You", race.you, race.total, theme.results_overview),
                race_progress_line("Opponent", race.opponent, race.total, theme.prompt_untyped),
            ];
            let race = Paragraph::new(race_lines).block(
                Block::default()
                    .title(Span::styled("Race", theme.title))
                    .borders(Borders::ALL)
                    .border_type(theme.border_type)
                    .border_style(theme.results_overview_border),
            );
            race.render(chunks[2], buf);
        }
    }
}

/// Builds one text progress bar line for race mode.
fn race_progress_line(
    label: &'static str,
    completed: usize,
    total: usize,
    style: ratatui::style::Style,
) -> Line<'static> {
    const BAR_WIDTH: usize = 20;

    let percent = race_percent(completed, total);
    let filled = BAR_WIDTH * completed.min(total) / total.max(1);
    let empty = BAR_WIDTH.saturating_sub(filled);
    Line::from(vec![Span::styled(
        format!(
            "{label:<8} [{}{}] {:>3}%",
            "#".repeat(filled),
            "-".repeat(empty),
            percent
        ),
        style,
    )])
}

/// Calculates a whole-number race progress percentage.
fn race_percent(completed: usize, total: usize) -> usize {
    (completed.min(total) * 100).checked_div(total).unwrap_or(0)
}

fn apply_drunk_offset(mut lines: Vec<Line<'static>>, offset: i16) -> Vec<Line<'static>> {
    if offset <= 0 {
        return lines;
    }

    let padding = Span::raw(" ".repeat(offset as usize));
    for line in &mut lines {
        line.spans.insert(0, padding.clone());
    }
    lines
}

fn words_to_spans(
    words: &[TestWord],
    current_word: usize,
    theme: &Theme,
    effects: TestRenderEffects,
) -> Vec<Vec<Span<'static>>> {
    let mut spans = Vec::new();
    let mut visual_index = 0usize;

    for (word_index, word) in words.iter().enumerate() {
        let parts = if word_index < current_word {
            split_typed_word(word)
        } else if word_index == current_word {
            split_current_word(word)
        } else {
            vec![(word.text.clone(), Status::Untyped)]
        };
        spans.push(word_parts_to_spans(
            parts,
            theme,
            effects,
            word_index >= current_word,
            &mut visual_index,
        ));
    }

    if effects.mirror_prompt {
        mirror_words(spans)
    } else {
        spans
    }
}

#[derive(PartialEq, Clone, Copy, Debug)]
enum Status {
    Correct,
    Incorrect,
    CurrentUntyped,
    CurrentCorrect,
    CurrentIncorrect,
    Cursor,
    Untyped,
    Overtyped,
}

fn split_current_word(word: &TestWord) -> Vec<(String, Status)> {
    let mut parts = Vec::new();
    let mut cur_string = String::new();
    let mut cur_status = Status::Untyped;

    let mut progress = word.progress.chars();
    for tc in word.text.chars() {
        let p = progress.next();
        let status = match p {
            None => Status::CurrentUntyped,
            Some(c) => match c {
                c if c == tc => Status::CurrentCorrect,
                _ => Status::CurrentIncorrect,
            },
        };

        if status == cur_status {
            cur_string.push(tc);
        } else {
            if !cur_string.is_empty() {
                parts.push((cur_string, cur_status));
                cur_string = String::new();
            }
            cur_string.push(tc);
            cur_status = status;

            // first currentuntyped is cursor
            if status == Status::CurrentUntyped {
                parts.push((cur_string, Status::Cursor));
                cur_string = String::new();
            }
        }
    }
    if !cur_string.is_empty() {
        parts.push((cur_string, cur_status));
    }
    let overtyped = progress.collect::<String>();
    if !overtyped.is_empty() {
        parts.push((overtyped, Status::Overtyped));
    }
    parts
}

fn split_typed_word(word: &TestWord) -> Vec<(String, Status)> {
    let mut parts = Vec::new();
    let mut cur_string = String::new();
    let mut cur_status = Status::Untyped;

    let mut progress = word.progress.chars();
    for tc in word.text.chars() {
        let p = progress.next();
        let status = match p {
            None => Status::Untyped,
            Some(c) => match c {
                c if c == tc => Status::Correct,
                _ => Status::Incorrect,
            },
        };

        if status == cur_status {
            cur_string.push(tc);
        } else {
            if !cur_string.is_empty() {
                parts.push((cur_string, cur_status));
                cur_string = String::new();
            }
            cur_string.push(tc);
            cur_status = status;
        }
    }
    if !cur_string.is_empty() {
        parts.push((cur_string, cur_status));
    }

    let overtyped = progress.collect::<String>();
    if !overtyped.is_empty() {
        parts.push((overtyped, Status::Overtyped));
    }
    parts
}

fn word_parts_to_spans(
    parts: Vec<(String, Status)>,
    theme: &Theme,
    effects: TestRenderEffects,
    apply_chaos: bool,
    visual_index: &mut usize,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (text, status) in parts {
        let style = match status {
            Status::Correct => theme.prompt_correct,
            Status::Incorrect => theme.prompt_incorrect,
            Status::Untyped => theme.prompt_untyped,
            Status::CurrentUntyped => theme.prompt_current_untyped,
            Status::CurrentCorrect => theme.prompt_current_correct,
            Status::CurrentIncorrect => theme.prompt_current_incorrect,
            Status::Cursor => theme.prompt_current_untyped.patch(theme.prompt_cursor),
            Status::Overtyped => theme.prompt_incorrect,
        };

        let text = if apply_chaos && is_untyped_status(status) {
            decorate_untyped_text(&text, effects, visual_index)
        } else {
            *visual_index += text.chars().count();
            text
        };

        spans.push(Span::styled(text, style));
    }
    spans.push(Span::styled(" ", theme.prompt_untyped));
    spans
}

fn is_untyped_status(status: Status) -> bool {
    matches!(
        status,
        Status::CurrentUntyped | Status::Cursor | Status::Untyped
    )
}

fn decorate_untyped_text(
    text: &str,
    effects: TestRenderEffects,
    visual_index: &mut usize,
) -> String {
    const HAUNTED_CHARS: [char; 4] = ['░', '▒', '▓', '█'];

    let mut decorated = String::new();
    let ghost_rate = 20 + effects.flicker_seed % 11;

    for character in text.chars() {
        let index = *visual_index;
        if effects.haunted_mode && chance(effects.flicker_seed, index, 17, 12) {
            let ghost_index =
                (mix(effects.flicker_seed ^ index as u64 ^ 0xCAFE) as usize) % HAUNTED_CHARS.len();
            decorated.push(HAUNTED_CHARS[ghost_index]);
        }

        if effects.ghost_mode && chance(effects.flicker_seed, index, 29, ghost_rate) {
            decorated.push(' ');
        } else {
            decorated.push(character);
        }

        *visual_index += 1;
    }

    decorated
}

fn mirror_words(words: Vec<Vec<Span<'static>>>) -> Vec<Vec<Span<'static>>> {
    words
        .into_iter()
        .rev()
        .map(|word| {
            word.into_iter()
                .rev()
                .map(|span| {
                    Span::styled(
                        span.content.as_ref().chars().rev().collect::<String>(),
                        span.style,
                    )
                })
                .collect()
        })
        .collect()
}

fn chance(seed: u64, index: usize, salt: u64, percent: u64) -> bool {
    mix(seed ^ (index as u64).wrapping_mul(0x9E37) ^ salt.wrapping_mul(0x51)) % 100 < percent
}

fn mix(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

impl ThemedWidget for &results::Results {
    fn render(self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        buf.set_style(area, theme.default);

        // Chunks
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);
        let res_chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1) // Graph looks tremendously better with just a little margin
            .constraints([Constraint::Ratio(1, 3), Constraint::Ratio(2, 3)])
            .split(chunks[0]);
        let info_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
            .split(res_chunks[0]);

        let msg = if self.race_progress.is_some() {
            "Press 'q' to quit"
        } else if self.missed_words.is_empty() {
            "Press 'q' to quit or 'r' for another test"
        } else {
            "Press 'q' to quit, 'r' for another test or 'p' to practice missed words"
        };

        let exit = Span::styled(msg, theme.results_restart_prompt);
        buf.set_span(chunks[1].x, chunks[1].y, &exit, chunks[1].width);

        // Sections
        let mut overview_text = Text::styled("", theme.results_overview);
        overview_text.extend([
            Line::from(format!(
                "Adjusted WPM: {:.1}",
                self.timing.overall_cps * WPM_PER_CPS * f64::from(self.accuracy.overall)
            )),
            Line::from(format!(
                "Accuracy: {:.1}%",
                f64::from(self.accuracy.overall) * 100f64
            )),
            Line::from(format!(
                "Raw WPM: {:.1}",
                self.timing.overall_cps * WPM_PER_CPS
            )),
            Line::from(format!("Correct Keypresses: {}", self.accuracy.overall)),
        ]);
        if let Some(race) = &self.race_progress {
            let message = race.message.clone().unwrap_or_else(|| match race.outcome {
                Some(RaceOutcome::Win) => "Race: You win".into(),
                Some(RaceOutcome::Lose) => "Race: You lose".into(),
                Some(RaceOutcome::Tie) => "Race: Tie".into(),
                Some(RaceOutcome::Disconnected) => "Race: Opponent disconnected".into(),
                None => "Race: Complete".into(),
            });
            overview_text.extend([Line::from(message)]);
        }
        let overview = Paragraph::new(overview_text).block(
            Block::default()
                .title(Span::styled("Overview", theme.title))
                .borders(Borders::ALL)
                .border_type(theme.border_type)
                .border_style(theme.results_overview_border),
        );
        overview.render(info_chunks[0], buf);

        let mut worst_keys: Vec<(&KeyEvent, &Fraction)> = self
            .accuracy
            .per_key
            .iter()
            .filter(|(key, _)| matches!(key.code, KeyCode::Char(_)))
            .collect();
        worst_keys.sort_unstable_by_key(|x| x.1);

        let mut worst_text = Text::styled("", theme.results_worst_keys);
        worst_text.extend(
            worst_keys
                .iter()
                .filter_map(|(key, acc)| {
                    if let KeyCode::Char(character) = key.code {
                        let key_accuracy = f64::from(**acc) * 100.0;
                        if key_accuracy != 100.0 {
                            Some(format!("- {} at {:.1}% accuracy", character, key_accuracy))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .take(5)
                .map(Line::from),
        );
        let worst = Paragraph::new(worst_text).block(
            Block::default()
                .title(Span::styled("Worst Keys", theme.title))
                .borders(Borders::ALL)
                .border_type(theme.border_type)
                .border_style(theme.results_worst_keys_border),
        );
        worst.render(info_chunks[1], buf);

        let wpm_sma: Vec<(f64, f64)> = self
            .timing
            .per_event
            .windows(WPM_SMA_WIDTH)
            .enumerate()
            .map(|(i, window)| {
                (
                    (i + WPM_SMA_WIDTH) as f64,
                    window.len() as f64 / window.iter().copied().sum::<f64>() * WPM_PER_CPS,
                )
            })
            .collect();

        // Render the chart if possible
        if !wpm_sma.is_empty() {
            let wpm_sma_min = wpm_sma
                .iter()
                .map(|(_, x)| x)
                .fold(f64::INFINITY, |a, &b| a.min(b));
            let wpm_sma_max = wpm_sma
                .iter()
                .map(|(_, x)| x)
                .fold(f64::NEG_INFINITY, |a, &b| a.max(b));

            let wpm_datasets = vec![Dataset::default()
                .name("WPM")
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(theme.results_chart)
                .data(&wpm_sma)];

            let y_label_min = wpm_sma_min as u16;
            let y_label_max = (wpm_sma_max as u16).max(y_label_min + 6);

            let wpm_chart = Chart::new(wpm_datasets)
                .block(Block::default().title(vec![Span::styled("Chart", theme.title)]))
                .x_axis(
                    Axis::default()
                        .title(Span::styled("Keypresses", theme.results_chart_x))
                        .bounds([0.0, self.timing.per_event.len() as f64]),
                )
                .y_axis(
                    Axis::default()
                        .title(Span::styled(
                            "WPM (10-keypress rolling average)",
                            theme.results_chart_y,
                        ))
                        .bounds([wpm_sma_min, wpm_sma_max])
                        .labels(
                            (y_label_min..y_label_max)
                                .step_by(5)
                                .map(|n| Span::raw(format!("{}", n)))
                                .collect(),
                        ),
                );
            wpm_chart.render(res_chunks[1], buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod split_words {
        use super::Status::*;
        use super::*;

        struct TestCase {
            word: &'static str,
            progress: &'static str,
            expected: Vec<(&'static str, Status)>,
        }

        fn setup(test_case: TestCase) -> (TestWord, Vec<(String, Status)>) {
            let mut word = TestWord::from(test_case.word);
            word.progress = test_case.progress.to_string();

            let expected = test_case
                .expected
                .iter()
                .map(|(s, v)| (s.to_string(), *v))
                .collect::<Vec<_>>();

            (word, expected)
        }

        #[test]
        fn typed_words_split() {
            let cases = vec![
                TestCase {
                    word: "monkeytype",
                    progress: "monkeytype",
                    expected: vec![("monkeytype", Correct)],
                },
                TestCase {
                    word: "monkeytype",
                    progress: "monkeXtype",
                    expected: vec![("monke", Correct), ("y", Incorrect), ("type", Correct)],
                },
                TestCase {
                    word: "monkeytype",
                    progress: "monkeas",
                    expected: vec![("monke", Correct), ("yt", Incorrect), ("ype", Untyped)],
                },
            ];

            for case in cases {
                let (word, expected) = setup(case);
                let got = split_typed_word(&word);
                assert_eq!(got, expected);
            }
        }

        #[test]
        fn current_word_split() {
            let cases = vec![
                TestCase {
                    word: "monkeytype",
                    progress: "monkeytype",
                    expected: vec![("monkeytype", CurrentCorrect)],
                },
                TestCase {
                    word: "monkeytype",
                    progress: "monke",
                    expected: vec![
                        ("monke", CurrentCorrect),
                        ("y", Cursor),
                        ("type", CurrentUntyped),
                    ],
                },
                TestCase {
                    word: "monkeytype",
                    progress: "monkeXt",
                    expected: vec![
                        ("monke", CurrentCorrect),
                        ("y", CurrentIncorrect),
                        ("t", CurrentCorrect),
                        ("y", Cursor),
                        ("pe", CurrentUntyped),
                    ],
                },
            ];

            for case in cases {
                let (word, expected) = setup(case);
                let got = split_current_word(&word);
                assert_eq!(got, expected);
            }
        }
    }
}
