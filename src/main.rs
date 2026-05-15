mod chaos;
mod config;
mod race;
mod settings;
mod test;
mod ui;

use chaos::ChaosState;
use config::{theme_by_name, Config, Theme};
use race::{RaceEvent, RaceSession};
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
    path::PathBuf,
    str,
    time::{Duration, Instant},
};

const TIME_MODE_WORD_COUNT: usize = 10_000;
const DEFAULT_RACE_ADDR: &str = "0.0.0.0:7878";
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

    /// Host a race or connect to the specified race host
    #[arg(
        long,
        value_name = "HOST:PORT",
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
        if self.time.is_some() {
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
    Test(Test),
    Results(Results),
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

fn start_test(opt: &Opt) -> io::Result<(Test, Option<RaceSession>)> {
    let mut contents = if opt.is_race_client() {
        Vec::new()
    } else {
        opt.gen_contents().unwrap_or_else(|error| {
            eprintln!("Error: {error}");
            std::process::exit(1);
        })
    };

    let race_session = setup_race(opt, &mut contents)?;

    if contents.is_empty() {
        eprintln!("Error: the provided file or language contains no words to type.");
        eprintln!("If you specified a file, make sure it isn't empty.");
        std::process::exit(1);
    }

    let mut test = build_test(contents, opt);
    if race_session.is_some() {
        test.enable_race();
    }

    Ok((test, race_session))
}

/// Builds a test using the selected CLI mode and editing options.
fn build_test(contents: Vec<String>, opt: &Opt) -> Test {
    match opt.time_limit() {
        Some(time_limit) => Test::new_with_time_limit(
            contents,
            !opt.no_backtrack,
            opt.sudden_death,
            !opt.no_backspace,
            Some(time_limit),
        ),
        None => Test::new(
            contents,
            !opt.no_backtrack,
            opt.sudden_death,
            !opt.no_backspace,
        ),
    }
}

/// Opens the requested race connection and replaces client contents with host words.
fn setup_race(opt: &Opt, contents: &mut Vec<String>) -> io::Result<Option<RaceSession>> {
    let Some(addr) = opt.race_addr() else {
        return Ok(None);
    };

    if opt.is_race_host() {
        println!("Waiting for race opponent on {addr}...");
        race::host(addr, contents)
            .map(Some)
            .map_err(|error| io::Error::new(error.kind(), format!("failed to host race: {error}")))
    } else {
        println!("Connecting to race host {addr}...");
        let (host_contents, session) = race::client(addr).map_err(|error| {
            io::Error::new(error.kind(), format!("failed to connect to race: {error}"))
        })?;
        *contents = host_contents;
        Ok(Some(session))
    }
}

/// Applies queued opponent race events to the active test.
fn apply_race_events(state: &mut State, race_session: &mut Option<RaceSession>) {
    let Some(session) = race_session.as_mut() else {
        return;
    };

    for event in session.drain_events() {
        let mut end_race = false;

        if let State::Test(test) = state {
            match event {
                RaceEvent::OpponentProgress(progress) => {
                    test.update_race_opponent(progress);
                }
                RaceEvent::OpponentFinished(progress) => {
                    test.update_race_opponent(progress);
                    let outcome = if test.completed_word_count() >= test.words.len() {
                        RaceOutcome::Tie
                    } else {
                        RaceOutcome::Lose
                    };
                    test.set_race_outcome(outcome, race_outcome_message(outcome));
                    end_race = true;
                }
                RaceEvent::Disconnected(message) => {
                    test.set_race_outcome(RaceOutcome::Disconnected, format!("Race: {message}"));
                    end_race = true;
                }
            }
        }

        if end_race {
            if let State::Test(test) = state {
                *state = State::Results(Results::from(&*test));
            }
        }
    }
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
        session.send_finish(after_progress)
    } else {
        session.send_progress(after_progress)
    };

    if let Err(error) = result {
        test.set_race_outcome(
            RaceOutcome::Disconnected,
            format!("Race: opponent disconnected ({error})"),
        );
        return true;
    }

    false
}

/// Stores a local finish outcome before showing race results.
fn finalize_local_race(test: &mut Test) {
    let Some(race) = &test.race_progress else {
        return;
    };
    if race.outcome.is_some() {
        return;
    }

    let outcome = if race.opponent >= race.total {
        RaceOutcome::Tie
    } else {
        RaceOutcome::Win
    };
    test.set_race_outcome(outcome, race_outcome_message(outcome));
}

/// Returns the results-screen label for a race outcome.
fn race_outcome_message(outcome: RaceOutcome) -> &'static str {
    match outcome {
        RaceOutcome::Win => "Race: You win",
        RaceOutcome::Lose => "Race: You lose",
        RaceOutcome::Tie => "Race: Tie",
        RaceOutcome::Disconnected => "Race: Opponent disconnected",
    }
}

fn main() -> io::Result<()> {
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
    terminal.clear()?;

    let mut state = State::Welcome;
    let mut race_session = None;
    let mut chaos = ChaosState::default();

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
        apply_race_events(&mut state, &mut race_session);
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
                    state = State::Results(Results::from(test));
                }
                State::Settings { .. } => {}
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
                    let (test, session) = start_test(&effective)?;
                    next_race_session = Some(session);
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
            State::Test(ref mut test) => {
                if let Some(Event::Key(key)) = event {
                    if key.kind == KeyEventKind::Press {
                        chaos.on_keypress(&settings);
                    }
                    let before_progress = test.completed_word_count();
                    test.handle_key(key);
                    let after_progress = test.completed_word_count();
                    chaos.observe_word_progress(&settings, before_progress, after_progress, now);
                    if report_race_progress(test, &mut race_session, before_progress) {
                        next_state = Some(State::Results(Results::from(&*test)));
                    } else if test.complete {
                        finalize_local_race(test);
                        next_state = Some(State::Results(Results::from(&*test)));
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
                    let (test, session) = start_test(&effective)?;
                    next_race_session = Some(session);
                    next_state = Some(State::Test(test));
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
                    next_state = Some(State::Test(build_test(practice_words, &effective)));
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
        if let State::Test(test) = &state {
            chaos.update_speed_demon(&settings, test, now);
        }
        let timed_out = match &state {
            State::Test(test) => {
                let effects = chaos.test_effects(&settings, test, now);
                if let Some(elapsed) = effects.accelerated_elapsed {
                    test.time_expired_after_elapsed(elapsed)
                } else if effects.time_multiplier <= 1.0 {
                    test.time_expired_at(now)
                } else {
                    test.time_expired_at_with_multiplier(now, effects.time_multiplier)
                }
            }
            _ => false,
        };
        if timed_out {
            if let State::Test(test) = &state {
                chaos.reset_test_effects();
                state = State::Results(Results::from(test));
            }
        }

        state.render_into(&mut terminal, &config, &settings, &languages, &chaos)?;
    }

    terminal::disable_raw_mode()?;
    execute!(
        io::stdout(),
        cursor::RestorePosition,
        cursor::Show,
        terminal::LeaveAlternateScreen,
    )?;

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
