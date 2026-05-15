mod chaos;
mod config;
mod gameplay;
mod race;
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

    /// Disable backspace
    #[arg(long)]
    no_backspace: bool,

    /// Enable a gameplay feature by kebab-case name; repeat for multiple features
    #[arg(long = "feature", value_enum, value_name = "FEATURE")]
    gameplay_features: Vec<GameplayFeature>,

    /// Enable every gameplay feature, including power mode
    #[arg(long)]
    all_gameplay_features: bool,

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
        lobby: race::HostLobby,
        test: Option<Test>,
        started_at: Instant,
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
    Test(Test),
    Results(Results),
}

enum HostSetupEvent {
    Ready(race::HostLobby),
    Failed(String),
}

enum JoinRaceEvent {
    Connected {
        words: Vec<String>,
        session: RaceSession,
    },
    Failed(String),
}

impl State {
    fn render_into<B: ratatui::backend::Backend>(
        &self,
        terminal: &mut Terminal<B>,
        config: &Config,
        settings: &Settings,
        languages: &[String],
        chaos: &ChaosState,
    ) -> io::Result<()> {
        let theme = chaos.apply_theme(&config.theme, settings);
        match self {
            State::Welcome => {
                terminal.draw(|f| {
                    let area = chaos.earthquake_area(f.size(), settings);
                    f.render_widget(theme.apply_to(Welcome), area);
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
                            public_addr: "Preparing tunnel...",
                            invite_command: "Preparing invite command...",
                            status: "Setting up race lobby...",
                            spinner: spinner_at(*started_at),
                            cancel_label: "Press Esc to cancel and go back",
                            error: error.as_deref(),
                        }),
                        area,
                    );
                })?;
            }
            State::RaceLobby {
                lobby, started_at, ..
            } => {
                terminal.draw(|f| {
                    let area = chaos.earthquake_area(f.size(), settings);
                    f.render_widget(
                        theme.apply_to(ui::RaceLobbyView {
                            room_code: lobby.room_code(),
                            public_addr: lobby.public_addr(),
                            invite_command: lobby.invite_command(),
                            status: "Waiting for opponent to connect...",
                            spinner: spinner_at(*started_at),
                            cancel_label: "Press Esc to cancel and go back",
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

struct Welcome;

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

        let content = Text::from(vec![
            Line::from(Span::styled("ttyper", theme.title)),
            Line::from(""),
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
        let (contents, session) = race::client(&invite).map_err(|error| {
            io::Error::new(error.kind(), format!("failed to connect to race: {error}"))
        })?;
        ensure_contents_not_empty(&contents);

        let mut test = build_synced_race_test(contents, opt);
        test.enable_race();
        return Ok(StartOutcome::Test(test, Some(session)));
    }

    let contents = opt.gen_contents().unwrap_or_else(|error| {
        eprintln!("Error: {error}");
        std::process::exit(1);
    });
    ensure_contents_not_empty(&contents);

    let test = build_test(contents, opt);
    if opt.is_race_host() {
        let race_words = test
            .words
            .iter()
            .map(|word| word.text.clone())
            .collect::<Vec<_>>();
        let lobby = race::HostLobby::start(DEFAULT_RACE_ADDR, race_words).map_err(|error| {
            io::Error::new(error.kind(), format!("failed to host race: {error}"))
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
    let race_words = test
        .words
        .iter()
        .map(|word| word.text.clone())
        .collect::<Vec<_>>();
    State::RaceLobbyStarting {
        receiver: spawn_host_lobby_setup(DEFAULT_RACE_ADDR.to_string(), race_words),
        test: Some(test),
        started_at: Instant::now(),
        error: None,
    }
}

fn spawn_host_lobby_setup(bind_addr: String, words: Vec<String>) -> Receiver<HostSetupEvent> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let event = match race::HostLobby::start(&bind_addr, words) {
            Ok(lobby) => HostSetupEvent::Ready(lobby),
            Err(error) => HostSetupEvent::Failed(format!("failed to host race: {error}")),
        };
        let _ = sender.send(event);
    });
    receiver
}

fn spawn_join_race(invite: race::RaceInvite) -> Receiver<JoinRaceEvent> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let event = match race::client(&invite) {
            Ok((words, session)) => JoinRaceEvent::Connected { words, session },
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
    build_prepared_test(prepared_words, opt, gameplay_features)
}

/// Builds a race client test from host-synchronized words without re-randomizing them.
fn build_synced_race_test(contents: Vec<String>, opt: &Opt) -> Test {
    let gameplay_features = feature_set(&opt.gameplay_features);
    let prepared_words = contents.into_iter().map(PreparedWord::from).collect();
    build_prepared_test(prepared_words, opt, gameplay_features)
}

fn build_prepared_test(
    prepared_words: Vec<PreparedWord>,
    opt: &Opt,
    gameplay_features: BTreeSet<GameplayFeature>,
) -> Test {
    Test::new_prepared(
        prepared_words,
        !opt.no_backtrack,
        opt.sudden_death,
        !opt.no_backspace,
        opt.time_limit(),
        gameplay_features,
        None,
    )
}

/// Applies queued opponent race events to the active test.
fn apply_race_events(
    state: &mut State,
    race_session: &mut Option<RaceSession>,
    settings: &mut Settings,
    settings_path: &Path,
) -> io::Result<()> {
    let Some(session) = race_session.as_mut() else {
        return Ok(());
    };
    let events = session.drain_events();

    for event in events {
        let mut end_race = false;

        if let State::Test(test) = state {
            match event {
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
            }
        }

        if end_race {
            *race_session = None;
            if let State::Test(test) = state {
                *state = results_state_from_test(&*test, settings, settings_path)?;
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

fn results_state_from_test(
    test: &Test,
    settings: &mut Settings,
    settings_path: &Path,
) -> io::Result<State> {
    let results = Results::from(test);
    update_best_wpm(settings, settings_path, &results)?;
    Ok(State::Results(results))
}

fn state_from_start_outcome(
    outcome: StartOutcome,
    settings: &Settings,
) -> (State, Option<RaceSession>) {
    match outcome {
        StartOutcome::Test(mut test, session) => {
            apply_persistent_gameplay_state(&mut test, settings);
            (State::Test(test), session)
        }
        StartOutcome::RaceLobby { mut test, lobby } => {
            apply_persistent_gameplay_state(&mut test, settings);
            (
                State::RaceLobby {
                    lobby,
                    test: Some(test),
                    started_at: Instant::now(),
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

    let settings_path = settings_path(opt.config_dir());
    let mut settings =
        Settings::load_or_default(&settings_path, &config.default_language, &languages)?;
    apply_settings_theme(&mut config, &configured_theme, &settings);

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
    let mut chaos = ChaosState::default();

    if cli_overrides.race {
        let effective = opt.effective(&settings, &cli_overrides);
        let (initial_state, session) = state_from_start_outcome(start_test(&effective)?, &settings);
        state = initial_state;
        race_session = session;
    }

    state.render_into(&mut terminal, &config, &settings, &languages, &chaos)?;
    loop {
        let event = if event::poll(Duration::from_millis(100))? {
            Some(event::read()?)
        } else {
            None
        };

        let now = Instant::now();
        chaos.tick(&settings, now);
        let was_test_before_race_events = matches!(state, State::Test(_));
        apply_race_events(&mut state, &mut race_session, &mut settings, &settings_path)?;
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
                    state = results_state_from_test(test, &mut settings, &settings_path)?;
                }
                State::RaceLobbyStarting { .. }
                | State::RaceLobby { .. }
                | State::JoinRace { .. }
                | State::JoinRaceConnecting { .. }
                | State::Settings { .. } => {}
            },
            _ => {}
        }

        let mut next_state = None;
        let mut next_race_session = None;
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
                    let effective = opt.effective(&settings, &cli_overrides);
                    let (state, session) =
                        state_from_start_outcome(start_test(&effective)?, &settings);
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
                    let effective = opt.effective(&settings, &cli_overrides);
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
                            next_state = Some(State::RaceLobby {
                                lobby,
                                test: Some(test),
                                started_at: Instant::now(),
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
            State::RaceLobby { lobby, test, .. } => {
                if let Some(Event::Key(KeyEvent {
                    code: KeyCode::Esc,
                    kind: KeyEventKind::Press,
                    modifiers: KeyModifiers::NONE,
                    ..
                })) = event
                {
                    lobby.cancel();
                    next_race_session = Some(None);
                    next_state = Some(State::Welcome);
                } else if let Some(lobby_event) = lobby.poll() {
                    match lobby_event {
                        LobbyEvent::OpponentConnected(mut session) => {
                            let Some(mut test) = test.take() else {
                                continue;
                            };
                            if let Some(tunnel) = lobby.take_tunnel() {
                                session.keep_tunnel(tunnel);
                            }
                            test.enable_race();
                            next_race_session = Some(Some(session));
                            next_state = Some(State::Test(test));
                        }
                        LobbyEvent::Cancelled => {
                            next_race_session = Some(None);
                            next_state = Some(State::Welcome);
                        }
                        LobbyEvent::Failed(message) => {
                            eprintln!("{message}");
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
                        Ok(JoinRaceEvent::Connected { words, session }) if !words.is_empty() => {
                            let effective = opt.effective(&settings, &cli_overrides);
                            let mut test = build_synced_race_test(words, &effective);
                            apply_persistent_gameplay_state(&mut test, &settings);
                            test.enable_race();
                            next_race_session = Some(Some(session));
                            next_state = Some(State::Test(test));
                        }
                        Ok(JoinRaceEvent::Connected { .. }) => {
                            next_state = Some(State::JoinRace {
                                input: input.clone(),
                                error: Some(JOIN_RACE_ERROR_MESSAGE.into()),
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
                    if report_race_progress(test, &mut race_session, before_progress) {
                        next_race_session = Some(None);
                        next_state = Some(results_state_from_test(
                            &*test,
                            &mut settings,
                            &settings_path,
                        )?);
                    } else if test.complete {
                        finalize_local_race(test);
                        if test.race_progress.is_some() {
                            next_race_session = Some(None);
                        }
                        next_state = Some(results_state_from_test(
                            &*test,
                            &mut settings,
                            &settings_path,
                        )?);
                    }
                }
            }
            State::Results(ref result) => match event {
                Some(Event::Key(KeyEvent {
                    code: KeyCode::Char('r'),
                    kind: KeyEventKind::Press,
                    modifiers: KeyModifiers::NONE,
                    ..
                })) => {
                    chaos.on_keypress(&settings);
                    if result.race_progress.is_some() {
                        continue;
                    }
                    chaos.reset_test_effects();
                    let effective = opt.effective(&settings, &cli_overrides);
                    let (state, session) =
                        state_from_start_outcome(start_test(&effective)?, &settings);
                    next_race_session = Some(session);
                    next_state = Some(state);
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
                    let effective = opt.effective(&settings, &cli_overrides);
                    chaos.reset_test_effects();
                    next_race_session = Some(None);
                    let mut test = build_test(practice_words, &effective);
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
        }

        let now = Instant::now();
        if let State::Test(test) = &mut state {
            test.tick(now);
            if test.complete {
                finalize_local_race(test);
                chaos.reset_test_effects();
                if test.race_progress.is_some() {
                    race_session = None;
                }
                state = results_state_from_test(&*test, &mut settings, &settings_path)?;
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
                if test.race_progress.is_some() {
                    race_session = None;
                }
                state = results_state_from_test(&*test, &mut settings, &settings_path)?;
            }
        }

        state.render_into(&mut terminal, &config, &settings, &languages, &chaos)?;
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
            no_backspace: false,
            gameplay_features: Vec::new(),
            all_gameplay_features: false,
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
