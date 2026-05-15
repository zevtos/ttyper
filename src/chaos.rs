use crate::{config::Theme, settings::Settings, test::Test, ui::TestRenderEffects};

use rand::Rng;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
};
use std::time::{Duration, Instant};

const RAINBOW_HUE_STEP: u16 = 15;
const DISCO_INTERVAL: Duration = Duration::from_millis(200);
const EARTHQUAKE_INTERVAL: Duration = Duration::from_millis(500);
const NEON_COLORS: [Color; 4] = [
    Color::Rgb(0xff, 0x00, 0xff),
    Color::Rgb(0x00, 0xff, 0xff),
    Color::Rgb(0x00, 0xff, 0x00),
    Color::Rgb(0xff, 0x66, 0x00),
];

#[derive(Debug)]
pub struct ChaosState {
    rainbow_hue: u16,
    seizure_seed: u64,
    disco_seed: u64,
    next_disco_tick: Instant,
    drunk_prompt_offset: i16,
    tiny_steps: u16,
    neon_step: usize,
    flicker_seed: u64,
    earthquake_offset: (i16, i16),
    next_earthquake_tick: Instant,
    blackout_until: Option<Instant>,
    last_blackout_word: usize,
    speed_elapsed: Duration,
    speed_last_tick: Option<Instant>,
}

impl Default for ChaosState {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            rainbow_hue: 0,
            seizure_seed: 0,
            disco_seed: 0,
            next_disco_tick: now,
            drunk_prompt_offset: 0,
            tiny_steps: 0,
            neon_step: 0,
            flicker_seed: 0,
            earthquake_offset: (0, 0),
            next_earthquake_tick: now,
            blackout_until: None,
            last_blackout_word: 0,
            speed_elapsed: Duration::from_secs(0),
            speed_last_tick: None,
        }
    }
}

impl ChaosState {
    pub fn tick(&mut self, settings: &Settings, now: Instant) {
        if settings.chaos_disco_mode {
            if now >= self.next_disco_tick {
                self.disco_seed = rand::thread_rng().gen();
                self.next_disco_tick = now + DISCO_INTERVAL;
            }
        } else {
            self.next_disco_tick = now;
        }

        if settings.chaos_earthquake_mode {
            if now >= self.next_earthquake_tick {
                self.earthquake_offset = random_earthquake_offset();
                self.next_earthquake_tick = now + EARTHQUAKE_INTERVAL;
            }
        } else {
            self.earthquake_offset = (0, 0);
            self.next_earthquake_tick = now;
        }

        if self.blackout_until.is_some_and(|until| now >= until) {
            self.blackout_until = None;
        }
    }

    pub fn on_keypress(&mut self, settings: &Settings) {
        let mut rng = rand::thread_rng();

        if settings.chaos_rainbow_mode {
            self.rainbow_hue = (self.rainbow_hue + RAINBOW_HUE_STEP) % 360;
        }
        if settings.chaos_seizure_mode {
            self.seizure_seed = rng.gen();
        }
        if settings.chaos_drunk_mode {
            self.drunk_prompt_offset = random_signed_magnitude(&mut rng, 1, 3);
        } else {
            self.drunk_prompt_offset = 0;
        }
        if settings.chaos_tiny_mode {
            self.tiny_steps = self.tiny_steps.saturating_add(1);
        }
        if settings.chaos_neon_mode {
            self.neon_step = self.neon_step.wrapping_add(1);
        }
        if settings.chaos_ghost_mode || settings.chaos_haunted_mode {
            self.flicker_seed = rng.gen();
        }
    }

    pub fn observe_word_progress(
        &mut self,
        settings: &Settings,
        before_words: usize,
        after_words: usize,
        now: Instant,
    ) {
        if !settings.chaos_blackout_mode || after_words <= before_words || after_words == 0 {
            return;
        }

        if after_words % 5 == 0 && self.last_blackout_word != after_words {
            self.blackout_until = Some(now + Duration::from_secs(1));
            self.last_blackout_word = after_words;
        }
    }

    pub fn reset_test_effects(&mut self) {
        self.drunk_prompt_offset = 0;
        self.tiny_steps = 0;
        self.blackout_until = None;
        self.last_blackout_word = 0;
        self.speed_elapsed = Duration::from_secs(0);
        self.speed_last_tick = None;
    }

    pub fn update_speed_demon(&mut self, settings: &Settings, test: &Test, now: Instant) {
        if !settings.chaos_speed_demon_mode || test.time_limit.is_none() {
            self.speed_elapsed = Duration::from_secs(0);
            self.speed_last_tick = None;
            return;
        }

        let Some(started_at) = test.started_at else {
            self.speed_elapsed = Duration::from_secs(0);
            self.speed_last_tick = None;
            return;
        };

        let last_tick = self.speed_last_tick.unwrap_or(started_at);
        let delta = now.checked_duration_since(last_tick).unwrap_or_default();
        let scaled_delta = scale_duration(
            delta,
            speed_multiplier(settings, test.completed_word_count()),
        );
        self.speed_elapsed = self.speed_elapsed.saturating_add(scaled_delta);
        self.speed_last_tick = Some(now);
    }

    pub fn apply_theme(&self, base: &Theme, settings: &Settings) -> Theme {
        let mut theme = base.clone();

        if settings.chaos_rainbow_mode {
            apply_rainbow(&mut theme, self.rainbow_hue);
        }
        if settings.chaos_seizure_mode {
            apply_seizure(&mut theme, self.seizure_seed);
        }
        if settings.chaos_neon_mode {
            apply_neon(&mut theme, self.neon_step);
        }
        if settings.chaos_disco_mode {
            apply_disco(&mut theme, self.disco_seed);
        }

        theme
    }

    pub fn earthquake_area(&self, area: Rect, settings: &Settings) -> Rect {
        if !settings.chaos_earthquake_mode {
            return area;
        }

        shifted_area(area, self.earthquake_offset)
    }

    pub fn tiny_area(&self, area: Rect, settings: &Settings) -> Rect {
        if !settings.chaos_tiny_mode || self.tiny_steps == 0 {
            return area;
        }

        let min_width = min_dimension(area.width);
        let min_height = min_dimension(area.height);
        let shrink = self.tiny_steps;
        let width = area.width.saturating_sub(shrink).max(min_width);
        let height = area.height.saturating_sub(shrink).max(min_height);

        Rect {
            x: area.x + area.width.saturating_sub(width) / 2,
            y: area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        }
    }

    pub fn test_effects(
        &self,
        settings: &Settings,
        test: &Test,
        now: Instant,
    ) -> TestRenderEffects {
        TestRenderEffects {
            mirror_prompt: settings.chaos_mirror_mode,
            ghost_mode: settings.chaos_ghost_mode,
            haunted_mode: settings.chaos_haunted_mode,
            flicker_seed: self.flicker_seed,
            drunk_prompt_offset: if settings.chaos_drunk_mode {
                self.drunk_prompt_offset
            } else {
                0
            },
            blackout_prompt: settings.chaos_blackout_mode
                && self.blackout_until.is_some_and(|until| now < until),
            time_multiplier: speed_multiplier(settings, test.completed_word_count()),
            accelerated_elapsed: if settings.chaos_speed_demon_mode && test.started_at.is_some() {
                Some(self.speed_elapsed)
            } else {
                None
            },
        }
    }
}

fn apply_rainbow(theme: &mut Theme, hue: u16) {
    theme.prompt_correct = set_fg(theme.prompt_correct, hsl_color(hue));
    theme.prompt_incorrect = set_fg(theme.prompt_incorrect, hsl_color(hue + 45));
    theme.prompt_current_correct = set_fg(theme.prompt_current_correct, hsl_color(hue + 90));
    theme.prompt_current_incorrect = set_fg(theme.prompt_current_incorrect, hsl_color(hue + 135));
    theme.prompt_current_untyped = set_fg(theme.prompt_current_untyped, hsl_color(hue + 180));
    theme.input_border = set_fg(theme.input_border, hsl_color(hue + 225));
    theme.prompt_border = set_fg(theme.prompt_border, hsl_color(hue + 270));
}

fn apply_seizure(theme: &mut Theme, seed: u64) {
    for_each_style(theme, |style, index| {
        style.fg = Some(random_color(seed, index as u64));
        style.bg = Some(random_color(seed, index as u64 + 64));
    });
}

fn apply_neon(theme: &mut Theme, step: usize) {
    for_each_style(theme, |style, index| {
        style.fg = Some(NEON_COLORS[(step + index) % NEON_COLORS.len()]);
        style.bg = Some(NEON_COLORS[(step + index + 2) % NEON_COLORS.len()]);
    });
}

fn apply_disco(theme: &mut Theme, seed: u64) {
    for_each_style(theme, |style, index| {
        let fg = style.fg.unwrap_or_else(|| random_color(seed, index as u64));
        let bg = style
            .bg
            .unwrap_or_else(|| random_color(seed, index as u64 + 128));

        if random_bool(seed, index as u64, 0) {
            style.fg = Some(bg);
            style.bg = Some(fg);
        } else {
            style.fg = Some(fg);
            style.bg = Some(bg);
        }

        *style = toggle_modifier(*style, Modifier::BOLD, random_bool(seed, index as u64, 1));
        *style = toggle_modifier(*style, Modifier::ITALIC, random_bool(seed, index as u64, 2));
        *style = toggle_modifier(
            *style,
            Modifier::UNDERLINED,
            random_bool(seed, index as u64, 3),
        );
    });
}

fn for_each_style(theme: &mut Theme, mut apply: impl FnMut(&mut Style, usize)) {
    let mut index = 0usize;
    macro_rules! each {
        ($field:ident) => {{
            apply(&mut theme.$field, index);
            index += 1;
        }};
    }

    each!(default);
    each!(title);
    each!(input_border);
    each!(prompt_border);
    each!(prompt_correct);
    each!(prompt_incorrect);
    each!(prompt_untyped);
    each!(prompt_current_correct);
    each!(prompt_current_incorrect);
    each!(prompt_current_untyped);
    each!(prompt_cursor);
    each!(results_overview);
    each!(results_overview_border);
    each!(results_worst_keys);
    each!(results_worst_keys_border);
    each!(results_chart);
    each!(results_chart_x);
    each!(results_chart_y);
    each!(results_restart_prompt);
    each!(results_timer);

    let _ = index;
}

fn set_fg(mut style: Style, color: Color) -> Style {
    style.fg = Some(color);
    style
}

fn toggle_modifier(style: Style, modifier: Modifier, enabled: bool) -> Style {
    if enabled {
        style.add_modifier(modifier)
    } else {
        style.remove_modifier(modifier)
    }
}

fn hsl_color(hue: u16) -> Color {
    let hue = f64::from(hue % 360) / 60.0;
    let chroma = 1.0;
    let x = chroma * (1.0 - (hue % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match hue as u8 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = 0.5 - chroma / 2.0;

    Color::Rgb(
        ((r1 + m) * 255.0).round() as u8,
        ((g1 + m) * 255.0).round() as u8,
        ((b1 + m) * 255.0).round() as u8,
    )
}

fn random_color(seed: u64, index: u64) -> Color {
    let value = mix(seed ^ index.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    Color::Rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

fn random_bool(seed: u64, index: u64, salt: u64) -> bool {
    mix(seed ^ index.wrapping_mul(37) ^ salt.wrapping_mul(997)) % 2 == 0
}

fn mix(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

fn random_earthquake_offset() -> (i16, i16) {
    let mut rng = rand::thread_rng();
    match rng.gen_range(0..4) {
        0 => (random_magnitude(&mut rng, 1, 2), 0),
        1 => (-random_magnitude(&mut rng, 1, 2), 0),
        2 => (0, random_magnitude(&mut rng, 1, 2)),
        _ => (0, -random_magnitude(&mut rng, 1, 2)),
    }
}

fn random_signed_magnitude(rng: &mut impl Rng, min: i16, max: i16) -> i16 {
    let magnitude = random_magnitude(rng, min, max);
    if rng.gen_bool(0.5) {
        magnitude
    } else {
        -magnitude
    }
}

fn random_magnitude(rng: &mut impl Rng, min: i16, max: i16) -> i16 {
    rng.gen_range(min..=max)
}

fn shifted_area(area: Rect, offset: (i16, i16)) -> Rect {
    let margin = 2u16.min(area.width / 2).min(area.height / 2);
    let width = area.width.saturating_sub(margin.saturating_mul(2));
    let height = area.height.saturating_sub(margin.saturating_mul(2));
    let max_x = area.x + area.width.saturating_sub(width);
    let max_y = area.y + area.height.saturating_sub(height);
    let center_x = i32::from(area.x + margin);
    let center_y = i32::from(area.y + margin);

    Rect {
        x: (center_x + i32::from(offset.0)).clamp(i32::from(area.x), i32::from(max_x)) as u16,
        y: (center_y + i32::from(offset.1)).clamp(i32::from(area.y), i32::from(max_y)) as u16,
        width,
        height,
    }
}

fn min_dimension(value: u16) -> u16 {
    ((f32::from(value) * 0.2).ceil() as u16).max(1)
}

fn speed_multiplier(settings: &Settings, completed_words: usize) -> f64 {
    if !settings.chaos_speed_demon_mode {
        return 1.0;
    }

    2f64.powi((completed_words / 10).min(16) as i32)
}

fn scale_duration(duration: Duration, multiplier: f64) -> Duration {
    if multiplier <= 1.0 {
        duration
    } else {
        Duration::from_secs_f64(duration.as_secs_f64() * multiplier)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn rainbow_keypress_advances_hue() {
        let mut chaos = ChaosState::default();
        let settings = Settings {
            chaos_rainbow_mode: true,
            ..Default::default()
        };

        chaos.on_keypress(&settings);
        let theme = chaos.apply_theme(&Theme::default(), &settings);

        assert_eq!(theme.prompt_correct.fg, Some(Color::Rgb(255, 64, 0)));
    }

    #[test]
    fn tiny_area_bottoms_out_at_twenty_percent() {
        let mut chaos = ChaosState::default();
        let settings = Settings {
            chaos_tiny_mode: true,
            ..Default::default()
        };

        for _ in 0..100 {
            chaos.on_keypress(&settings);
        }

        let area = chaos.tiny_area(Rect::new(0, 0, 100, 50), &settings);
        assert_eq!(area.width, 20);
        assert_eq!(area.height, 10);
    }

    #[test]
    fn blackout_starts_every_five_completed_words() {
        let mut chaos = ChaosState::default();
        let settings = Settings {
            chaos_blackout_mode: true,
            ..Default::default()
        };
        let now = Instant::now();

        chaos.observe_word_progress(&settings, 4, 5, now);
        assert!(chaos.blackout_until.is_some_and(|until| until > now));
    }

    #[test]
    fn speed_demon_scales_elapsed_after_word_milestones() {
        let mut chaos = ChaosState::default();
        let settings = Settings {
            chaos_speed_demon_mode: true,
            ..Default::default()
        };
        let start = Instant::now();
        let mut test = Test::new_with_time_limit(
            vec!["word".into(); 20],
            true,
            false,
            true,
            Some(Duration::from_secs(30)),
        );
        test.started_at = Some(start);

        chaos.update_speed_demon(&settings, &test, start + Duration::from_secs(5));
        test.current_word = 10;
        chaos.update_speed_demon(&settings, &test, start + Duration::from_secs(6));

        let effects = chaos.test_effects(&settings, &test, start + Duration::from_secs(6));
        assert_eq!(effects.accelerated_elapsed, Some(Duration::from_secs(7)));
    }
}
