use crate::{
    config::{Theme, THEME_NAMES},
    ui::ThemedWidget,
};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use rand::Rng;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Widget},
};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub const TIME_LIMITS: [Option<u64>; 5] = [None, Some(15), Some(30), Some(60), Some(120)];
pub const WORD_COUNTS: [usize; 5] = [10, 25, 50, 100, 200];
pub const MIN_WORD_LENGTHS: [Option<usize>; 5] = [None, Some(3), Some(4), Some(5), Some(6)];
pub const MAX_WORD_LENGTHS: [Option<usize>; 4] = [None, Some(6), Some(8), Some(10)];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub sudden_death: bool,
    pub no_backtrack: bool,
    pub no_backspace: bool,
    pub time_limit: Option<u64>,
    pub word_count: usize,
    pub punctuation: bool,
    pub numbers: bool,
    pub min_word_length: Option<usize>,
    pub max_word_length: Option<usize>,
    pub host_race: bool,
    pub race_address: String,
    pub theme: String,
    pub language: String,
    pub chaos_rainbow_mode: bool,
    pub chaos_seizure_mode: bool,
    pub chaos_disco_mode: bool,
    pub chaos_drunk_mode: bool,
    pub chaos_tiny_mode: bool,
    pub chaos_mirror_mode: bool,
    pub chaos_ghost_mode: bool,
    pub chaos_earthquake_mode: bool,
    pub chaos_speed_demon_mode: bool,
    pub chaos_haunted_mode: bool,
    pub chaos_blackout_mode: bool,
    pub chaos_neon_mode: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            sudden_death: false,
            no_backtrack: false,
            no_backspace: false,
            time_limit: None,
            word_count: 50,
            punctuation: false,
            numbers: false,
            min_word_length: None,
            max_word_length: None,
            host_race: false,
            race_address: String::new(),
            theme: "Default".into(),
            language: "english200".into(),
            chaos_rainbow_mode: false,
            chaos_seizure_mode: false,
            chaos_disco_mode: false,
            chaos_drunk_mode: false,
            chaos_tiny_mode: false,
            chaos_mirror_mode: false,
            chaos_ghost_mode: false,
            chaos_earthquake_mode: false,
            chaos_speed_demon_mode: false,
            chaos_haunted_mode: false,
            chaos_blackout_mode: false,
            chaos_neon_mode: false,
        }
    }
}

impl Settings {
    pub fn load_or_default(
        path: &Path,
        default_language: &str,
        languages: &[String],
    ) -> io::Result<Self> {
        let mut settings = match fs::read_to_string(path) {
            Ok(contents) => serde_json::from_str(&contents)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self {
                language: default_language.into(),
                ..Default::default()
            },
            Err(error) => return Err(error),
        };
        settings.normalize(default_language, languages);
        Ok(settings)
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let contents = serde_json::to_string_pretty(self)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        fs::write(path, contents)
    }

    pub fn normalize(&mut self, default_language: &str, languages: &[String]) {
        if !TIME_LIMITS.contains(&self.time_limit) {
            self.time_limit = None;
        }
        if !WORD_COUNTS.contains(&self.word_count) {
            self.word_count = 50;
        }
        if !MIN_WORD_LENGTHS.contains(&self.min_word_length) {
            self.min_word_length = None;
        }
        if !MAX_WORD_LENGTHS.contains(&self.max_word_length) {
            self.max_word_length = None;
        }
        if !THEME_NAMES.contains(&self.theme.as_str()) {
            self.theme = "Default".into();
        }
        if !languages.iter().any(|language| language == &self.language) {
            self.language = if languages
                .iter()
                .any(|language| language == default_language)
            {
                default_language.into()
            } else {
                languages
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "english200".into())
            };
        }
    }

    pub fn enabled_chaos_count(&self) -> usize {
        [
            self.chaos_rainbow_mode,
            self.chaos_seizure_mode,
            self.chaos_disco_mode,
            self.chaos_drunk_mode,
            self.chaos_tiny_mode,
            self.chaos_mirror_mode,
            self.chaos_ghost_mode,
            self.chaos_earthquake_mode,
            self.chaos_speed_demon_mode,
            self.chaos_haunted_mode,
            self.chaos_blackout_mode,
            self.chaos_neon_mode,
        ]
        .into_iter()
        .filter(|enabled| *enabled)
        .count()
    }
}

#[derive(Clone, Debug, Default)]
pub struct SettingsScreen {
    selected: usize,
    editing_race_address: bool,
}

pub struct SettingsView<'a> {
    pub screen: &'a SettingsScreen,
    pub settings: &'a Settings,
    pub languages: &'a [String],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsAction {
    None,
    Close,
    Changed,
}

#[derive(Clone, Copy)]
enum SettingItem {
    SuddenDeath,
    NoBacktrack,
    NoBackspace,
    TimeLimit,
    WordCount,
    Punctuation,
    Numbers,
    MinWordLength,
    MaxWordLength,
    HostRace,
    RaceAddress,
    Theme,
    ChaosRainbowMode,
    ChaosSeizureMode,
    ChaosDiscoMode,
    ChaosDrunkMode,
    ChaosTinyMode,
    ChaosMirrorMode,
    ChaosGhostMode,
    ChaosEarthquakeMode,
    ChaosSpeedDemonMode,
    ChaosHauntedMode,
    ChaosBlackoutMode,
    ChaosNeonMode,
    Language,
}

#[derive(Clone, Copy)]
enum Step {
    Previous,
    Next,
}

const SELECTABLE_ITEMS: [SettingItem; 25] = [
    SettingItem::SuddenDeath,
    SettingItem::NoBacktrack,
    SettingItem::NoBackspace,
    SettingItem::TimeLimit,
    SettingItem::WordCount,
    SettingItem::Punctuation,
    SettingItem::Numbers,
    SettingItem::MinWordLength,
    SettingItem::MaxWordLength,
    SettingItem::HostRace,
    SettingItem::RaceAddress,
    SettingItem::Theme,
    SettingItem::ChaosRainbowMode,
    SettingItem::ChaosSeizureMode,
    SettingItem::ChaosDiscoMode,
    SettingItem::ChaosDrunkMode,
    SettingItem::ChaosTinyMode,
    SettingItem::ChaosMirrorMode,
    SettingItem::ChaosGhostMode,
    SettingItem::ChaosEarthquakeMode,
    SettingItem::ChaosSpeedDemonMode,
    SettingItem::ChaosHauntedMode,
    SettingItem::ChaosBlackoutMode,
    SettingItem::ChaosNeonMode,
    SettingItem::Language,
];

impl SettingsScreen {
    pub fn handle_key(
        &mut self,
        key: KeyEvent,
        settings: &mut Settings,
        languages: &[String],
    ) -> SettingsAction {
        if key.kind != KeyEventKind::Press {
            return SettingsAction::None;
        }

        if self.editing_race_address {
            return self.handle_race_address_key(key, settings);
        }

        match key.code {
            KeyCode::Char('s') | KeyCode::Char('S')
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                SettingsAction::Close
            }
            KeyCode::Esc => SettingsAction::Close,
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                SettingsAction::None
            }
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(SELECTABLE_ITEMS.len() - 1);
                SettingsAction::None
            }
            KeyCode::Char(' ') => self.toggle_selected(settings),
            KeyCode::Enter => self.activate_selected(settings),
            KeyCode::Left => self.change_selected(settings, languages, Step::Previous),
            KeyCode::Right => self.change_selected(settings, languages, Step::Next),
            _ => SettingsAction::None,
        }
    }

    fn handle_race_address_key(
        &mut self,
        key: KeyEvent,
        settings: &mut Settings,
    ) -> SettingsAction {
        match key.code {
            KeyCode::Enter | KeyCode::Esc => {
                self.editing_race_address = false;
                SettingsAction::None
            }
            KeyCode::Backspace => {
                settings.race_address.pop();
                SettingsAction::Changed
            }
            KeyCode::Char(character)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                settings.race_address.push(character);
                SettingsAction::Changed
            }
            _ => SettingsAction::None,
        }
    }

    fn activate_selected(&mut self, settings: &mut Settings) -> SettingsAction {
        match self.selected_item() {
            SettingItem::SuddenDeath => toggle(&mut settings.sudden_death),
            SettingItem::NoBacktrack => toggle(&mut settings.no_backtrack),
            SettingItem::NoBackspace => toggle(&mut settings.no_backspace),
            SettingItem::Punctuation => toggle(&mut settings.punctuation),
            SettingItem::Numbers => toggle(&mut settings.numbers),
            SettingItem::HostRace => toggle(&mut settings.host_race),
            SettingItem::ChaosRainbowMode => toggle(&mut settings.chaos_rainbow_mode),
            SettingItem::ChaosSeizureMode => toggle(&mut settings.chaos_seizure_mode),
            SettingItem::ChaosDiscoMode => toggle(&mut settings.chaos_disco_mode),
            SettingItem::ChaosDrunkMode => toggle(&mut settings.chaos_drunk_mode),
            SettingItem::ChaosTinyMode => toggle(&mut settings.chaos_tiny_mode),
            SettingItem::ChaosMirrorMode => toggle(&mut settings.chaos_mirror_mode),
            SettingItem::ChaosGhostMode => toggle(&mut settings.chaos_ghost_mode),
            SettingItem::ChaosEarthquakeMode => toggle(&mut settings.chaos_earthquake_mode),
            SettingItem::ChaosSpeedDemonMode => toggle(&mut settings.chaos_speed_demon_mode),
            SettingItem::ChaosHauntedMode => toggle(&mut settings.chaos_haunted_mode),
            SettingItem::ChaosBlackoutMode => toggle(&mut settings.chaos_blackout_mode),
            SettingItem::ChaosNeonMode => toggle(&mut settings.chaos_neon_mode),
            SettingItem::RaceAddress => {
                self.editing_race_address = true;
                SettingsAction::None
            }
            _ => SettingsAction::None,
        }
    }

    fn toggle_selected(&mut self, settings: &mut Settings) -> SettingsAction {
        match self.selected_item() {
            SettingItem::SuddenDeath
            | SettingItem::NoBacktrack
            | SettingItem::NoBackspace
            | SettingItem::Punctuation
            | SettingItem::Numbers
            | SettingItem::HostRace
            | SettingItem::ChaosRainbowMode
            | SettingItem::ChaosSeizureMode
            | SettingItem::ChaosDiscoMode
            | SettingItem::ChaosDrunkMode
            | SettingItem::ChaosTinyMode
            | SettingItem::ChaosMirrorMode
            | SettingItem::ChaosGhostMode
            | SettingItem::ChaosEarthquakeMode
            | SettingItem::ChaosSpeedDemonMode
            | SettingItem::ChaosHauntedMode
            | SettingItem::ChaosBlackoutMode
            | SettingItem::ChaosNeonMode => self.activate_selected(settings),
            _ => SettingsAction::None,
        }
    }

    fn change_selected(
        &mut self,
        settings: &mut Settings,
        languages: &[String],
        step: Step,
    ) -> SettingsAction {
        match self.selected_item() {
            SettingItem::TimeLimit => {
                settings.time_limit = cycle_copy(settings.time_limit, &TIME_LIMITS, step);
                SettingsAction::Changed
            }
            SettingItem::WordCount => {
                settings.word_count = cycle_copy(settings.word_count, &WORD_COUNTS, step);
                SettingsAction::Changed
            }
            SettingItem::MinWordLength => {
                settings.min_word_length =
                    cycle_copy(settings.min_word_length, &MIN_WORD_LENGTHS, step);
                SettingsAction::Changed
            }
            SettingItem::MaxWordLength => {
                settings.max_word_length =
                    cycle_copy(settings.max_word_length, &MAX_WORD_LENGTHS, step);
                SettingsAction::Changed
            }
            SettingItem::Theme => {
                settings.theme = cycle_str(&settings.theme, &THEME_NAMES, step);
                SettingsAction::Changed
            }
            SettingItem::Language if !languages.is_empty() => {
                settings.language = cycle_string(&settings.language, languages, step);
                SettingsAction::Changed
            }
            _ => SettingsAction::None,
        }
    }

    fn selected_item(&self) -> SettingItem {
        SELECTABLE_ITEMS[self.selected]
    }
}

fn toggle(value: &mut bool) -> SettingsAction {
    *value = !*value;
    SettingsAction::Changed
}

fn cycle_copy<T: Copy + PartialEq>(current: T, options: &[T], step: Step) -> T {
    let index = options
        .iter()
        .position(|option| option == &current)
        .unwrap_or_default();
    let next = match step {
        Step::Previous => index.checked_sub(1).unwrap_or(options.len() - 1),
        Step::Next => (index + 1) % options.len(),
    };
    options[next]
}

fn cycle_str(current: &str, options: &[&str], step: Step) -> String {
    let index = options
        .iter()
        .position(|option| option == &current)
        .unwrap_or_default();
    let next = match step {
        Step::Previous => index.checked_sub(1).unwrap_or(options.len() - 1),
        Step::Next => (index + 1) % options.len(),
    };
    options[next].into()
}

fn cycle_string(current: &str, options: &[String], step: Step) -> String {
    let index = options
        .iter()
        .position(|option| option == current)
        .unwrap_or_default();
    let next = match step {
        Step::Previous => index.checked_sub(1).unwrap_or(options.len() - 1),
        Step::Next => (index + 1) % options.len(),
    };
    options[next].clone()
}

impl ThemedWidget for SettingsView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer, theme: &Theme) {
        buf.set_style(area, theme.default);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(area);

        buf.set_line(
            chunks[0].x,
            chunks[0].y,
            &Line::from(Span::styled("Settings (S or Esc to close)", theme.title)),
            chunks[0].width,
        );

        let body = Block::default()
            .borders(Borders::ALL)
            .border_type(theme.border_type)
            .border_style(theme.input_border);
        let body_inner = body.inner(chunks[1]);
        body.render(chunks[1], buf);

        let (lines, selected_line) =
            settings_lines(self.screen, self.settings, self.languages, theme);
        let scroll = selected_line.saturating_sub(body_inner.height.saturating_sub(1) as usize);
        Paragraph::new(Text::from(lines))
            .scroll((scroll as u16, 0))
            .render(body_inner, buf);

        buf.set_line(
            chunks[2].x,
            chunks[2].y,
            &Line::from(Span::styled(
                "Arrow keys to navigate · Space/Enter to toggle · Left/Right to change selection",
                theme.results_restart_prompt,
            )),
            chunks[2].width,
        );
    }
}

fn settings_lines(
    screen: &SettingsScreen,
    settings: &Settings,
    languages: &[String],
    theme: &Theme,
) -> (Vec<Line<'static>>, usize) {
    let mut lines = Vec::new();
    let mut selectable = 0usize;
    let mut selected_line = 0usize;

    push_section(&mut lines, "TEST", theme);
    push_item(
        &mut lines,
        format!("{} Sudden Death", checkbox(settings.sudden_death)),
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );
    push_item(
        &mut lines,
        format!("{} No Backtrack", checkbox(settings.no_backtrack)),
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );
    push_item(
        &mut lines,
        format!("{} No Backspace", checkbox(settings.no_backspace)),
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );
    push_item(
        &mut lines,
        format!("Time Limit: < {} >", time_limit_label(settings.time_limit)),
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );
    push_item(
        &mut lines,
        format!("Word Count: < {} >", settings.word_count),
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );

    push_section(&mut lines, "DIFFICULTY", theme);
    push_item(
        &mut lines,
        format!("{} Punctuation", checkbox(settings.punctuation)),
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );
    push_item(
        &mut lines,
        format!("{} Numbers", checkbox(settings.numbers)),
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );
    push_item(
        &mut lines,
        format!(
            "Min Word Length: < {} >",
            word_length_label(settings.min_word_length, "Off")
        ),
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );
    push_item(
        &mut lines,
        format!(
            "Max Word Length: < {} >",
            word_length_label(settings.max_word_length, "Any")
        ),
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );

    push_section(&mut lines, "RACE", theme);
    push_item(
        &mut lines,
        format!("{} Host Race", checkbox(settings.host_race)),
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );
    let race_address = if screen.editing_race_address {
        format!("Race Address: [{}|]", settings.race_address)
    } else {
        format!("Race Address: [{}]", settings.race_address)
    };
    push_item(
        &mut lines,
        race_address,
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );

    push_section(&mut lines, "DISPLAY", theme);
    push_item(
        &mut lines,
        format!("Theme: < {} >", settings.theme),
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );

    push_section(&mut lines, "CHAOS (!)", theme);
    if settings.enabled_chaos_count() > 3 {
        lines.push(chaos_warning_line());
    }
    push_item(
        &mut lines,
        format!("{} Rainbow Mode", checkbox(settings.chaos_rainbow_mode)),
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );
    push_item(
        &mut lines,
        format!("{} Seizure Mode", checkbox(settings.chaos_seizure_mode)),
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );
    push_item(
        &mut lines,
        format!("{} Disco Mode", checkbox(settings.chaos_disco_mode)),
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );
    push_item(
        &mut lines,
        format!("{} Drunk Mode", checkbox(settings.chaos_drunk_mode)),
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );
    push_item(
        &mut lines,
        format!("{} Tiny Mode", checkbox(settings.chaos_tiny_mode)),
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );
    push_item(
        &mut lines,
        format!("{} Mirror Mode", checkbox(settings.chaos_mirror_mode)),
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );
    push_item(
        &mut lines,
        format!("{} Ghost Mode", checkbox(settings.chaos_ghost_mode)),
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );
    push_item(
        &mut lines,
        format!(
            "{} Earthquake Mode",
            checkbox(settings.chaos_earthquake_mode)
        ),
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );
    push_item(
        &mut lines,
        format!(
            "{} Speed Demon Mode",
            checkbox(settings.chaos_speed_demon_mode)
        ),
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );
    push_item(
        &mut lines,
        format!("{} Haunted Mode", checkbox(settings.chaos_haunted_mode)),
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );
    push_item(
        &mut lines,
        format!("{} Blackout Mode", checkbox(settings.chaos_blackout_mode)),
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );
    push_item(
        &mut lines,
        format!("{} Neon Mode", checkbox(settings.chaos_neon_mode)),
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );

    push_section(&mut lines, "LANGUAGE", theme);
    let language = if languages.is_empty() {
        "Language: < none >".into()
    } else {
        format!("Language: < {} >", settings.language)
    };
    push_item(
        &mut lines,
        language,
        &mut selectable,
        &mut selected_line,
        screen,
        theme,
    );

    (lines, selected_line)
}

fn push_section(lines: &mut Vec<Line<'static>>, label: &'static str, theme: &Theme) {
    lines.push(Line::from(Span::styled(label, theme.title)));
}

fn chaos_warning_line() -> Line<'static> {
    let mut rng = rand::thread_rng();
    let spans = "WARNING: YOU ASKED FOR THIS"
        .chars()
        .map(|character| {
            Span::styled(
                character.to_string(),
                Style::default()
                    .fg(Color::Rgb(rng.gen(), rng.gen(), rng.gen()))
                    .add_modifier(Modifier::BOLD | Modifier::RAPID_BLINK),
            )
        })
        .collect::<Vec<_>>();

    Line::from(spans)
}

fn push_item(
    lines: &mut Vec<Line<'static>>,
    label: String,
    selectable: &mut usize,
    selected_line: &mut usize,
    screen: &SettingsScreen,
    theme: &Theme,
) {
    let selected = screen.selected == *selectable;
    if selected {
        *selected_line = lines.len();
    }
    *selectable += 1;

    lines.push(Line::from(Span::styled(
        label,
        item_style(selected, screen.editing_race_address, theme),
    )));
}

fn item_style(selected: bool, editing_race_address: bool, theme: &Theme) -> Style {
    if selected && editing_race_address {
        theme
            .prompt_current_correct
            .add_modifier(Modifier::REVERSED)
    } else if selected {
        theme
            .prompt_current_untyped
            .add_modifier(Modifier::REVERSED)
    } else {
        theme.default
    }
}

fn checkbox(enabled: bool) -> &'static str {
    if enabled {
        "[x]"
    } else {
        "[ ]"
    }
}

fn time_limit_label(value: Option<u64>) -> String {
    value
        .map(|seconds| format!("{seconds}s"))
        .unwrap_or_else(|| "Off".into())
}

fn word_length_label(value: Option<usize>, disabled: &'static str) -> String {
    value
        .map(|length| length.to_string())
        .unwrap_or_else(|| disabled.into())
}

pub fn settings_path(config_dir: PathBuf) -> PathBuf {
    config_dir.join("settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chaos_defaults_are_disabled() {
        assert_eq!(Settings::default().enabled_chaos_count(), 0);
    }

    #[test]
    fn enabled_chaos_count_tracks_independent_toggles() {
        let settings = Settings {
            chaos_rainbow_mode: true,
            chaos_disco_mode: true,
            chaos_neon_mode: true,
            ..Default::default()
        };

        assert_eq!(settings.enabled_chaos_count(), 3);
    }

    #[test]
    fn settings_lines_adds_warning_after_four_chaos_modes() {
        let screen = SettingsScreen::default();
        let settings = Settings {
            chaos_rainbow_mode: true,
            chaos_seizure_mode: true,
            chaos_disco_mode: true,
            chaos_drunk_mode: true,
            ..Default::default()
        };

        let (lines, _) = settings_lines(&screen, &settings, &[], &Theme::default());
        let text = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        assert!(text.iter().any(|line| line == "CHAOS (!)"));
        assert!(text
            .iter()
            .any(|line| line == "WARNING: YOU ASKED FOR THIS"));
    }
}
