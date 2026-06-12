use crate::{
    config::Theme,
    gameplay::{GameplayFeature, WordKind},
};

use super::test::{results, RaceOutcome, Test, TestWord};

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
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
pub const POWER_BURST_LIFETIME_MS: u128 = 650;

const POWER_PARTICLE_DIRECTIONS: [(i16, i16); 16] = [
    (0, -1),
    (1, -1),
    (2, -1),
    (2, 0),
    (2, 1),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-2, 1),
    (-2, 0),
    (-2, -1),
    (-1, -1),
    (0, -2),
    (1, -2),
    (0, 2),
    (-1, 2),
];
const POWER_PARTICLE_SYMBOLS: [&str; 6] = ["*", "+", "x", ".", "#", "o"];
const POWER_PARTICLE_COLORS: [Color; 6] = [
    Color::Rgb(0xff, 0xd7, 0x00),
    Color::Rgb(0xff, 0x66, 0x00),
    Color::Rgb(0xff, 0x2d, 0x55),
    Color::Rgb(0x00, 0xd7, 0xff),
    Color::Rgb(0x7c, 0xff, 0x4f),
    Color::Rgb(0xff, 0x7a, 0xff),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PowerBurst {
    pub seed: u64,
    pub started_at: Instant,
    pub combo: usize,
}

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
    pub power_burst: Option<PowerBurst>,
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
            power_burst: None,
        }
    }
}

pub struct TestView<'a> {
    pub test: &'a Test,
    pub effects: TestRenderEffects,
}

pub struct RaceLobbyView<'a> {
    pub room_code: &'a str,
    pub public_addr: &'a str,
    pub invite_command: &'a str,
    pub status: &'a str,
    pub spinner: &'a str,
    pub cancel_label: &'a str,
    pub start_label: &'a str,
    pub copy_hint: &'a str,
    pub error: Option<&'a str>,
}

impl ThemedWidget for RaceLobbyView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        buf.set_style(area, theme.default);

        let lobby_area = centered_rect(area, 52, 16);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type)
            .border_style(theme.input_border);
        let inner = block.inner(lobby_area);
        block.render(lobby_area, buf);

        let status = if let Some(error) = self.error {
            error.to_string()
        } else if self.spinner.is_empty() {
            self.status.to_string()
        } else {
            format!("{} {}", self.status, self.spinner)
        };
        let status_style = if self.error.is_some() {
            theme.prompt_incorrect
        } else {
            theme.results_overview
        };

        // copy_hint: flash green when "✓ Copied!", otherwise dimmed
        let copy_style = if self.copy_hint.starts_with('\u{2713}') {
            theme.prompt_current_correct
        } else {
            theme.results_restart_prompt
        };

        let mut lines = vec![
            Line::from(Span::styled("RACE LOBBY - LOCAL HOST", theme.title)),
            Line::from(""),
            Line::from(vec![
                Span::styled("Room code : ", theme.results_overview),
                Span::styled(self.room_code.to_string(), theme.results_timer),
            ]),
            Line::from(vec![
                Span::styled("Address   : ", theme.results_overview),
                Span::raw(self.public_addr.to_string()),
            ]),
            Line::from("Share this command with your friend:"),
            Line::from(""),
            Line::from(Span::styled(
                self.invite_command.to_string(),
                theme.prompt_current_untyped,
            )),
            Line::from(Span::styled(self.copy_hint.to_string(), copy_style)),
            Line::from(""),
            Line::from(Span::styled(status, status_style)),
        ];

        if !self.start_label.is_empty() {
            lines.push(Line::from(Span::styled(
                self.start_label.to_string(),
                theme.prompt_current_correct,
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            self.cancel_label.to_string(),
            theme.results_restart_prompt,
        )));

        Paragraph::new(Text::from(lines)).render(inner, buf);
    }
}

pub struct JoinRaceLobbyView<'a> {
    pub status: &'a str,
    pub spinner: &'a str,
}

impl ThemedWidget for JoinRaceLobbyView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        buf.set_style(area, theme.default);

        let lobby_area = centered_rect(area, 52, 10);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type)
            .border_style(theme.input_border);
        let inner = block.inner(lobby_area);
        block.render(lobby_area, buf);

        let status = if self.spinner.is_empty() {
            self.status.to_string()
        } else {
            format!("{} {}", self.status, self.spinner)
        };

        let lines = Text::from(vec![
            Line::from(Span::styled("RACE LOBBY - JOINED", theme.title)),
            Line::from(""),
            Line::from(Span::styled(status, theme.results_overview)),
            Line::from(""),
            Line::from("Wait for the host to start the race..."),
            Line::from(""),
            Line::from(Span::styled(
                "Press Esc to leave",
                theme.results_restart_prompt,
            )),
        ]);

        Paragraph::new(lines).render(inner, buf);
    }
}

pub struct JoinRaceView<'a> {
    pub input: &'a str,
    pub error: Option<&'a str>,
}

impl ThemedWidget for JoinRaceView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        buf.set_style(area, theme.default);

        let join_area = centered_rect(area, 52, 15);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type)
            .border_style(theme.input_border);
        let inner = block.inner(join_area);
        block.render(join_area, buf);

        let mut lines = vec![
            Line::from(Span::styled("JOIN A RACE", theme.title)),
            Line::from(""),
            Line::from("Enter the connection string from the host:"),
            Line::from(Span::styled(
                "  IP:PORT#CODE  or just  IP#CODE",
                theme.results_overview,
            )),
            Line::from(Span::styled(
                "  e.g. 192.168.1.5:7878#1234",
                theme.prompt_untyped,
            )),
            Line::from(""),
            Line::from(Span::styled(
                format!("> {}_", self.input),
                theme.prompt_current_untyped,
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Press Enter to connect",
                theme.results_overview,
            )),
            Line::from(Span::styled(
                "Press Esc to go back",
                theme.results_restart_prompt,
            )),
        ];

        if let Some(error) = self.error {
            lines.push(Line::from(Span::styled(
                error.to_string(),
                theme.prompt_incorrect,
            )));
        }

        Paragraph::new(Text::from(lines)).render(inner, buf);
    }
}

pub struct JoiningRaceView<'a> {
    pub addr: &'a str,
    pub room_code: &'a str,
    pub spinner: &'a str,
}

impl ThemedWidget for JoiningRaceView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        buf.set_style(area, theme.default);

        let joining_area = centered_rect(area, 49, 10);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type)
            .border_style(theme.input_border);
        let inner = block.inner(joining_area);
        block.render(joining_area, buf);

        let lines = Text::from(vec![
            Line::from(Span::styled("JOINING RACE", theme.title)),
            Line::from(""),
            Line::from(Span::styled(
                format!("Connecting to {}... {}", self.addr, self.spinner),
                theme.results_overview,
            )),
            Line::from(vec![
                Span::styled("Room code: ", theme.results_overview),
                Span::styled(self.room_code.to_string(), theme.results_timer),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Press Esc to cancel",
                theme.results_restart_prompt,
            )),
        ]);

        Paragraph::new(lines).render(inner, buf);
    }
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);

    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
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
        let now = Instant::now();
        if let Some(wpm) = test.live_wpm_at(now) {
            input_title.push(Span::raw(" "));
            input_title.push(Span::styled(
                format!("WPM {:.1}", wpm),
                theme.results_overview,
            ));
        }
        let remaining = if let Some(elapsed) = effects.accelerated_elapsed {
            test.time_remaining_after_elapsed(elapsed)
        } else if effects.time_multiplier * test.visual_elapsed_multiplier() <= 1.0 {
            test.time_remaining_at(now)
        } else {
            test.time_remaining_at_with_multiplier(
                now,
                effects.time_multiplier * test.visual_elapsed_multiplier(),
            )
        };
        if let Some(remaining) = remaining {
            let remaining_seconds = remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0);
            input_title.push(Span::raw(" "));
            input_title.push(Span::styled(
                format!("Time {}s", remaining_seconds),
                theme.results_timer,
            ));
        }
        for part in test.gameplay_status_parts(now) {
            input_title.push(Span::raw(" "));
            input_title.push(Span::styled(part, theme.results_timer));
        }
        if let Some(tag) = &test.rank_tag {
            input_title.push(Span::raw(" "));
            input_title.push(Span::styled(tag.clone(), theme.results_overview));
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
                let words = words_to_spans(test, theme, effects, now);

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

        render_power_mode_effect(test, effects.power_burst, area, buf, now);
    }
}

fn render_power_mode_effect(
    test: &Test,
    burst: Option<PowerBurst>,
    bounds: Rect,
    buf: &mut Buffer,
    now: Instant,
) {
    if !test.feature_enabled(GameplayFeature::PowerMode) || bounds.width < 6 || bounds.height < 4 {
        return;
    }

    let Some(burst) = burst else {
        return;
    };

    let age_ms = now.saturating_duration_since(burst.started_at).as_millis();
    if age_ms >= POWER_BURST_LIFETIME_MS {
        return;
    }

    let (origin_x, origin_y) = power_burst_origin(burst.seed, bounds);

    draw_power_particles(burst, origin_x, origin_y, bounds, buf, age_ms);
    draw_power_combo_label(burst, origin_x, origin_y, bounds, buf, age_ms);
}

fn power_burst_origin(seed: u64, bounds: Rect) -> (u16, u16) {
    let safe_x = bounds.x.saturating_add(1);
    let safe_y = bounds.y.saturating_add(1);
    let safe_width = bounds.width.saturating_sub(2).max(1);
    let safe_height = bounds.height.saturating_sub(2).max(1);
    let x = safe_x + (mix(seed) % u64::from(safe_width)) as u16;
    let y = safe_y + (mix(seed.rotate_left(17)) % u64::from(safe_height)) as u16;

    (x, y)
}

fn draw_power_combo_label(
    burst: PowerBurst,
    origin_x: u16,
    origin_y: u16,
    bounds: Rect,
    buf: &mut Buffer,
    age_ms: u128,
) {
    let label = format!("{}x", burst.combo);
    let label_width = Line::from(label.as_str()).width() as u16;
    let min_start_x = bounds.x.saturating_add(1);
    let max_start_x = bounds.right().saturating_sub(1 + label_width);
    let label_x = if max_start_x <= min_start_x {
        min_start_x
    } else {
        origin_x
            .saturating_sub(label_width / 2)
            .clamp(min_start_x, max_start_x)
    };
    let float = (age_ms / 180) as u16;
    let label_y = origin_y.saturating_sub(float).clamp(
        bounds.y.saturating_add(1),
        bounds.bottom().saturating_sub(2),
    );
    let color = POWER_PARTICLE_COLORS[particle_index(
        burst.seed.rotate_left(11),
        burst.combo,
        POWER_PARTICLE_COLORS.len(),
    )];
    let style = Style::default().fg(color).add_modifier(Modifier::BOLD);
    let span = Span::styled(label, style);
    let max_width = bounds.right().saturating_sub(1).saturating_sub(label_x);

    buf.set_span(label_x, label_y, &span, max_width);
}

fn draw_power_particles(
    burst: PowerBurst,
    origin_x: u16,
    origin_y: u16,
    bounds: Rect,
    buf: &mut Buffer,
    age_ms: u128,
) {
    let particle_count = (8 + burst.combo.min(8)).min(POWER_PARTICLE_DIRECTIONS.len());
    let radius = 1 + (age_ms / 120) as i16;
    let vertical_radius = (radius / 2).max(1);

    for index in 0..particle_count {
        let direction_index = particle_index(burst.seed, index, POWER_PARTICLE_DIRECTIONS.len());
        let (dx, dy) = POWER_PARTICLE_DIRECTIONS[direction_index];
        let horizontal = radius + (index % 3) as i16;
        let vertical = vertical_radius + (index % 2) as i16;
        let x = origin_x as i16 + dx.signum() * horizontal + dx / 2;
        let y = origin_y as i16 + dy.signum() * vertical;

        if x < 0 || y < 0 || !rect_contains(bounds, x as u16, y as u16) {
            continue;
        }

        let symbol = POWER_PARTICLE_SYMBOLS[particle_index(
            burst.seed ^ age_ms as u64,
            index,
            POWER_PARTICLE_SYMBOLS.len(),
        )];
        let color = POWER_PARTICLE_COLORS[particle_index(
            burst.seed.rotate_left(7),
            index,
            POWER_PARTICLE_COLORS.len(),
        )];
        let style = Style::default().fg(color).add_modifier(Modifier::BOLD);
        buf.get_mut(x as u16, y as u16)
            .set_symbol(symbol)
            .set_style(style);
    }
}

fn particle_index(seed: u64, index: usize, len: usize) -> usize {
    (mix(seed ^ (index as u64).wrapping_mul(0x9E37_79B9)) as usize) % len
}

fn rect_contains(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x && x < rect.right() && y >= rect.y && y < rect.bottom()
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
            "{label:<8} {}{} {:>3}%",
            "█".repeat(filled),
            "░".repeat(empty),
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
    test: &Test,
    theme: &Theme,
    effects: TestRenderEffects,
    now: Instant,
) -> Vec<Vec<Span<'static>>> {
    let mut spans = Vec::new();
    let mut visual_index = 0usize;
    let current_word = test.current_word;
    let mut word_indices: Vec<usize> = (0..test.words.len()).collect();

    if test.feature_enabled(GameplayFeature::WordSwapTrick)
        && test
            .started_at
            .is_some_and(|started| now.saturating_duration_since(started).as_secs() % 6 >= 3)
    {
        for pair_start in ((current_word + 1)..word_indices.len()).step_by(2) {
            if pair_start + 1 < word_indices.len() {
                word_indices.swap(pair_start, pair_start + 1);
            }
        }
    }

    for word_index in word_indices {
        if test.feature_enabled(GameplayFeature::OneWordAtATime) && word_index != current_word {
            continue;
        }

        let word = &test.words[word_index];
        let parts = if word_index < current_word {
            if test.feature_enabled(GameplayFeature::NegativeSpaceMode) {
                vec![(" ".repeat(word.prompt().chars().count()), Status::Untyped)]
            } else {
                split_typed_word(
                    word,
                    test.feature_enabled(GameplayFeature::DisappearingText),
                )
            }
        } else if word_index == current_word {
            split_current_word(
                word,
                test.feature_enabled(GameplayFeature::DisappearingText),
            )
        } else {
            vec![(word.prompt().to_string(), Status::Untyped)]
        };
        spans.push(word_parts_to_spans(
            parts,
            word.kind,
            theme,
            effects,
            word_index >= current_word,
            &mut visual_index,
        ));
    }

    if effects.mirror_prompt || test.feature_enabled(GameplayFeature::ReverseSentence) {
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

fn split_current_word(word: &TestWord, disappearing_text: bool) -> Vec<(String, Status)> {
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
        let shown = if disappearing_text && status == Status::CurrentCorrect {
            ' '
        } else {
            tc
        };

        if status == cur_status {
            cur_string.push(shown);
        } else {
            if !cur_string.is_empty() {
                parts.push((cur_string, cur_status));
                cur_string = String::new();
            }
            cur_string.push(shown);
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

fn split_typed_word(word: &TestWord, disappearing_text: bool) -> Vec<(String, Status)> {
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
        let shown = if disappearing_text && status == Status::Correct {
            ' '
        } else {
            tc
        };

        if status == cur_status {
            cur_string.push(shown);
        } else {
            if !cur_string.is_empty() {
                parts.push((cur_string, cur_status));
                cur_string = String::new();
            }
            cur_string.push(shown);
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
    kind: WordKind,
    theme: &Theme,
    effects: TestRenderEffects,
    apply_chaos: bool,
    visual_index: &mut usize,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (text, status) in parts {
        let mut style = match status {
            Status::Correct => theme.prompt_correct,
            Status::Incorrect => theme.prompt_incorrect,
            Status::Untyped => theme.prompt_untyped,
            Status::CurrentUntyped => theme.prompt_current_untyped,
            Status::CurrentCorrect => theme.prompt_current_correct,
            Status::CurrentIncorrect => theme.prompt_current_incorrect,
            Status::Cursor => theme.prompt_current_untyped.patch(theme.prompt_cursor),
            Status::Overtyped => theme.prompt_incorrect,
        };
        style = match kind {
            WordKind::Penalty => style.patch(theme.prompt_incorrect),
            WordKind::Bonus => style.patch(theme.results_timer),
            WordKind::DoublePoints | WordKind::Boss | WordKind::ComboBreaker => {
                style.patch(theme.results_overview)
            }
            WordKind::Normal => style,
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
            Line::from(format!("Adjusted WPM: {:.1}", self.adjusted_wpm())),
            Line::from(format!(
                "Accuracy: {:.1}%",
                f64::from(self.accuracy.overall) * 100f64
            )),
            Line::from(format!(
                "Raw WPM: {:.1}",
                self.timing.overall_cps * WPM_PER_CPS
            )),
            Line::from(format!(
                "Gameplay Multiplier: x{:.2}",
                self.gameplay_multiplier
            )),
            Line::from(format!("Correct Keypresses: {}", self.accuracy.overall)),
        ]);
        overview_text.extend(self.gameplay_summary.iter().cloned().map(Line::from));
        if let Some(banner) = &self.rank_banner {
            overview_text.extend([Line::from(banner.progress_line.clone())]);
            if !banner.message.is_empty() {
                overview_text.extend([Line::from(Span::styled(
                    banner.message.clone(),
                    theme.results_timer,
                ))]);
            }
        }
        if let Some(race) = &self.race_progress {
            let message = race.message.clone().unwrap_or_else(|| match race.outcome {
                Some(RaceOutcome::Win) => "You won!".into(),
                Some(RaceOutcome::Lose) => "Opponent won!".into(),
                Some(RaceOutcome::Tie) => "Tie!".into(),
                Some(RaceOutcome::Disconnected) => "Race: Opponent disconnected".into(),
                None => "Race: Complete".into(),
            });
            overview_text.extend([Line::from(message)]);
            if race.you_wpm.is_some() || race.opponent_wpm.is_some() {
                overview_text.extend([Line::from(format!(
                    "Your WPM: {:.0}  |  Opponent WPM: {:.0}",
                    race.you_wpm.unwrap_or_default(),
                    race.opponent_wpm.unwrap_or_default()
                ))]);
            }
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

    #[test]
    fn power_burst_origin_stays_inside_screen_bounds() {
        let bounds = Rect::new(10, 5, 40, 12);
        let (x, y) = power_burst_origin(12345, bounds);

        assert!(x > bounds.x);
        assert!(x < bounds.right().saturating_sub(1));
        assert!(y > bounds.y);
        assert!(y < bounds.bottom().saturating_sub(1));
    }

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
                let got = split_typed_word(&word, false);
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
                let got = split_current_word(&word, false);
                assert_eq!(got, expected);
            }
        }
    }
}
