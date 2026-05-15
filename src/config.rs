use ratatui::{
    style::{Color, Modifier, Style},
    widgets::BorderType,
};
use serde::{
    de::{self, IntoDeserializer},
    Deserialize,
};

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    pub default_language: String,
    pub theme: Theme,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_language: "english200".into(),
            theme: Theme::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Theme {
    #[serde(deserialize_with = "deserialize_style")]
    pub default: Style,
    #[serde(deserialize_with = "deserialize_style")]
    pub title: Style,

    // test widget
    #[serde(deserialize_with = "deserialize_style")]
    pub input_border: Style,
    #[serde(deserialize_with = "deserialize_style")]
    pub prompt_border: Style,

    #[serde(deserialize_with = "deserialize_border_type")]
    pub border_type: BorderType,

    #[serde(deserialize_with = "deserialize_style")]
    pub prompt_correct: Style,
    #[serde(deserialize_with = "deserialize_style")]
    pub prompt_incorrect: Style,
    #[serde(deserialize_with = "deserialize_style")]
    pub prompt_untyped: Style,

    #[serde(deserialize_with = "deserialize_style")]
    pub prompt_current_correct: Style,
    #[serde(deserialize_with = "deserialize_style")]
    pub prompt_current_incorrect: Style,
    #[serde(deserialize_with = "deserialize_style")]
    pub prompt_current_untyped: Style,

    #[serde(deserialize_with = "deserialize_style")]
    pub prompt_cursor: Style,

    // results widget
    #[serde(deserialize_with = "deserialize_style")]
    pub results_overview: Style,
    #[serde(deserialize_with = "deserialize_style")]
    pub results_overview_border: Style,

    #[serde(deserialize_with = "deserialize_style")]
    pub results_worst_keys: Style,
    #[serde(deserialize_with = "deserialize_style")]
    pub results_worst_keys_border: Style,

    #[serde(deserialize_with = "deserialize_style")]
    pub results_chart: Style,
    #[serde(deserialize_with = "deserialize_style")]
    pub results_chart_x: Style,
    #[serde(deserialize_with = "deserialize_style")]
    pub results_chart_y: Style,

    #[serde(deserialize_with = "deserialize_style")]
    pub results_restart_prompt: Style,
    #[serde(deserialize_with = "deserialize_style")]
    pub results_timer: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            default: Style::default(),

            title: Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),

            input_border: Style::default().fg(Color::Cyan),
            prompt_border: Style::default().fg(Color::Green),

            border_type: BorderType::Rounded,

            prompt_correct: Style::default().fg(Color::Green),
            prompt_incorrect: Style::default().fg(Color::Red),
            prompt_untyped: Style::default().fg(Color::Gray),

            prompt_current_correct: Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
            prompt_current_incorrect: Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            prompt_current_untyped: Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),

            prompt_cursor: Style::default().add_modifier(Modifier::UNDERLINED),

            results_overview: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            results_overview_border: Style::default().fg(Color::Cyan),

            results_worst_keys: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            results_worst_keys_border: Style::default().fg(Color::Cyan),

            results_chart: Style::default().fg(Color::Cyan),
            results_chart_x: Style::default().fg(Color::Cyan),
            results_chart_y: Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::BOLD),

            results_restart_prompt: Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::ITALIC),
            results_timer: Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        }
    }
}

pub const THEME_NAMES: [&str; 7] = [
    "Default",
    "Catppuccin",
    "Dracula",
    "Nord",
    "Gruvbox",
    "Solarized",
    "Tokyo Night",
];

pub fn theme_by_name(name: &str) -> Theme {
    match name {
        "Catppuccin" => preset_theme(PresetTheme {
            input_border: "89b4fa",
            prompt_border: "a6e3a1",
            prompt_correct: "a6e3a1",
            prompt_incorrect: "f38ba8",
            prompt_untyped: "6c7086",
            prompt_current_correct: "a6e3a1;bold",
            prompt_current_incorrect: "f38ba8;bold",
            prompt_current_untyped: "89b4fa;bold",
            results_overview: "cba6f7;bold",
            results_overview_border: "cba6f7",
            results_worst_keys: "cba6f7;bold",
            results_worst_keys_border: "cba6f7",
            results_chart: "89b4fa",
            title: "cba6f7;bold",
        }),
        "Dracula" => preset_theme(PresetTheme {
            input_border: "bd93f9",
            prompt_border: "50fa7b",
            prompt_correct: "50fa7b",
            prompt_incorrect: "ff5555",
            prompt_untyped: "6272a4",
            prompt_current_correct: "50fa7b;bold",
            prompt_current_incorrect: "ff5555;bold",
            prompt_current_untyped: "bd93f9;bold",
            results_overview: "ff79c6;bold",
            results_overview_border: "ff79c6",
            results_worst_keys: "ff79c6;bold",
            results_worst_keys_border: "ff79c6",
            results_chart: "bd93f9",
            title: "ff79c6;bold",
        }),
        "Nord" => preset_theme(PresetTheme {
            input_border: "81a1c1",
            prompt_border: "a3be8c",
            prompt_correct: "a3be8c",
            prompt_incorrect: "bf616a",
            prompt_untyped: "4c566a",
            prompt_current_correct: "a3be8c;bold",
            prompt_current_incorrect: "bf616a;bold",
            prompt_current_untyped: "81a1c1;bold",
            results_overview: "88c0d0;bold",
            results_overview_border: "88c0d0",
            results_worst_keys: "88c0d0;bold",
            results_worst_keys_border: "88c0d0",
            results_chart: "81a1c1",
            title: "88c0d0;bold",
        }),
        "Gruvbox" => preset_theme(PresetTheme {
            input_border: "83a598",
            prompt_border: "b8bb26",
            prompt_correct: "b8bb26",
            prompt_incorrect: "fb4934",
            prompt_untyped: "665c54",
            prompt_current_correct: "b8bb26;bold",
            prompt_current_incorrect: "fb4934;bold",
            prompt_current_untyped: "83a598;bold",
            results_overview: "fabd2f;bold",
            results_overview_border: "fabd2f",
            results_worst_keys: "fabd2f;bold",
            results_worst_keys_border: "fabd2f",
            results_chart: "83a598",
            title: "fabd2f;bold",
        }),
        "Solarized" => preset_theme(PresetTheme {
            input_border: "268bd2",
            prompt_border: "859900",
            prompt_correct: "859900",
            prompt_incorrect: "dc322f",
            prompt_untyped: "586e75",
            prompt_current_correct: "859900;bold",
            prompt_current_incorrect: "dc322f;bold",
            prompt_current_untyped: "268bd2;bold",
            results_overview: "2aa198;bold",
            results_overview_border: "2aa198",
            results_worst_keys: "2aa198;bold",
            results_worst_keys_border: "2aa198",
            results_chart: "268bd2",
            title: "2aa198;bold",
        }),
        "Tokyo Night" => preset_theme(PresetTheme {
            input_border: "7aa2f7",
            prompt_border: "9ece6a",
            prompt_correct: "9ece6a",
            prompt_incorrect: "f7768e",
            prompt_untyped: "565f89",
            prompt_current_correct: "9ece6a;bold",
            prompt_current_incorrect: "f7768e;bold",
            prompt_current_untyped: "7aa2f7;bold",
            results_overview: "bb9af7;bold",
            results_overview_border: "bb9af7",
            results_worst_keys: "bb9af7;bold",
            results_worst_keys_border: "bb9af7",
            results_chart: "7aa2f7",
            title: "bb9af7;bold",
        }),
        _ => Theme::default(),
    }
}

struct PresetTheme {
    input_border: &'static str,
    prompt_border: &'static str,
    prompt_correct: &'static str,
    prompt_incorrect: &'static str,
    prompt_untyped: &'static str,
    prompt_current_correct: &'static str,
    prompt_current_incorrect: &'static str,
    prompt_current_untyped: &'static str,
    results_overview: &'static str,
    results_overview_border: &'static str,
    results_worst_keys: &'static str,
    results_worst_keys_border: &'static str,
    results_chart: &'static str,
    title: &'static str,
}

fn preset_theme(preset: PresetTheme) -> Theme {
    let defaults = Theme::default();
    Theme {
        default: defaults.default,
        title: style_from_str(preset.title),
        input_border: style_from_str(preset.input_border),
        prompt_border: style_from_str(preset.prompt_border),
        border_type: defaults.border_type,
        prompt_correct: style_from_str(preset.prompt_correct),
        prompt_incorrect: style_from_str(preset.prompt_incorrect),
        prompt_untyped: style_from_str(preset.prompt_untyped),
        prompt_current_correct: style_from_str(preset.prompt_current_correct),
        prompt_current_incorrect: style_from_str(preset.prompt_current_incorrect),
        prompt_current_untyped: style_from_str(preset.prompt_current_untyped),
        prompt_cursor: defaults.prompt_cursor,
        results_overview: style_from_str(preset.results_overview),
        results_overview_border: style_from_str(preset.results_overview_border),
        results_worst_keys: style_from_str(preset.results_worst_keys),
        results_worst_keys_border: style_from_str(preset.results_worst_keys_border),
        results_chart: style_from_str(preset.results_chart),
        results_chart_x: style_from_str(preset.results_chart),
        results_chart_y: defaults.results_chart_y,
        results_restart_prompt: defaults.results_restart_prompt,
        results_timer: defaults.results_timer,
    }
}

fn style_from_str(value: &str) -> Style {
    deserialize_style(de::IntoDeserializer::<de::value::Error>::into_deserializer(
        value,
    ))
    .expect("preset theme style should be valid")
}

fn deserialize_style<'de, D>(deserializer: D) -> Result<Style, D::Error>
where
    D: de::Deserializer<'de>,
{
    struct StyleVisitor;
    impl de::Visitor<'_> for StyleVisitor {
        type Value = Style;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a string describing a text style")
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
            let (colors, modifiers) = value.split_once(';').unwrap_or((value, ""));
            let (fg, bg) = colors.split_once(':').unwrap_or((colors, "none"));

            let mut style = Style {
                fg: match fg {
                    "none" | "" => None,
                    _ => Some(deserialize_color(fg.into_deserializer())?),
                },
                bg: match bg {
                    "none" | "" => None,
                    _ => Some(deserialize_color(bg.into_deserializer())?),
                },
                ..Default::default()
            };

            for modifier in modifiers.split_terminator(';') {
                style = style.add_modifier(match modifier {
                    "bold" => Modifier::BOLD,
                    "crossed_out" => Modifier::CROSSED_OUT,
                    "dim" => Modifier::DIM,
                    "hidden" => Modifier::HIDDEN,
                    "italic" => Modifier::ITALIC,
                    "rapid_blink" => Modifier::RAPID_BLINK,
                    "slow_blink" => Modifier::SLOW_BLINK,
                    "reversed" => Modifier::REVERSED,
                    "underlined" => Modifier::UNDERLINED,
                    _ => {
                        return Err(E::invalid_value(
                            de::Unexpected::Str(modifier),
                            &"a style modifier",
                        ))
                    }
                });
            }

            Ok(style)
        }
    }

    deserializer.deserialize_str(StyleVisitor)
}

fn deserialize_color<'de, D>(deserializer: D) -> Result<Color, D::Error>
where
    D: de::Deserializer<'de>,
{
    struct ColorVisitor;
    impl de::Visitor<'_> for ColorVisitor {
        type Value = Color;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a color name or hexadecimal color code")
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
            match value {
                "reset" => Ok(Color::Reset),
                "black" => Ok(Color::Black),
                "white" => Ok(Color::White),
                "red" => Ok(Color::Red),
                "green" => Ok(Color::Green),
                "yellow" => Ok(Color::Yellow),
                "blue" => Ok(Color::Blue),
                "magenta" => Ok(Color::Magenta),
                "cyan" => Ok(Color::Cyan),
                "gray" => Ok(Color::Gray),
                "darkgray" => Ok(Color::DarkGray),
                "lightred" => Ok(Color::LightRed),
                "lightgreen" => Ok(Color::LightGreen),
                "lightyellow" => Ok(Color::LightYellow),
                "lightblue" => Ok(Color::LightBlue),
                "lightmagenta" => Ok(Color::LightMagenta),
                "lightcyan" => Ok(Color::LightCyan),
                _ => {
                    if value.len() == 6 {
                        let parse_error = |_| E::custom("color code was not valid hexadecimal");

                        Ok(Color::Rgb(
                            u8::from_str_radix(&value[0..2], 16).map_err(parse_error)?,
                            u8::from_str_radix(&value[2..4], 16).map_err(parse_error)?,
                            u8::from_str_radix(&value[4..6], 16).map_err(parse_error)?,
                        ))
                    } else {
                        Err(E::invalid_value(
                            de::Unexpected::Str(value),
                            &"a color name or hexadecimal color code",
                        ))
                    }
                }
            }
        }
    }

    deserializer.deserialize_str(ColorVisitor)
}

fn deserialize_border_type<'de, D>(deserializer: D) -> Result<BorderType, D::Error>
where
    D: de::Deserializer<'de>,
{
    struct BorderTypeVisitor;
    impl de::Visitor<'_> for BorderTypeVisitor {
        type Value = BorderType;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a border type")
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
            match value {
                "plain" => Ok(BorderType::Plain),
                "rounded" => Ok(BorderType::Rounded),
                "double" => Ok(BorderType::Double),
                "thick" => Ok(BorderType::Thick),
                "quadrantinside" => Ok(BorderType::QuadrantInside),
                "quadrantoutside" => Ok(BorderType::QuadrantOutside),
                _ => Err(E::invalid_value(
                    de::Unexpected::Str(value),
                    &"a border type",
                )),
            }
        }
    }

    deserializer.deserialize_str(BorderTypeVisitor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_basic_colors() {
        fn color(string: &str) -> Color {
            deserialize_color(de::IntoDeserializer::<de::value::Error>::into_deserializer(
                string,
            ))
            .expect("failed to deserialize color")
        }

        assert_eq!(color("black"), Color::Black);
        assert_eq!(color("000000"), Color::Rgb(0, 0, 0));
        assert_eq!(color("ffffff"), Color::Rgb(0xff, 0xff, 0xff));
        assert_eq!(color("FFFFFF"), Color::Rgb(0xff, 0xff, 0xff));
    }

    #[test]
    fn deserializes_styles() {
        fn style(string: &str) -> Style {
            deserialize_style(de::IntoDeserializer::<de::value::Error>::into_deserializer(
                string,
            ))
            .expect("failed to deserialize style")
        }

        assert_eq!(style("none"), Style::default());
        assert_eq!(style("none:none"), Style::default());
        assert_eq!(style("none:none;"), Style::default());

        assert_eq!(style("black"), Style::default().fg(Color::Black));
        assert_eq!(
            style("black:white"),
            Style::default().fg(Color::Black).bg(Color::White)
        );

        assert_eq!(
            style("none;bold"),
            Style::default().add_modifier(Modifier::BOLD)
        );
        assert_eq!(
            style("none;bold;italic;underlined;"),
            Style::default()
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::ITALIC)
                .add_modifier(Modifier::UNDERLINED)
        );

        assert_eq!(
            style("00ff00:000000;bold;dim;italic;slow_blink"),
            Style::default()
                .fg(Color::Rgb(0, 0xff, 0))
                .bg(Color::Rgb(0, 0, 0))
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::DIM)
                .add_modifier(Modifier::ITALIC)
                .add_modifier(Modifier::SLOW_BLINK)
        );
    }

    #[test]
    fn deserializes_border_types() {
        fn border_type(string: &str) -> BorderType {
            deserialize_border_type(de::IntoDeserializer::<de::value::Error>::into_deserializer(
                string,
            ))
            .expect("failed to deserialize border type")
        }
        assert_eq!(border_type("plain"), BorderType::Plain);
        assert_eq!(border_type("rounded"), BorderType::Rounded);
        assert_eq!(border_type("double"), BorderType::Double);
        assert_eq!(border_type("thick"), BorderType::Thick);
        assert_eq!(border_type("quadrantinside"), BorderType::QuadrantInside);
        assert_eq!(border_type("quadrantoutside"), BorderType::QuadrantOutside);
    }
}
