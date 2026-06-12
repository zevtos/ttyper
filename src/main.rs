mod chaos;
mod config;
mod gameplay;
mod history;
mod race;
mod rank;
mod settings;
mod test;
mod tunnel;
mod ui;

use chaos::ChaosState;
use config::{theme_by_name, Config, Theme};
use gameplay::{feature_set, GameplayFeature, PreparedWord, ALL_GAMEPLAY_FEATURES};
use race::{LobbyEvent, RaceEvent, RaceSession};
use settings::{settings_path, Settings, SettingsAction, SettingsScreen, SettingsView};
use test::{results::Results, RaceOutcome, Test};

use clap::{parser::ValueSource, ArgMatches, CommandFactory, FromArgMatches, Parser, Subcommand};
use clap_complete::{generate, Shell};
use crossterm::{
    self, cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute, terminal,
};
use rand::{seq::SliceRandom, thread_rng, Rng};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    terminal::Terminal,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
};
use rust_embed::RustEmbed;
use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs,
    io::{self, BufRead},
    num,
    path::{Path, PathBuf},
    str,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
    time::{Duration, Instant},
};

const TIME_MODE_WORD_COUNT: usize = 10_000;
const DEFAULT_RACE_ADDR: &str = "127.0.0.1:7878";
const JOIN_RACE_ERROR_MESSAGE: &str =
    "Could not connect. Check the connection string and try again.";
const PUNCTUATION_CHANCE: f64 = 0.2;
const NUMBER_CHANCE: f64 = 0.15;
const PUNCTUATION_MARKS: [char; 4] = ['.', ',', '!', '?'];

#[derive(RustEmbed)]
#[folder = "resources/runtime"]
struct Resources;

/// Loads rank-corpus resources from the user's language dir (overrides) or
/// the embedded resources.
struct EmbeddedLoader {
    language_dir: PathBuf,
}

impl rank::generate::ResourceLoader for EmbeddedLoader {
    fn load_words(&self, resource: &str) -> Option<Vec<String>> {
        let bytes = resource
            .strip_prefix("language/")
            .and_then(|name| fs::read(self.language_dir.join(name)).ok())
            .or_else(|| Resources::get(resource).map(|file| file.data.into_owned()))?;
        let text = str::from_utf8(&bytes).ok()?;
        Some(
            text.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(ToOwned::to_owned)
                .collect(),
        )
    }
}

#[derive(Clone, Debug, Parser)]
#[command(about, version)]
struct Opt {
    /// Read test contents from the specified file, or "-" for stdin
    #[arg(value_name = "PATH")]
    contents: Option<PathBuf>,

    #[arg(short, long)]
    debug: bool,

    /// Specify word count
    #[arg(short, long, value_name = "N", default_value = "50")]
    words: num::NonZeroUsize,

    /// Use time-based mode with the specified countdown duration
    #[arg(long, value_name = "SECONDS", conflicts_with = "race")]
    time: Option<num::NonZeroU64>,

    /// Filter out words shorter than the specified length
    #[arg(long, value_name = "N")]
    min_length: Option<num::NonZeroUsize>,

    /// Filter out words longer than the specified length
    #[arg(long, value_name = "N")]
    max_length: Option<num::NonZeroUsize>,

    /// Randomly append punctuation to some words
    #[arg(long)]
    punctuation: bool,

    /// Randomly insert standalone number tokens between words
    #[arg(long)]
    numbers: bool,

    /// Host a race or connect to the specified race host and room code
    #[arg(
        long,
        value_name = "HOST:PORT#CODE",
        num_args = 0..=1
    )]
    race: Option<Option<String>>,

    /// Use config file
    #[arg(short, long, value_name = "PATH")]
    config: Option<PathBuf>,

    /// Specify test language in file
    #[arg(long, value_name = "PATH")]
    language_file: Option<PathBuf>,

    /// Specify test language
    #[arg(short, long, value_name = "LANG")]
    language: Option<String>,

    /// List installed languages
    #[arg(long)]
    list_languages: bool,

    /// Disable backtracking to completed words
    #[arg(long)]
    no_backtrack: bool,

    /// Enable sudden death mode to restart on first error
    #[arg(long)]
    sudden_death: bool,

    /// Phoenix Protocol: one mistake burns the test and a fresh word set
    /// rises from the ashes (mutually exclusive with --sudden-death)
    #[arg(long, conflicts_with = "sudden_death")]
    phoenix: bool,

    /// Disable backspace
    #[arg(long)]
    no_backspace: bool,

    /// Enable a gameplay feature by kebab-case name; repeat for multiple features
    #[arg(long = "feature", value_enum, value_name = "FEATURE")]
    gameplay_features: Vec<GameplayFeature>,

    /// Enable every gameplay feature, including power mode
    #[arg(long)]
    all_gameplay_features: bool,

    /// Practice a typing rank (G lowest .. S highest)
    #[arg(long, value_enum, ignore_case = true, value_name = "RANK")]
    rank: Option<rank::Rank>,

    /// Practice a specific level (1..=10) within the rank
    #[arg(long, value_name = "N")]
    level: Option<u32>,

    /// Print rank/level progress and exit
    #[arg(long)]
    rank_status: bool,

    /// Resolved rank session; computed, never parsed from the CLI.
    #[clap(skip)]
    rank_session: Option<rank::RankSession>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Clone, Debug, Subcommand)]
enum Command {
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
}

#[derive(Debug, Default)]
struct CliOverrides {
    words: bool,
    time: bool,
    min_length: bool,
    max_length: bool,
    punctuation: bool,
    numbers: bool,
    race: bool,
    language: bool,
    no_backtrack: bool,
    sudden_death: bool,
    phoenix: bool,
    no_backspace: bool,
    gameplay_features: bool,
    all_gameplay_features: bool,
}

impl CliOverrides {
    fn from_matches(matches: &ArgMatches) -> Self {
        Self {
            words: is_command_line_arg(matches, "words"),
            time: is_command_line_arg(matches, "time"),
            min_length: is_command_line_arg(matches, "min_length"),
            max_length: is_command_line_arg(matches, "max_length"),
            punctuation: is_command_line_arg(matches, "punctuation"),
            numbers: is_command_line_arg(matches, "numbers"),
            race: is_command_line_arg(matches, "race"),
            language: is_command_line_arg(matches, "language"),
            no_backtrack: is_command_line_arg(matches, "no_backtrack"),
            sudden_death: is_command_line_arg(matches, "sudden_death"),
            phoenix: is_command_line_arg(matches, "phoenix"),
            no_backspace: is_command_line_arg(matches, "no_backspace"),
            gameplay_features: is_command_line_arg(matches, "gameplay_features"),
            all_gameplay_features: is_command_line_arg(matches, "all_gameplay_features"),
        }
    }
}

fn is_command_line_arg(matches: &ArgMatches, id: &str) -> bool {
    matches.value_source(id) == Some(ValueSource::CommandLine)
}

impl Opt {
    fn effective(&self, settings: &Settings, overrides: &CliOverrides) -> Self {
        let mut effective = self.clone();

        if !overrides.words {
            effective.words = num::NonZeroUsize::new(settings.word_count)
                .expect("settings word count should be non-zero");
        }
        if !overrides.time {
            effective.time = settings.time_limit.and_then(num::NonZeroU64::new);
        }
        if !overrides.min_length {
            effective.min_length = settings.min_word_length.and_then(num::NonZeroUsize::new);
        }
        if !overrides.max_length {
            effective.max_length = settings.max_word_length.and_then(num::NonZeroUsize::new);
        }
        if !overrides.punctuation {
            effective.punctuation = settings.punctuation;
        }
        if !overrides.numbers {
            effective.numbers = settings.numbers;
        }
        if !overrides.race {
            effective.race = settings_race(settings);
        }
        if !overrides.language {
            effective.language = Some(settings.language.clone());
        }
        if !overrides.no_backtrack {
            effective.no_backtrack = settings.no_backtrack;
        }
        if !overrides.sudden_death {
            effective.sudden_death = settings.sudden_death;
        }
        if !overrides.phoenix {
            effective.phoenix = settings.phoenix;
        }
        // Mutually exclusive; an explicit CLI choice wins over saved settings.
        if effective.phoenix && effective.sudden_death {
            if overrides.sudden_death {
                effective.phoenix = false;
            } else {
                effective.sudden_death = false;
            }
        }
        if !overrides.no_backspace {
            effective.no_backspace = settings.no_backspace;
        }
        if overrides.all_gameplay_features || effective.all_gameplay_features {
            effective.gameplay_features = ALL_GAMEPLAY_FEATURES.to_vec();
        } else if !overrides.gameplay_features {
            effective.gameplay_features = settings.gameplay_features.clone();
        }

        effective
    }

    fn gen_contents(&self) -> Result<Vec<String>, String> {
        // Rank corpus, unless an explicit corpus (file/stdin/--language) wins.
        if self.contents.is_none() && self.language_file.is_none() {
            if let Some(session) = self.rank_session.as_ref().filter(|s| s.use_rank_corpus) {
                let loader = EmbeddedLoader {
                    language_dir: self.language_dir(),
                };
                let mut rng = thread_rng();
                match rank::generate::generate_rank_corpus(
                    &session.spec,
                    self.generated_word_count(),
                    &loader,
                    &mut rng,
                ) {
                    Ok(mut words) => {
                        // Explicit CLI difficulty flags layer on top of the
                        // rank corpus (and already made the session
                        // non-qualifying at resolution time).
                        if session.cli_punctuation {
                            for word in &mut words {
                                if rng.gen_bool(PUNCTUATION_CHANCE) {
                                    if let Some(mark) = PUNCTUATION_MARKS.choose(&mut rng) {
                                        word.push(*mark);
                                    }
                                }
                            }
                        }
                        if session.cli_numbers {
                            let mut with_numbers = Vec::with_capacity(words.len());
                            for word in words {
                                with_numbers.push(word);
                                if rng.gen_bool(NUMBER_CHANCE) {
                                    with_numbers.push(rng.gen_range(0..100).to_string());
                                }
                            }
                            words = with_numbers;
                        }
                        return Ok(words);
                    }
                    Err(error) => {
                        // Never crash on a missing rank resource: fall back
                        // to the legacy language path below.
                        if self.debug {
                            eprintln!(
                                "[ttyper] rank corpus failed ({error}); using language fallback"
                            );
                        }
                    }
                }
            }
        }

        match &self.contents {
            Some(path) => {
                let contents: Vec<String> = if path.as_os_str() == "-" {
                    std::io::stdin()
                        .lock()
                        .lines()
                        .map_while(Result::ok)
                        .collect()
                } else {
                    let file = fs::File::open(path).expect("Error reading language file.");
                    io::BufReader::new(file)
                        .lines()
                        .map_while(Result::ok)
                        .collect()
                };

                let contents = self.filter_word_pool(contents)?;
                let contents = if self.time.is_some() && !contents.is_empty() {
                    contents
                        .iter()
                        .cloned()
                        .cycle()
                        .take(TIME_MODE_WORD_COUNT)
                        .collect()
                } else {
                    contents
                };

                Ok(self.apply_word_transformations(contents, &mut thread_rng()))
            }
            None => {
                let lang_name = self
                    .language
                    .clone()
                    .unwrap_or_else(|| self.config().default_language);

                let bytes: Vec<u8> = self
                    .language_file
                    .as_ref()
                    .map(fs::read)
                    .and_then(Result::ok)
                    .or_else(|| fs::read(self.language_dir().join(&lang_name)).ok())
                    .or_else(|| {
                        Resources::get(&format!("language/{}", &lang_name))
                            .map(|f| f.data.into_owned())
                    })
                    .ok_or_else(|| {
                        format!("couldn't find language '{lang_name}'. Use --list-languages to see installed languages.")
                    })?;

                let mut rng = thread_rng();

                let mut language: Vec<String> = str::from_utf8(&bytes)
                    .expect("Language file had non-utf8 encoding.")
                    .lines()
                    .map(ToOwned::to_owned)
                    .collect();
                language = self.filter_word_pool(language)?;
                language.shuffle(&mut rng);

                let mut contents: Vec<_> = language
                    .into_iter()
                    .cycle()
                    .take(self.generated_word_count())
                    .collect();
                contents.shuffle(&mut rng);

                Ok(self.apply_word_transformations(contents, &mut rng))
            }
        }
    }

    /// Applies min/max length filters and reports empty filtered pools.
    fn filter_word_pool(&self, words: Vec<String>) -> Result<Vec<String>, String> {
        let min_length = self.min_length.map(num::NonZeroUsize::get);
        let max_length = self.max_length.map(num::NonZeroUsize::get);

        if let (Some(min), Some(max)) = (min_length, max_length) {
            if min > max {
                return Err(format!(
                    "--min-length ({min}) cannot be greater than --max-length ({max})."
                ));
            }
        }

        if words.is_empty() {
            return Ok(words);
        }

        let filtered: Vec<String> = words
            .into_iter()
            .filter(|word| {
                let len = word.chars().count();
                min_length.is_none_or(|min| len >= min) && max_length.is_none_or(|max| len <= max)
            })
            .collect();

        if filtered.is_empty() {
            return Err(
                "word length filters removed every word; adjust --min-length or --max-length."
                    .into(),
            );
        }

        Ok(filtered)
    }

    /// Applies optional punctuation and standalone number token transformations.
    fn apply_word_transformations<R: Rng + ?Sized>(
        &self,
        words: Vec<String>,
        rng: &mut R,
    ) -> Vec<String> {
        let punctuated: Vec<String> = words
            .into_iter()
            .map(|mut word| {
                if self.punctuation && rng.gen_bool(PUNCTUATION_CHANCE) {
                    if let Some(mark) = PUNCTUATION_MARKS.choose(rng) {
                        word.push(*mark);
                    }
                }
                word
            })
            .collect();

        if !self.numbers {
            return punctuated;
        }

        let mut with_numbers = Vec::new();
        for word in punctuated {
            with_numbers.push(word);
            if rng.gen_bool(NUMBER_CHANCE) {
                with_numbers.push(rng.gen_range(0..100).to_string());
            }
        }

        with_numbers
    }

    /// Number of generated words for the selected test mode.
    fn generated_word_count(&self) -> usize {
        if self.time.is_some()
            || self
                .gameplay_features
                .contains(&GameplayFeature::EnduranceMode)
        {
            TIME_MODE_WORD_COUNT
        } else {
            self.words.get()
        }
    }

    /// Countdown duration for timed mode.
    fn time_limit(&self) -> Option<Duration> {
        self.time.map(|seconds| Duration::from_secs(seconds.get()))
    }

    /// Returns true when this process should host a race server.
    fn is_race_host(&self) -> bool {
        matches!(self.race, Some(None))
    }

    /// Returns true when this process should connect as a race client.
    fn is_race_client(&self) -> bool {
        matches!(self.race, Some(Some(_)))
    }

    /// Returns the host bind address or client target address for race mode.
    fn race_addr(&self) -> Option<&str> {
        match &self.race {
            Some(None) => Some(DEFAULT_RACE_ADDR),
            Some(Some(addr)) => Some(addr.as_str()),
            None => None,
        }
    }

    /// Configuration
    fn config(&self) -> Config {
        fs::read(
            self.config
                .clone()
                .unwrap_or_else(|| self.config_dir().join("config.toml")),
        )
        .map(|bytes| {
            toml::from_str(str::from_utf8(&bytes).unwrap_or_default())
                .expect("Configuration was ill-formed.")
        })
        .unwrap_or_default()
    }

    /// Installed languages under config directory
    fn languages(&self) -> io::Result<impl Iterator<Item = OsString>> {
        let builtin = Resources::iter().filter_map(|name| {
            name.strip_prefix("language/")
                .map(ToOwned::to_owned)
                .map(OsString::from)
        });

        let configured = self
            .language_dir()
            .read_dir()
            .into_iter()
            .flatten()
            .map_while(Result::ok)
            .map(|e| e.file_name());

        Ok(builtin.chain(configured))
    }

    /// Config directory
    fn config_dir(&self) -> PathBuf {
        dirs::config_dir()
            .expect("Failed to find config directory.")
            .join("ttyper")
    }

    /// Language directory under config directory
    fn language_dir(&self) -> PathBuf {
        self.config_dir().join("language")
    }
}

enum State {
    Welcome,
    Settings {
        screen: SettingsScreen,
        previous: Box<State>,
    },
    RaceLobbyStarting {
        receiver: Receiver<HostSetupEvent>,
        test: Option<Test>,
        started_at: Instant,
        error: Option<String>,
    },
    RaceLobby {
        test: Test,
        started_at: Instant,
        copied_at: Option<Instant>,
    },
    JoinRace {
        input: String,
        error: Option<String>,
    },
    JoinRaceConnecting {
        input: String,
        invite: race::RaceInvite,
        receiver: Receiver<JoinRaceEvent>,
        started_at: Instant,
    },
    JoinRaceLobby {
        test: Test,
        started_at: Instant,
    },
    Test(Test),
    Results(Results),
}

enum HostSetupEvent {
    Ready(race::HostLobby),
    Failed(String),
}

enum JoinRaceEvent {
    Connected(Vec<String>, RaceSession),
    Failed(String),
}

impl State {
    #[allow(clippy::too_many_arguments)]
    fn render_into<B: ratatui::backend::Backend>(
        &self,
        terminal: &mut Terminal<B>,
        config: &Config,
        settings: &Settings,
        languages: &[String],
        chaos: &ChaosState,
        race_session: &Option<RaceSession>,
        host_lobby: &Option<race::HostLobby>,
        welcome_rank_line: Option<&str>,
    ) -> io::Result<()> {
        let theme = chaos.apply_theme(&config.theme, settings);
        match self {
            State::Welcome => {
                terminal.draw(|f| {
                    let area = chaos.earthquake_area(f.size(), settings);
                    f.render_widget(
                        theme.apply_to(Welcome {
                            rank_line: welcome_rank_line.map(str::to_string),
                        }),
                        area,
                    );
                })?;
            }
            State::Settings { screen, .. } => {
                terminal.draw(|f| {
                    let area = chaos.earthquake_area(f.size(), settings);
                    f.render_widget(
                        theme.apply_to(SettingsView {
                            screen,
                            settings,
                            languages,
                        }),
                        area,
                    );
                })?;
            }
            State::RaceLobbyStarting {
                started_at, error, ..
            } => {
                terminal.draw(|f| {
                    let area = chaos.earthquake_area(f.size(), settings);
                    f.render_widget(
                        theme.apply_to(ui::RaceLobbyView {
                            room_code: "----",
                            public_addr: "Binding local port...",
                            invite_command: "Generating invite command...",
                            status: "Setting up local race lobby...",
                            spinner: spinner_at(*started_at),
                            cancel_label: "Press Esc to cancel and go back",
                            start_label: "",
                            copy_hint: "",
                            error: error.as_deref(),
                        }),
                        area,
                    );
                })?;
            }
            State::RaceLobby {
                started_at,
                copied_at,
                ..
            } => {
                let Some(lobby) = host_lobby else {
                    return Ok(());
                };
                let copy_label = copied_at
                    .filter(|t| t.elapsed().as_secs() < 3)
                    .map(|_| "✓ Copied!")
                    .unwrap_or("Press C to copy");
                let (status, start_label) = if race_session.is_some() {
                    ("Opponent connected!", "Press S to start the race")
                } else {
                    ("Waiting for opponent to connect...", "")
                };
                terminal.draw(|f| {
                    let area = chaos.earthquake_area(f.size(), settings);
                    f.render_widget(
                        theme.apply_to(ui::RaceLobbyView {
                            room_code: lobby.room_code(),
                            public_addr: lobby.public_addr(),
                            invite_command: lobby.invite_command(),
                            status,
                            spinner: spinner_at(*started_at),
                            cancel_label: "Press Esc to cancel and go back",
                            start_label,
                            copy_hint: copy_label,
                            error: None,
                        }),
                        area,
                    );
                })?;
            }
            State::JoinRace { input, error } => {
                terminal.draw(|f| {
                    let area = chaos.earthquake_area(f.size(), settings);
                    f.render_widget(
                        theme.apply_to(ui::JoinRaceView {
                            input,
                            error: error.as_deref(),
                        }),
                        area,
                    );
                })?;
            }
            State::JoinRaceConnecting {
                invite, started_at, ..
            } => {
                terminal.draw(|f| {
                    let area = chaos.earthquake_area(f.size(), settings);
                    f.render_widget(
                        theme.apply_to(ui::JoiningRaceView {
                            addr: &invite.addr,
                            room_code: &invite.room_code,
                            spinner: spinner_at(*started_at),
                        }),
                        area,
                    );
                })?;
            }
            State::JoinRaceLobby { started_at, .. } => {
                terminal.draw(|f| {
                    let area = chaos.earthquake_area(f.size(), settings);
                    f.render_widget(
                        theme.apply_to(ui::JoinRaceLobbyView {
                            status: "Joined race lobby!",
                            spinner: spinner_at(*started_at),
                        }),
                        area,
                    );
                })?;
            }
            State::Test(test) => {
                terminal.draw(|f| {
                    let area = chaos.tiny_area(chaos.earthquake_area(f.size(), settings), settings);
                    let effects = chaos.test_effects(settings, test, Instant::now());
                    f.render_widget(theme.apply_to(ui::TestView { test, effects }), area);

                    // Position cursor at end of input for IME composition support
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Length(3), Constraint::Length(6)])
                        .split(area);
                    let inner_x = chunks[0].x + 1;
                    let inner_y = chunks[0].y + 1;
                    let progress_width =
                        ratatui::text::Line::from(test.words[test.current_word].progress.as_str())
                            .width() as u16;
                    let max_cursor_x = chunks[0].right().saturating_sub(2);
                    f.set_cursor((inner_x + progress_width).min(max_cursor_x), inner_y);
                })?;
            }
            State::Results(results) => {
                terminal.draw(|f| {
                    let area = chaos.earthquake_area(f.size(), settings);
                    f.render_widget(theme.apply_to(results), area);
                })?;
            }
        }
        Ok(())
    }
}

struct Welcome {
    rank_line: Option<String>,
}

struct TerminalCleanup {
    active: bool,
}

impl TerminalCleanup {
    fn active() -> Self {
        Self { active: true }
    }

    fn finish(&mut self) -> io::Result<()> {
        if self.active {
            self.active = false;
            terminal::disable_raw_mode()?;
            execute!(
                io::stdout(),
                cursor::RestorePosition,
                cursor::Show,
                terminal::LeaveAlternateScreen,
            )?;
        }
        Ok(())
    }
}

impl Drop for TerminalCleanup {
    fn drop(&mut self) {
        if self.active {
            let _ = terminal::disable_raw_mode();
            let _ = execute!(
                io::stdout(),
                cursor::RestorePosition,
                cursor::Show,
                terminal::LeaveAlternateScreen,
            );
        }
    }
}

impl ui::ThemedWidget for Welcome {
    fn render(self, area: ratatui::layout::Rect, buf: &mut ratatui::buffer::Buffer, theme: &Theme) {
        buf.set_style(area, theme.default);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1)])
            .margin(1)
            .split(area);

        let mut lines = vec![
            Line::from(Span::styled("ttyper", theme.title)),
            Line::from(""),
        ];
        if let Some(rank_line) = &self.rank_line {
            lines.push(Line::from(Span::styled(
                rank_line.clone(),
                theme.results_overview,
            )));
            lines.push(Line::from(""));
        }
        lines.extend(vec![
            Line::from(Span::styled("Press Enter to start", theme.results_overview)),
            Line::from(Span::styled(
                "Press R to host race",
                theme.prompt_current_untyped,
            )),
            Line::from(Span::styled(
                "Press J to join race",
                theme.prompt_current_untyped,
            )),
            Line::from(Span::styled(
                "Press S for settings",
                theme.prompt_current_untyped,
            )),
            Line::from(Span::styled(
                "Press q to quit",
                theme.results_restart_prompt,
            )),
        ]);
        let content = Text::from(lines);

        let welcome = Paragraph::new(content).block(
            Block::default()
                .title(Span::styled("Welcome", theme.title))
                .borders(Borders::ALL)
                .border_type(theme.border_type)
                .border_style(theme.input_border),
        );
        ratatui::widgets::Widget::render(welcome, chunks[0], buf);
    }
}

/// Computes the effective options and resolves the rank session for this
/// launch. The rank session prescribes the word count unless `-w` was given.
fn effective_with_rank(opt: &Opt, settings: &Settings, overrides: &CliOverrides) -> Opt {
    let mut effective = opt.effective(settings, overrides);

    // Without an explicit -w the level prescribes its word count, so saved
    // settings don't silently disqualify rank sessions.
    let word_count_for_gate = if overrides.words {
        effective.words.get()
    } else {
        rank::ladder::WORD_COUNT_MIN
    };
    let session = rank::resolve_session(
        effective.rank,
        effective.level,
        &settings.rank_profile,
        &rank::SessionOverrides {
            custom_corpus: effective.contents.is_some() || effective.language_file.is_some(),
            language_override: overrides.language,
            punctuation_override: overrides.punctuation,
            numbers_override: overrides.numbers,
            length_override: overrides.min_length || overrides.max_length,
            time_mode: effective.time.is_some(),
            race: effective.race.is_some(),
            gameplay_features: !effective.gameplay_features.is_empty(),
            word_count: word_count_for_gate,
        },
    );

    if let Some(session) = &session {
        if session.use_rank_corpus && !overrides.words {
            effective.words = num::NonZeroUsize::new(session.spec.word_count_min)
                .expect("prescribed word count is non-zero");
        }
    }
    effective.rank_session = session;
    effective
}

/// Test-start context for the history record, derived from the effective
/// options. Race mode is stamped at test end from the live test state.
fn build_session_meta(opt: &Opt, corpus_kind_override: Option<&str>) -> history::SessionMeta {
    let rank_session = opt.rank_session.as_ref();
    let corpus = if let Some(kind) = corpus_kind_override {
        history::Corpus {
            kind: kind.into(),
            name: None,
            language: None,
        }
    } else if let Some(path) = &opt.contents {
        if path.as_os_str() == "-" {
            history::Corpus {
                kind: "stdin".into(),
                name: None,
                language: None,
            }
        } else {
            history::Corpus {
                kind: "file".into(),
                name: path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned()),
                language: None,
            }
        }
    } else if let Some(path) = &opt.language_file {
        history::Corpus {
            kind: "file".into(),
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned()),
            language: None,
        }
    } else if let Some(session) = rank_session.filter(|session| session.use_rank_corpus) {
        history::Corpus {
            kind: "rank".into(),
            name: Some(session.spec.id.key()),
            language: None,
        }
    } else {
        let language = opt.language.clone();
        history::Corpus {
            kind: "language".into(),
            name: language.clone(),
            language,
        }
    };

    let mode = if opt.time.is_some() { "time" } else { "words" };
    history::SessionMeta {
        mode: mode.into(),
        time_limit_secs: opt.time.map(num::NonZeroU64::get),
        word_count_requested: (opt.time.is_none()).then(|| opt.words.get() as u32),
        corpus,
        rank: rank_session.map(|session| session.spec.id.rank.as_str().to_string()),
        level: rank_session.map(|session| session.spec.id.level),
        qualifying: rank_session.is_some_and(|session| session.qualifying),
        phoenix: opt.phoenix,
        chaos_modes: Vec::new(),
    }
}

/// Welcome-screen summary of the rank session a plain Enter would start.
fn rank_welcome_line(opt: &Opt, settings: &Settings, overrides: &CliOverrides) -> Option<String> {
    let effective = effective_with_rank(opt, settings, overrides);
    let session = effective.rank_session?;
    Some(rank::welcome_line(&settings.rank_profile, &session))
}

/// Chaos modes are display-only and recorded for completeness.
fn chaos_mode_names(settings: &Settings) -> Vec<String> {
    [
        ("rainbow", settings.chaos_rainbow_mode),
        ("seizure", settings.chaos_seizure_mode),
        ("disco", settings.chaos_disco_mode),
        ("drunk", settings.chaos_drunk_mode),
        ("tiny", settings.chaos_tiny_mode),
        ("mirror", settings.chaos_mirror_mode),
        ("ghost", settings.chaos_ghost_mode),
        ("earthquake", settings.chaos_earthquake_mode),
        ("speed-demon", settings.chaos_speed_demon_mode),
        ("haunted", settings.chaos_haunted_mode),
        ("blackout", settings.chaos_blackout_mode),
        ("neon", settings.chaos_neon_mode),
    ]
    .into_iter()
    .filter(|(_, enabled)| *enabled)
    .map(|(name, _)| name.to_string())
    .collect()
}

fn language_names(opt: &Opt) -> io::Result<Vec<String>> {
    let names: BTreeSet<String> = opt
        .languages()?
        .map(|name| name.into_string().expect("Ill-formatted language name."))
        .collect();
    Ok(names.into_iter().collect())
}

fn settings_race(settings: &Settings) -> Option<Option<String>> {
    if settings.host_race {
        Some(None)
    } else {
        let address = settings.race_address.trim();
        if address.is_empty() {
            None
        } else {
            Some(Some(address.into()))
        }
    }
}

fn apply_settings_theme(config: &mut Config, configured_theme: &Theme, settings: &Settings) {
    config.theme = if settings.theme == "Default" {
        configured_theme.clone()
    } else {
        theme_by_name(&settings.theme)
    };
}

fn open_settings(state: &mut State) {
    let previous = std::mem::replace(state, State::Welcome);
    *state = State::Settings {
        screen: SettingsScreen::default(),
        previous: Box::new(previous),
    };
}

fn close_settings(state: &mut State) {
    let current = std::mem::replace(state, State::Welcome);
    if let State::Settings { previous, .. } = current {
        *state = *previous;
    }
}

fn spinner_at(started_at: Instant) -> &'static str {
    const FRAMES: [&str; 4] = ["|", "/", "-", "\\"];
    let index = (Instant::now()
        .saturating_duration_since(started_at)
        .as_millis()
        / 150) as usize
        % FRAMES.len();
    FRAMES[index]
}

enum StartOutcome {
    Test(Test, Option<RaceSession>),
    RaceLobby { test: Test, lobby: race::HostLobby },
}

fn start_test(opt: &Opt) -> io::Result<StartOutcome> {
    if opt.is_race_client() {
        let addr = opt
            .race_addr()
            .expect("race client should have a configured address");
        let invite = race::parse_invite(addr)?;
        println!("Connecting to race host {}...", invite.addr);
        let (words, session) = race::client(&invite).map_err(|error| {
            io::Error::new(error.kind(), format!("failed to connect to race: {error}"))
        })?;

        let test = build_synced_race_test(words, opt);
        return Ok(StartOutcome::Test(test, Some(session)));
    }

    let contents = opt.gen_contents().unwrap_or_else(|error| {
        eprintln!("Error: {error}");
        std::process::exit(1);
    });
    ensure_contents_not_empty(&contents);

    let test = build_test(contents, opt);
    if opt.is_race_host() {
        let words: Vec<String> = test.words.iter().map(|w| w.text.clone()).collect();
        let lobby = race::HostLobby::start(DEFAULT_RACE_ADDR, words).map_err(|error| {
            io::Error::new(error.kind(), format!("failed to connect to race: {error}"))
        })?;
        return Ok(StartOutcome::RaceLobby { test, lobby });
    }

    Ok(StartOutcome::Test(test, None))
}

fn start_home_host_race(opt: &Opt, settings: &Settings) -> State {
    let contents = opt.gen_contents().unwrap_or_else(|error| {
        eprintln!("Error: {error}");
        std::process::exit(1);
    });
    ensure_contents_not_empty(&contents);

    let mut test = build_test(contents, opt);
    apply_persistent_gameplay_state(&mut test, settings);
    start_host_lobby_setup(test)
}

fn start_host_lobby_setup(test: Test) -> State {
    let words = test.words.iter().map(|w| w.text.clone()).collect();
    State::RaceLobbyStarting {
        receiver: spawn_host_lobby_setup(DEFAULT_RACE_ADDR.to_string(), words),
        test: Some(test),
        started_at: Instant::now(),
        error: None,
    }
}

fn spawn_host_lobby_setup(_bind_addr: String, words: Vec<String>) -> Receiver<HostSetupEvent> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let event = match race::HostLobby::start_local(words) {
            Ok(lobby) => HostSetupEvent::Ready(lobby),
            Err(error) => HostSetupEvent::Failed(format!("failed to host local race: {error}")),
        };
        let _ = sender.send(event);
    });
    receiver
}

fn spawn_join_race(invite: race::RaceInvite) -> Receiver<JoinRaceEvent> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let event = match race::client(&invite) {
            Ok((words, session)) => JoinRaceEvent::Connected(words, session),
            Err(_) => JoinRaceEvent::Failed(JOIN_RACE_ERROR_MESSAGE.into()),
        };
        let _ = sender.send(event);
    });
    receiver
}

fn ensure_contents_not_empty(contents: &[String]) {
    if contents.is_empty() {
        eprintln!("Error: the provided file or language contains no words to type.");
        eprintln!("If you specified a file, make sure it isn't empty.");
        std::process::exit(1);
    }
}

/// Builds a test using the selected CLI mode and editing options.
fn build_test(contents: Vec<String>, opt: &Opt) -> Test {
    let gameplay_features = feature_set(&opt.gameplay_features);
    let prepared_words = gameplay::prepare_words(contents, &gameplay_features);
    let mut test = build_prepared_test(prepared_words, opt, gameplay_features);
    test.session_meta = Some(build_session_meta(opt, None));
    test
}

/// Builds a race client test from host-synchronized words without re-randomizing them.
fn build_synced_race_test(contents: Vec<String>, opt: &Opt) -> Test {
    let gameplay_features = feature_set(&opt.gameplay_features);
    let prepared_words = contents.into_iter().map(PreparedWord::from).collect();
    let mut test = build_prepared_test(prepared_words, opt, gameplay_features);
    test.session_meta = Some(build_session_meta(opt, Some("race_synced")));
    test
}

fn build_prepared_test(
    prepared_words: Vec<PreparedWord>,
    opt: &Opt,
    gameplay_features: BTreeSet<GameplayFeature>,
) -> Test {
    let mut test = Test::new_prepared(
        prepared_words,
        !opt.no_backtrack,
        opt.sudden_death,
        !opt.no_backspace,
        opt.time_limit(),
        gameplay_features,
        None,
    );
    test.rank_tag = opt
        .rank_session
        .as_ref()
        .filter(|session| session.use_rank_corpus)
        .map(|session| {
            format!(
                "{}·L{}",
                session.spec.id.rank.as_str(),
                session.spec.id.level
            )
        });
    test.phoenix_enabled = opt.phoenix;
    test
}

/// Applies queued opponent race events to the active test or lobby.
fn apply_race_events(
    state: &mut State,
    race_session: &mut Option<RaceSession>,
    settings: &mut Settings,
    settings_path: &Path,
    history_path: &Path,
    opt: &Opt,
) -> io::Result<()> {
    let Some(session) = race_session.as_mut() else {
        return Ok(());
    };
    let events = session.drain_events();

    for event in events {
        let mut end_race = false;

        match state {
            State::Test(test) => match event {
                RaceEvent::OpponentProgress(progress) => {
                    test.update_race_opponent(progress);
                }
                RaceEvent::OpponentFinished { wpm, accuracy } => {
                    test.update_race_opponent(test.words.len());
                    test.set_race_opponent_metrics(wpm, accuracy);
                    let (local_wpm, local_accuracy) = race_metrics_from_test(test);
                    test.set_race_local_metrics(local_wpm, local_accuracy);
                    let outcome = if test.completed_word_count() >= test.words.len() {
                        RaceOutcome::Tie
                    } else {
                        RaceOutcome::Lose
                    };
                    test.set_race_outcome(outcome, race_outcome_message(outcome));
                    end_race = true;
                }
                RaceEvent::Disconnected(_message) => {
                    let (local_wpm, local_accuracy) = race_metrics_from_test(test);
                    test.set_race_local_metrics(local_wpm, local_accuracy);
                    test.set_race_outcome(
                        RaceOutcome::Disconnected,
                        "Opponent disconnected. Race ended early.",
                    );
                    end_race = true;
                }
                _ => {}
            },
            State::JoinRaceLobby {
                test, started_at, ..
            } => match event {
                RaceEvent::SyncWords(words) => {
                    *test = build_synced_race_test(words, opt);
                    apply_persistent_gameplay_state(test, settings);
                    *started_at = Instant::now();
                }
                RaceEvent::Start => {
                    test.enable_race();
                    *state = State::Test(std::mem::replace(
                        test,
                        Test::new_prepared(
                            vec![],
                            false,
                            false,
                            false,
                            None,
                            BTreeSet::new(),
                            None,
                        ),
                    ));
                }
                RaceEvent::Disconnected(_message) => {
                    *race_session = None;
                    *state = State::Welcome;
                }
                _ => {}
            },
            State::RaceLobby { .. } => {
                if let RaceEvent::Disconnected(_message) = event {
                    *race_session = None;
                }
            }
            _ => {}
        }

        if end_race {
            if let State::Test(test) = state {
                *state = results_state_from_test(
                    &*test,
                    settings,
                    settings_path,
                    history_path,
                    opt.debug,
                    None,
                )?;
            }
        }
    }

    Ok(())
}

/// Reports local race progress and returns true when a network error ends the race.
fn report_race_progress(
    test: &mut Test,
    race_session: &mut Option<RaceSession>,
    before_progress: usize,
) -> bool {
    test.update_race_you();
    let after_progress = test.completed_word_count();
    if after_progress <= before_progress {
        return false;
    }

    let Some(session) = race_session.as_mut() else {
        return false;
    };

    let result = if test.complete {
        let (wpm, accuracy) = race_metrics_from_test(test);
        test.set_race_local_metrics(wpm, accuracy);
        session.send_finish(wpm, accuracy)
    } else {
        session.send_progress(after_progress)
    };

    if let Err(error) = result {
        let (wpm, accuracy) = race_metrics_from_test(test);
        test.set_race_local_metrics(wpm, accuracy);
        test.set_race_outcome(
            RaceOutcome::Disconnected,
            format!("Opponent disconnected. Race ended early. ({error})"),
        );
        return true;
    }

    false
}

/// Stores a local finish outcome before showing race results.
fn finalize_local_race(test: &mut Test) {
    let Some((opponent, total)) = test.race_progress.as_ref().and_then(|race| {
        race.outcome
            .is_none()
            .then_some((race.opponent, race.total))
    }) else {
        return;
    };

    let (wpm, accuracy) = race_metrics_from_test(test);
    test.set_race_local_metrics(wpm, accuracy);
    let outcome = if opponent >= total {
        RaceOutcome::Tie
    } else {
        RaceOutcome::Win
    };
    test.set_race_outcome(outcome, race_outcome_message(outcome));
}

/// Returns the results-screen label for a race outcome.
fn race_outcome_message(outcome: RaceOutcome) -> &'static str {
    match outcome {
        RaceOutcome::Win => "You won!",
        RaceOutcome::Lose => "Opponent won!",
        RaceOutcome::Tie => "Tie!",
        RaceOutcome::Disconnected => "Race: Opponent disconnected",
    }
}

fn race_metrics_from_test(test: &Test) -> (f64, f64) {
    let results = Results::from(test);
    (
        results.adjusted_wpm(),
        f64::from(results.accuracy.overall) * 100.0,
    )
}

fn apply_persistent_gameplay_state(test: &mut Test, settings: &Settings) {
    test.gameplay.ghost_best_wpm = (settings.best_wpm > 0.0).then_some(settings.best_wpm);
}

fn update_best_wpm(
    settings: &mut Settings,
    settings_path: &Path,
    results: &Results,
) -> io::Result<()> {
    let adjusted_wpm = results.adjusted_wpm();
    if adjusted_wpm > settings.best_wpm {
        settings.best_wpm = adjusted_wpm;
        settings.save_to(settings_path)?;
    }
    Ok(())
}

/// The single end-of-test funnel: computes results, persists the history
/// record, and evaluates rank promotion. History failures never block the
/// results screen.
fn results_state_from_test(
    test: &Test,
    settings: &mut Settings,
    settings_path: &Path,
    history_path: &Path,
    debug: bool,
    end_reason_hint: Option<&str>,
) -> io::Result<State> {
    let mut results = Results::from(test);
    update_best_wpm(settings, settings_path, &results)?;

    if let Some(meta) = &test.session_meta {
        let mut meta = meta.clone();
        if test.race_progress.is_some() {
            meta.mode = "race".into();
            meta.qualifying = false;
        }
        if let Some(mut record) = history::build_record(test, &results, &meta, end_reason_hint) {
            record.chaos_modes = chaos_mode_names(settings);

            if let Some(id) = record
                .rank
                .as_ref()
                .zip(record.level)
                .and_then(|(rank, level)| rank::LevelId::parse(rank, level))
            {
                let spec = rank::ladder::level_spec(id);
                if record.corpus.kind == "rank" && record.completed {
                    let improved = settings.rank_profile.record_best(id, record.adjusted_wpm);
                    if improved {
                        settings.save_to(settings_path)?;
                    }
                }
                let (outcome, streak) =
                    rank::promotion::evaluate(&settings.rank_profile, &spec, history_path, &record);
                record.promotion_event = match &outcome {
                    rank::promotion::PromotionOutcome::None => None,
                    rank::promotion::PromotionOutcome::LevelCleared { .. } => {
                        Some("level_cleared".into())
                    }
                    rank::promotion::PromotionOutcome::RankUp { .. } => {
                        Some("rank_promoted".into())
                    }
                    rank::promotion::PromotionOutcome::Mastery { .. } => Some("mastery".into()),
                };
                results.rank_banner =
                    Some(rank::promotion::banner(&spec, outcome, &record, streak));
            }

            history::append_record_best_effort(history_path, &record, debug);
        }
    }

    Ok(State::Results(results))
}

fn state_from_start_outcome(
    outcome: StartOutcome,
    settings: &Settings,
    host_lobby: &mut Option<race::HostLobby>,
) -> (State, Option<RaceSession>) {
    match outcome {
        StartOutcome::Test(mut test, session) => {
            apply_persistent_gameplay_state(&mut test, settings);
            (State::Test(test), session)
        }
        StartOutcome::RaceLobby { mut test, lobby } => {
            apply_persistent_gameplay_state(&mut test, settings);
            *host_lobby = Some(lobby);
            (
                State::RaceLobby {
                    test,
                    started_at: Instant::now(),
                    copied_at: None,
                },
                None,
            )
        }
    }
}

fn main() -> io::Result<()> {
    tunnel::register_cleanup_handlers();

    let matches = Opt::command().get_matches();
    let cli_overrides = CliOverrides::from_matches(&matches);
    let opt = Opt::from_arg_matches(&matches).expect("parsed CLI should match Opt");
    if opt.debug {
        dbg!(&opt);
        dbg!(&cli_overrides);
    }

    if let Some(Command::Completions { shell }) = &opt.command {
        generate(*shell, &mut Opt::command(), "ttyper", &mut io::stdout());
        return Ok(());
    }

    let mut config = opt.config();
    let configured_theme = config.theme.clone();
    let languages = language_names(&opt)?;

    if opt.list_languages {
        languages.iter().for_each(|name| println!("{name}"));

        return Ok(());
    }

    if let Some(level) = opt.level {
        if !(1..=rank::LEVELS_PER_RANK).contains(&level) {
            eprintln!("Error: levels are 1..={}.", rank::LEVELS_PER_RANK);
            std::process::exit(1);
        }
    }

    let settings_path = settings_path(opt.config_dir());
    let history_file = history::history_path(&opt.config_dir());
    let mut settings =
        Settings::load_or_default(&settings_path, &config.default_language, &languages)?;
    apply_settings_theme(&mut config, &configured_theme, &settings);

    if opt.rank_status {
        let effective = effective_with_rank(&opt, &settings, &cli_overrides);
        print!(
            "{}",
            rank::status::render(
                &settings.rank_profile,
                effective.rank_session.as_ref(),
                &history_file,
            )
        );
        return Ok(());
    }

    if opt.debug {
        dbg!(&config);
        dbg!(&settings);
    }

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    terminal::enable_raw_mode()?;
    execute!(
        io::stdout(),
        cursor::Hide,
        cursor::SavePosition,
        terminal::EnterAlternateScreen,
    )?;
    let mut terminal_cleanup = TerminalCleanup::active();
    terminal.clear()?;

    let mut state = State::Welcome;
    let mut race_session = None;
    let mut host_lobby = None;
    let mut chaos = ChaosState::default();

    if cli_overrides.race {
        let effective = effective_with_rank(&opt, &settings, &cli_overrides);
        let (initial_state, session) =
            state_from_start_outcome(start_test(&effective)?, &settings, &mut host_lobby);
        state = initial_state;
        race_session = session;
    }

    let welcome_rank_line = rank_welcome_line(&opt, &settings, &cli_overrides);
    state.render_into(
        &mut terminal,
        &config,
        &settings,
        &languages,
        &chaos,
        &race_session,
        &host_lobby,
        welcome_rank_line.as_deref(),
    )?;
    loop {
        let event = if event::poll(Duration::from_millis(100))? {
            Some(event::read()?)
        } else {
            None
        };

        let now = Instant::now();
        chaos.tick(&settings, now);
        let was_test_before_race_events = matches!(state, State::Test(_));
        apply_race_events(
            &mut state,
            &mut race_session,
            &mut settings,
            &settings_path,
            &history_file,
            &opt,
        )?;
        if was_test_before_race_events && !matches!(state, State::Test(_)) {
            chaos.reset_test_effects();
        }

        // handle exit controls
        match event.as_ref() {
            Some(Event::Key(KeyEvent {
                code: KeyCode::Char('c'),
                kind: KeyEventKind::Press,
                modifiers: KeyModifiers::CONTROL,
                ..
            })) => break,
            Some(Event::Key(KeyEvent {
                code: KeyCode::Esc,
                kind: KeyEventKind::Press,
                modifiers: KeyModifiers::NONE,
                ..
            })) => match &state {
                State::Welcome | State::Results(_) => break,
                State::Test(test) => {
                    chaos.on_keypress(&settings);
                    chaos.reset_test_effects();
                    if test.race_progress.is_some() {
                        race_session = None;
                    }
                    state = results_state_from_test(
                        test,
                        &mut settings,
                        &settings_path,
                        &history_file,
                        opt.debug,
                        Some("aborted"),
                    )?;
                }
                State::RaceLobbyStarting { .. }
                | State::RaceLobby { .. }
                | State::JoinRace { .. }
                | State::JoinRaceConnecting { .. }
                | State::JoinRaceLobby { .. }
                | State::Settings { .. } => {}
            },
            _ => {}
        }

        let mut next_state = None;
        let mut next_race_session = None;
        let mut next_host_lobby = None;
        let mut settings_close_requested = false;
        let mut settings_open_requested = false;

        match &mut state {
            State::Welcome => match event {
                Some(Event::Key(KeyEvent {
                    code: KeyCode::Enter,
                    kind: KeyEventKind::Press,
                    modifiers: KeyModifiers::NONE,
                    ..
                })) => {
                    chaos.on_keypress(&settings);
                    chaos.reset_test_effects();
                    let effective = effective_with_rank(&opt, &settings, &cli_overrides);
                    let (state, session) = state_from_start_outcome(
                        start_test(&effective)?,
                        &settings,
                        &mut host_lobby,
                    );
                    next_race_session = Some(session);
                    next_state = Some(state);
                }
                Some(Event::Key(KeyEvent {
                    code: KeyCode::Char('r') | KeyCode::Char('R'),
                    kind: KeyEventKind::Press,
                    modifiers: KeyModifiers::NONE | KeyModifiers::SHIFT,
                    ..
                })) => {
                    chaos.on_keypress(&settings);
                    chaos.reset_test_effects();
                    let effective = effective_with_rank(&opt, &settings, &cli_overrides);
                    next_race_session = Some(None);
                    next_state = Some(start_home_host_race(&effective, &settings));
                }
                Some(Event::Key(KeyEvent {
                    code: KeyCode::Char('j') | KeyCode::Char('J'),
                    kind: KeyEventKind::Press,
                    modifiers: KeyModifiers::NONE | KeyModifiers::SHIFT,
                    ..
                })) => {
                    chaos.on_keypress(&settings);
                    next_race_session = Some(None);
                    next_state = Some(State::JoinRace {
                        input: String::new(),
                        error: None,
                    });
                }
                Some(Event::Key(KeyEvent {
                    code: KeyCode::Char('s') | KeyCode::Char('S'),
                    kind: KeyEventKind::Press,
                    modifiers: KeyModifiers::NONE | KeyModifiers::SHIFT,
                    ..
                })) => {
                    chaos.on_keypress(&settings);
                    settings_open_requested = true;
                }
                Some(Event::Key(KeyEvent {
                    code: KeyCode::Char('q'),
                    kind: KeyEventKind::Press,
                    modifiers: KeyModifiers::NONE,
                    ..
                })) => {
                    chaos.on_keypress(&settings);
                    break;
                }
                _ => {}
            },
            State::Settings { screen, .. } => {
                if let Some(Event::Key(key)) = event {
                    match screen.handle_key(key, &mut settings, &languages) {
                        SettingsAction::None => {}
                        SettingsAction::Close => settings_close_requested = true,
                        SettingsAction::Changed => {
                            settings.normalize(&config.default_language, &languages);
                            settings.save_to(&settings_path)?;
                            apply_settings_theme(&mut config, &configured_theme, &settings);
                        }
                    }
                    if key.kind == KeyEventKind::Press {
                        chaos.on_keypress(&settings);
                    }
                }
            }
            State::RaceLobbyStarting {
                receiver,
                test,
                error,
                ..
            } => {
                if let Some(Event::Key(KeyEvent {
                    code: KeyCode::Esc,
                    kind: KeyEventKind::Press,
                    modifiers: KeyModifiers::NONE,
                    ..
                })) = event
                {
                    next_race_session = Some(None);
                    next_state = Some(State::Welcome);
                } else if error.is_none() {
                    match receiver.try_recv() {
                        Ok(HostSetupEvent::Ready(lobby)) => {
                            let Some(test) = test.take() else {
                                continue;
                            };
                            next_host_lobby = Some(Some(lobby));
                            next_state = Some(State::RaceLobby {
                                test,
                                started_at: Instant::now(),
                                copied_at: None,
                            });
                        }
                        Ok(HostSetupEvent::Failed(message)) => {
                            *error = Some(message);
                        }
                        Err(TryRecvError::Empty) => {}
                        Err(TryRecvError::Disconnected) => {
                            *error = Some("failed to host race: setup stopped unexpectedly".into());
                        }
                    }
                }
            }
            State::RaceLobby {
                test, copied_at, ..
            } => {
                let lobby = host_lobby
                    .as_mut()
                    .expect("RaceLobby state should have host_lobby");
                if let Some(Event::Key(KeyEvent {
                    code: KeyCode::Esc,
                    kind: KeyEventKind::Press,
                    modifiers: KeyModifiers::NONE,
                    ..
                })) = event
                {
                    lobby.cancel();
                    next_host_lobby = Some(None);
                    next_race_session = Some(None);
                    next_state = Some(State::Welcome);
                } else if let Some(Event::Key(KeyEvent {
                    code: KeyCode::Char('c') | KeyCode::Char('C'),
                    kind: KeyEventKind::Press,
                    modifiers: KeyModifiers::NONE | KeyModifiers::SHIFT,
                    ..
                })) = event
                {
                    let cmd = lobby.invite_command().to_string();
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_text(cmd);
                    }
                    *copied_at = Some(Instant::now());
                } else if let Some(Event::Key(KeyEvent {
                    code: KeyCode::Char('s') | KeyCode::Char('S'),
                    kind: KeyEventKind::Press,
                    modifiers: KeyModifiers::NONE | KeyModifiers::SHIFT,
                    ..
                })) = event
                {
                    if let Some(session) = race_session.as_mut() {
                        let words: Vec<String> =
                            test.words.iter().map(|w| w.text.clone()).collect();
                        let _ = session.send_words(&words);
                        let _ = session.send_start();
                        test.enable_race();
                        next_state = Some(State::Test(std::mem::replace(
                            test,
                            Test::new_prepared(
                                vec![],
                                false,
                                false,
                                false,
                                None,
                                BTreeSet::new(),
                                None,
                            ),
                        )));
                    }
                } else if let Some(lobby_event) = lobby.poll() {
                    match lobby_event {
                        LobbyEvent::OpponentConnected(mut session) => {
                            if let Some(tunnel) = lobby.take_tunnel() {
                                session.keep_tunnel(tunnel);
                            }
                            let words: Vec<String> =
                                test.words.iter().map(|w| w.text.clone()).collect();
                            let _ = session.send_words(&words);
                            next_race_session = Some(Some(session));
                        }
                        LobbyEvent::Cancelled => {
                            next_host_lobby = Some(None);
                            next_race_session = Some(None);
                            next_state = Some(State::Welcome);
                        }
                        LobbyEvent::Failed(message) => {
                            eprintln!("{message}");
                            next_host_lobby = Some(None);
                            next_race_session = Some(None);
                            next_state = Some(State::Welcome);
                        }
                    }
                }
            }
            State::JoinRace { input, error } => match event {
                Some(Event::Key(KeyEvent {
                    code: KeyCode::Esc,
                    kind: KeyEventKind::Press,
                    modifiers: KeyModifiers::NONE,
                    ..
                })) => {
                    next_race_session = Some(None);
                    next_state = Some(State::Welcome);
                }
                Some(Event::Key(KeyEvent {
                    code: KeyCode::Enter,
                    kind: KeyEventKind::Press,
                    modifiers: KeyModifiers::NONE,
                    ..
                })) => match race::parse_invite(input.trim()) {
                    Ok(invite) => {
                        let input = input.trim().to_string();
                        next_state = Some(State::JoinRaceConnecting {
                            receiver: spawn_join_race(invite.clone()),
                            input,
                            invite,
                            started_at: Instant::now(),
                        });
                    }
                    Err(_) => {
                        *error = Some(JOIN_RACE_ERROR_MESSAGE.into());
                    }
                },
                Some(Event::Key(KeyEvent {
                    code: KeyCode::Backspace,
                    kind: KeyEventKind::Press,
                    ..
                })) => {
                    chaos.on_keypress(&settings);
                    input.pop();
                    *error = None;
                }
                Some(Event::Key(KeyEvent {
                    code: KeyCode::Char(character),
                    kind: KeyEventKind::Press,
                    modifiers,
                    ..
                })) if !modifiers.contains(KeyModifiers::CONTROL)
                    && !modifiers.contains(KeyModifiers::ALT) =>
                {
                    chaos.on_keypress(&settings);
                    input.push(character);
                    *error = None;
                }
                Some(Event::Paste(pasted)) => {
                    input.push_str(&pasted);
                    *error = None;
                }
                _ => {}
            },
            State::JoinRaceConnecting {
                input, receiver, ..
            } => {
                if let Some(Event::Key(KeyEvent {
                    code: KeyCode::Esc,
                    kind: KeyEventKind::Press,
                    modifiers: KeyModifiers::NONE,
                    ..
                })) = event
                {
                    next_race_session = Some(None);
                    next_state = Some(State::Welcome);
                } else {
                    match receiver.try_recv() {
                        Ok(JoinRaceEvent::Connected(words, session)) => {
                            next_race_session = Some(Some(session));
                            next_state = Some(State::JoinRaceLobby {
                                test: build_synced_race_test(words, &opt),
                                started_at: Instant::now(),
                            });
                        }
                        Ok(JoinRaceEvent::Failed(message)) => {
                            next_state = Some(State::JoinRace {
                                input: input.clone(),
                                error: Some(message),
                            });
                        }
                        Err(TryRecvError::Empty) => {}
                        Err(TryRecvError::Disconnected) => {
                            next_state = Some(State::JoinRace {
                                input: input.clone(),
                                error: Some(JOIN_RACE_ERROR_MESSAGE.into()),
                            });
                        }
                    }
                }
            }
            State::JoinRaceLobby { .. } => {
                if let Some(Event::Key(KeyEvent {
                    code: KeyCode::Esc,
                    kind: KeyEventKind::Press,
                    modifiers: KeyModifiers::NONE,
                    ..
                })) = event
                {
                    next_race_session = Some(None);
                    next_state = Some(State::Welcome);
                }
            }
            State::Test(ref mut test) => {
                if let Some(Event::Key(key)) = event {
                    if key.kind == KeyEventKind::Press {
                        chaos.on_keypress(&settings);
                    }
                    let before_progress = test.completed_word_count();
                    let before_power_combo = test.gameplay.combo;
                    test.handle_key(key);
                    if test.feature_enabled(GameplayFeature::PowerMode)
                        && test.completed_word_count() > before_progress
                        && test.gameplay.combo > before_power_combo
                    {
                        chaos.on_power_combo(test.gameplay.combo, now);
                    }
                    let after_progress = test.completed_word_count();
                    chaos.observe_word_progress(&settings, before_progress, after_progress, now);
                    if test.regen_requested {
                        // Phoenix death: respawn the test with a freshly
                        // generated word set; no history record for ashes.
                        test.regen_requested = false;
                        chaos.reset_test_effects();
                        let effective = effective_with_rank(&opt, &settings, &cli_overrides);
                        let contents = effective.gen_contents().unwrap_or_default();
                        if contents.is_empty() {
                            test.reset();
                        } else {
                            let mut reborn = build_test(contents, &effective);
                            apply_persistent_gameplay_state(&mut reborn, &settings);
                            next_state = Some(State::Test(reborn));
                        }
                    } else if report_race_progress(test, &mut race_session, before_progress) {
                        next_race_session = Some(None);
                        next_state = Some(results_state_from_test(
                            &*test,
                            &mut settings,
                            &settings_path,
                            &history_file,
                            opt.debug,
                            None,
                        )?);
                    } else if test.complete {
                        finalize_local_race(test);
                        next_state = Some(results_state_from_test(
                            &*test,
                            &mut settings,
                            &settings_path,
                            &history_file,
                            opt.debug,
                            None,
                        )?);
                    }
                }
            }
            State::Results(ref result) => match event {
                Some(Event::Key(KeyEvent {
                    code: KeyCode::Char('n') | KeyCode::Char('N'),
                    kind: KeyEventKind::Press,
                    modifiers: KeyModifiers::NONE | KeyModifiers::SHIFT,
                    ..
                })) if result
                    .rank_banner
                    .as_ref()
                    .is_some_and(|banner| banner.outcome.advances()) =>
                {
                    chaos.on_keypress(&settings);
                    let banner = result
                        .rank_banner
                        .clone()
                        .expect("guard checked the banner exists");
                    rank::promotion::commit_advance(&mut settings.rank_profile, &banner.outcome);
                    settings.save_to(&settings_path)?;
                    chaos.reset_test_effects();
                    let effective = effective_with_rank(&opt, &settings, &cli_overrides);
                    let (state, session) = state_from_start_outcome(
                        start_test(&effective)?,
                        &settings,
                        &mut host_lobby,
                    );
                    next_race_session = Some(session);
                    next_state = Some(state);
                }
                Some(Event::Key(KeyEvent {
                    code: KeyCode::Char('r'),
                    kind: KeyEventKind::Press,
                    modifiers: KeyModifiers::NONE,
                    ..
                })) => {
                    chaos.on_keypress(&settings);
                    if result.race_progress.is_some() {
                        if host_lobby.is_some() {
                            let effective = effective_with_rank(&opt, &settings, &cli_overrides);
                            let test = build_test(
                                effective.gen_contents().unwrap_or_default(),
                                &effective,
                            );
                            next_state = Some(State::RaceLobby {
                                test,
                                started_at: Instant::now(),
                                copied_at: None,
                            });
                        } else if race_session.is_some() {
                            next_state = Some(State::JoinRaceLobby {
                                test: Test::new_prepared(
                                    vec![],
                                    false,
                                    false,
                                    false,
                                    None,
                                    BTreeSet::new(),
                                    None,
                                ),
                                started_at: Instant::now(),
                            });
                        }
                    } else {
                        chaos.reset_test_effects();
                        let effective = effective_with_rank(&opt, &settings, &cli_overrides);
                        let (state, session) = state_from_start_outcome(
                            start_test(&effective)?,
                            &settings,
                            &mut host_lobby,
                        );
                        next_race_session = Some(session);
                        next_state = Some(state);
                    }
                }
                Some(Event::Key(KeyEvent {
                    code: KeyCode::Char('p'),
                    kind: KeyEventKind::Press,
                    modifiers: KeyModifiers::NONE,
                    ..
                })) => {
                    chaos.on_keypress(&settings);
                    if result.race_progress.is_some() {
                        continue;
                    }
                    if result.missed_words.is_empty() {
                        continue;
                    }
                    // repeat each missed word 5 times
                    let mut practice_words: Vec<String> = (result.missed_words)
                        .iter()
                        .flat_map(|w| vec![w.clone(); 5])
                        .collect();
                    practice_words.shuffle(&mut thread_rng());
                    let effective = effective_with_rank(&opt, &settings, &cli_overrides);
                    chaos.reset_test_effects();
                    next_race_session = Some(None);
                    let mut test = build_test(practice_words, &effective);
                    if let Some(meta) = &mut test.session_meta {
                        // Practice repeats missed words; never a rank corpus.
                        meta.corpus = history::Corpus {
                            kind: "practice".into(),
                            name: None,
                            language: None,
                        };
                        meta.qualifying = false;
                    }
                    apply_persistent_gameplay_state(&mut test, &settings);
                    next_state = Some(State::Test(test));
                }
                Some(Event::Key(KeyEvent {
                    code: KeyCode::Char('s') | KeyCode::Char('S'),
                    kind: KeyEventKind::Press,
                    modifiers: KeyModifiers::NONE | KeyModifiers::SHIFT,
                    ..
                })) => {
                    chaos.on_keypress(&settings);
                    settings_open_requested = true;
                }
                Some(Event::Key(KeyEvent {
                    code: KeyCode::Char('q'),
                    kind: KeyEventKind::Press,
                    modifiers: KeyModifiers::NONE,
                    ..
                })) => {
                    chaos.on_keypress(&settings);
                    break;
                }
                _ => {}
            },
        }

        if let Some(session) = next_race_session {
            race_session = session;
        }
        if let Some(lobby) = next_host_lobby {
            host_lobby = lobby;
        }
        if let Some(next) = next_state {
            if matches!(state, State::Test(_)) && !matches!(next, State::Test(_)) {
                chaos.reset_test_effects();
            }
            state = next;
        }
        if settings_open_requested {
            open_settings(&mut state);
        }
        if settings_close_requested {
            close_settings(&mut state);
            if let State::RaceLobby { test, .. } = &mut state {
                let effective = effective_with_rank(&opt, &settings, &cli_overrides);
                *test = build_test(effective.gen_contents().unwrap_or_default(), &effective);
                apply_persistent_gameplay_state(test, &settings);
                if let Some(session) = race_session.as_mut() {
                    let words: Vec<String> = test.words.iter().map(|w| w.text.clone()).collect();
                    let _ = session.send_words(&words);
                }
            }
        }

        let now = Instant::now();
        if let State::Test(test) = &mut state {
            test.tick(now);
            if test.complete {
                finalize_local_race(test);
                chaos.reset_test_effects();
                state = results_state_from_test(
                    &*test,
                    &mut settings,
                    &settings_path,
                    &history_file,
                    opt.debug,
                    None,
                )?;
            }
        }

        let now = Instant::now();
        if let State::Test(test) = &state {
            chaos.update_speed_demon(&settings, test, now);
        }
        let timed_out = match &state {
            State::Test(test) => {
                let effects = chaos.test_effects(&settings, test, now);
                let time_multiplier = effects.time_multiplier * test.visual_elapsed_multiplier();
                if let Some(elapsed) = effects.accelerated_elapsed {
                    test.time_expired_after_elapsed(elapsed)
                } else if time_multiplier <= 1.0 {
                    test.time_expired_at(now)
                } else {
                    test.time_expired_at_with_multiplier(now, time_multiplier)
                }
            }
            _ => false,
        };
        if timed_out {
            if let State::Test(test) = &mut state {
                finalize_local_race(test);
                chaos.reset_test_effects();
                state = results_state_from_test(
                    &*test,
                    &mut settings,
                    &settings_path,
                    &history_file,
                    opt.debug,
                    Some("timeout"),
                )?;
            }
        }

        let welcome_rank_line = match &state {
            State::Welcome => rank_welcome_line(&opt, &settings, &cli_overrides),
            _ => None,
        };
        state.render_into(
            &mut terminal,
            &config,
            &settings,
            &languages,
            &chaos,
            &race_session,
            &host_lobby,
            welcome_rank_line.as_deref(),
        )?;
    }

    terminal_cleanup.finish()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_opt(path: PathBuf) -> Opt {
        Opt {
            contents: Some(path),
            debug: false,
            words: num::NonZeroUsize::new(50).unwrap(),
            time: None,
            min_length: None,
            max_length: None,
            punctuation: false,
            numbers: false,
            race: None,
            config: None,
            language_file: None,
            language: None,
            list_languages: false,
            no_backtrack: false,
            sudden_death: false,
            phoenix: false,
            no_backspace: false,
            gameplay_features: Vec::new(),
            all_gameplay_features: false,
            rank: None,
            level: None,
            rank_status: false,
            rank_session: None,
            command: None,
        }
    }

    #[test]
    fn gen_contents_empty_file_returns_empty_vec() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.txt");
        fs::File::create(&path).unwrap();

        let contents = make_opt(path).gen_contents().unwrap();
        assert!(contents.is_empty(), "empty file should produce empty vec");
    }

    #[test]
    fn gen_contents_nonempty_file_returns_words() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("words.txt");
        let mut f = fs::File::create(&path).unwrap();
        writeln!(f, "hello world rust").unwrap();

        let contents = make_opt(path).gen_contents().unwrap();
        assert!(!contents.is_empty(), "non-empty file should produce words");
    }

    #[test]
    fn filter_word_pool_rejects_invalid_length_bounds() {
        let mut opt = make_opt(PathBuf::from("unused"));
        opt.min_length = num::NonZeroUsize::new(5);
        opt.max_length = num::NonZeroUsize::new(3);

        let error = opt
            .filter_word_pool(vec!["rust".into()])
            .expect_err("invalid bounds should return an error");

        assert!(error.contains("--min-length"));
    }

    #[test]
    fn filter_word_pool_rejects_empty_filtered_pool() {
        let mut opt = make_opt(PathBuf::from("unused"));
        opt.min_length = num::NonZeroUsize::new(10);

        let error = opt
            .filter_word_pool(vec!["rust".into(), "code".into()])
            .expect_err("empty filtered pool should return an error");

        assert!(error.contains("removed every word"));
    }

    #[test]
    fn filter_word_pool_keeps_words_within_bounds() {
        let mut opt = make_opt(PathBuf::from("unused"));
        opt.min_length = num::NonZeroUsize::new(4);
        opt.max_length = num::NonZeroUsize::new(5);

        let words = opt
            .filter_word_pool(vec!["go".into(), "rust".into(), "typing".into()])
            .unwrap();

        assert_eq!(words, vec!["rust"]);
    }

    #[test]
    fn rank_corpus_generates_from_embedded_resources() {
        let mut opt = make_opt(PathBuf::from("unused"));
        opt.contents = None;
        let settings = Settings::default();
        let overrides = CliOverrides::default();

        for rank in rank::ALL_RANKS {
            for level in [1, 5, 10] {
                opt.rank = Some(rank);
                opt.level = Some(level);
                let effective = effective_with_rank(&opt, &settings, &overrides);
                let session = effective
                    .rank_session
                    .as_ref()
                    .expect("rank session should resolve");
                assert!(session.use_rank_corpus);
                let words = effective
                    .gen_contents()
                    .unwrap_or_else(|e| panic!("{}{} corpus failed: {e}", rank.as_str(), level));
                assert_eq!(
                    words.len(),
                    50,
                    "{}{} should produce the prescribed word count",
                    rank.as_str(),
                    level
                );
                assert!(words.iter().all(|word| !word.is_empty()));
            }
        }
    }

    #[test]
    fn rank_g1_words_match_plain_english200_shape() {
        let mut opt = make_opt(PathBuf::from("unused"));
        opt.contents = None;
        opt.rank = Some(rank::Rank::G);
        opt.level = Some(1);
        let effective = effective_with_rank(&opt, &Settings::default(), &CliOverrides::default());
        let words = effective.gen_contents().unwrap();
        assert!(words
            .iter()
            .all(|word| word.chars().all(|c| c.is_alphabetic() && c.is_lowercase())));
    }

    #[test]
    fn no_rank_flags_keep_legacy_session_meta() {
        let opt = make_opt(PathBuf::from("unused"));
        let effective = effective_with_rank(&opt, &Settings::default(), &CliOverrides::default());
        assert!(effective.rank_session.is_none());
        let meta = build_session_meta(&effective, None);
        assert_eq!(meta.mode, "words");
        assert!(meta.rank.is_none());
        assert!(!meta.qualifying);
    }

    #[test]
    fn funnel_writes_history_record_and_evaluates_promotion() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");
        let history_path = dir.path().join("history.jsonl");
        let mut settings = Settings::default();

        let mut opt = make_opt(PathBuf::from("unused"));
        opt.contents = None;
        opt.rank = Some(rank::Rank::G);
        let effective = effective_with_rank(&opt, &settings, &CliOverrides::default());

        let mut test = build_test(vec!["ab".into(), "cd".into()], &effective);
        for c in ['a', 'b', ' ', 'c', 'd'] {
            test.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert!(test.complete);

        let state = results_state_from_test(
            &test,
            &mut settings,
            &settings_path,
            &history_path,
            false,
            None,
        )
        .unwrap();
        let State::Results(results) = state else {
            panic!("expected results state");
        };
        let banner = results.rank_banner.expect("rank session should set banner");
        assert!(
            banner.outcome.advances(),
            "instant perfect run should clear G1"
        );

        let records = history::read_tail(&history_path, &history::TailQuery::default()).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].corpus.kind, "rank");
        assert_eq!(records[0].rank.as_deref(), Some("G"));
        assert_eq!(records[0].promotion_event.as_deref(), Some("level_cleared"));
        assert!(records[0].completed);
        assert!(records[0].qualifying);
        assert_eq!(records[0].keystrokes.len(), 5);
        assert!(settings
            .rank_profile
            .best_wpm(rank::LevelId::new(rank::Rank::G, 1))
            .is_some());
    }

    #[test]
    fn explicit_sudden_death_beats_saved_phoenix() {
        let mut opt = make_opt(PathBuf::from("unused"));
        opt.sudden_death = true;
        let settings = Settings {
            phoenix: true,
            ..Default::default()
        };
        let overrides = CliOverrides {
            sudden_death: true,
            ..Default::default()
        };

        let effective = opt.effective(&settings, &overrides);
        assert!(effective.sudden_death);
        assert!(!effective.phoenix);

        // Without the explicit CLI flag the saved phoenix wins.
        let effective = opt.effective(&settings, &CliOverrides::default());
        assert!(effective.phoenix);
        assert!(!effective.sudden_death);
    }

    #[test]
    fn saved_settings_override_cli_defaults() {
        let opt = make_opt(PathBuf::from("unused"));
        let settings = Settings {
            word_count: 100,
            time_limit: Some(30),
            sudden_death: true,
            no_backtrack: true,
            no_backspace: true,
            punctuation: true,
            numbers: true,
            min_word_length: Some(4),
            max_word_length: Some(8),
            language: "rust".into(),
            ..Default::default()
        };

        let effective = opt.effective(&settings, &CliOverrides::default());

        assert_eq!(effective.words.get(), 100);
        assert_eq!(effective.time.unwrap().get(), 30);
        assert_eq!(effective.min_length.unwrap().get(), 4);
        assert_eq!(effective.max_length.unwrap().get(), 8);
        assert_eq!(effective.language, Some("rust".into()));
        assert!(effective.sudden_death);
        assert!(effective.no_backtrack);
        assert!(effective.no_backspace);
        assert!(effective.punctuation);
        assert!(effective.numbers);
    }

    #[test]
    fn explicit_cli_values_override_saved_settings() {
        let mut opt = make_opt(PathBuf::from("unused"));
        opt.words = num::NonZeroUsize::new(25).unwrap();
        opt.language = Some("python".into());
        let settings = Settings {
            word_count: 100,
            language: "rust".into(),
            ..Default::default()
        };
        let overrides = CliOverrides {
            words: true,
            language: true,
            ..Default::default()
        };

        let effective = opt.effective(&settings, &overrides);

        assert_eq!(effective.words.get(), 25);
        assert_eq!(effective.language, Some("python".into()));
    }
}
