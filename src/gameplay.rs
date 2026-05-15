use clap::ValueEnum;
use rand::{seq::SliceRandom, Rng};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
#[clap(rename_all = "kebab-case")]
pub enum GameplayFeature {
    LivesSystem,
    ComboMultiplier,
    WordRush,
    CheckpointMode,
    SuddenDeathPlus,
    PracticeWordLock,
    StreakSaver,
    GhostRace,
    AdaptiveSpeed,
    ShrinkingWordPool,
    BossRush,
    PenaltyWords,
    BonusWords,
    DoublePointsWord,
    NoRepeatMode,
    SpeedRamp,
    SlowStartMode,
    RandomWordLengthBurst,
    MirrorTyping,
    AnagramMode,
    FirstLetterHintOnly,
    LastLetterHintOnly,
    VowelBlackout,
    ConsonantBlackout,
    OneWordAtATime,
    TimedPerWord,
    DisappearingText,
    JumbledSentence,
    ReverseSentence,
    CaseSensitivityMode,
    SilentLettersMode,
    ExtraLetterTrap,
    MissingLetterTrap,
    WordSwapTrick,
    FadingPrompt,
    AcceleratingCursor,
    FreezePowerUp,
    DoubleTimePowerUp,
    WordShield,
    RicochetMode,
    ComboBreakerWord,
    RandomRestartThreat,
    EnduranceMode,
    PointBuyMode,
    WordAuction,
    NegativeSpaceMode,
    RelayMode,
    PrecisionMode,
    CrescendoMode,
    PrestigeChallenge,
    PowerMode,
}

pub const ALL_GAMEPLAY_FEATURES: [GameplayFeature; 51] = [
    GameplayFeature::PowerMode,
    GameplayFeature::LivesSystem,
    GameplayFeature::ComboMultiplier,
    GameplayFeature::WordRush,
    GameplayFeature::CheckpointMode,
    GameplayFeature::SuddenDeathPlus,
    GameplayFeature::PracticeWordLock,
    GameplayFeature::StreakSaver,
    GameplayFeature::GhostRace,
    GameplayFeature::AdaptiveSpeed,
    GameplayFeature::ShrinkingWordPool,
    GameplayFeature::BossRush,
    GameplayFeature::PenaltyWords,
    GameplayFeature::BonusWords,
    GameplayFeature::DoublePointsWord,
    GameplayFeature::NoRepeatMode,
    GameplayFeature::SpeedRamp,
    GameplayFeature::SlowStartMode,
    GameplayFeature::RandomWordLengthBurst,
    GameplayFeature::MirrorTyping,
    GameplayFeature::AnagramMode,
    GameplayFeature::FirstLetterHintOnly,
    GameplayFeature::LastLetterHintOnly,
    GameplayFeature::VowelBlackout,
    GameplayFeature::ConsonantBlackout,
    GameplayFeature::OneWordAtATime,
    GameplayFeature::TimedPerWord,
    GameplayFeature::DisappearingText,
    GameplayFeature::JumbledSentence,
    GameplayFeature::ReverseSentence,
    GameplayFeature::CaseSensitivityMode,
    GameplayFeature::SilentLettersMode,
    GameplayFeature::ExtraLetterTrap,
    GameplayFeature::MissingLetterTrap,
    GameplayFeature::WordSwapTrick,
    GameplayFeature::FadingPrompt,
    GameplayFeature::AcceleratingCursor,
    GameplayFeature::FreezePowerUp,
    GameplayFeature::DoubleTimePowerUp,
    GameplayFeature::WordShield,
    GameplayFeature::RicochetMode,
    GameplayFeature::ComboBreakerWord,
    GameplayFeature::RandomRestartThreat,
    GameplayFeature::EnduranceMode,
    GameplayFeature::PointBuyMode,
    GameplayFeature::WordAuction,
    GameplayFeature::NegativeSpaceMode,
    GameplayFeature::RelayMode,
    GameplayFeature::PrecisionMode,
    GameplayFeature::CrescendoMode,
    GameplayFeature::PrestigeChallenge,
];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum WordKind {
    #[default]
    Normal,
    Penalty,
    Bonus,
    DoublePoints,
    Boss,
    ComboBreaker,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedWord {
    pub text: String,
    pub display: String,
    pub kind: WordKind,
}

impl From<String> for PreparedWord {
    fn from(text: String) -> Self {
        Self {
            display: text.clone(),
            text,
            kind: WordKind::Normal,
        }
    }
}

impl From<&str> for PreparedWord {
    fn from(text: &str) -> Self {
        Self::from(text.to_string())
    }
}

impl GameplayFeature {
    pub fn label(self) -> &'static str {
        match self {
            Self::LivesSystem => "Lives System",
            Self::ComboMultiplier => "Combo Multiplier",
            Self::WordRush => "Word Rush",
            Self::CheckpointMode => "Checkpoint Mode",
            Self::SuddenDeathPlus => "Sudden Death Plus",
            Self::PracticeWordLock => "Practice Word Lock",
            Self::StreakSaver => "Streak Saver",
            Self::GhostRace => "Ghost Race",
            Self::AdaptiveSpeed => "Adaptive Speed",
            Self::ShrinkingWordPool => "Shrinking Word Pool",
            Self::BossRush => "Boss Rush",
            Self::PenaltyWords => "Penalty Words",
            Self::BonusWords => "Bonus Words",
            Self::DoublePointsWord => "Double Points Word",
            Self::NoRepeatMode => "No Repeat Mode",
            Self::SpeedRamp => "Speed Ramp",
            Self::SlowStartMode => "Slow Start Mode",
            Self::RandomWordLengthBurst => "Random Word Length Burst",
            Self::MirrorTyping => "Mirror Typing",
            Self::AnagramMode => "Anagram Mode",
            Self::FirstLetterHintOnly => "First Letter Hint Only",
            Self::LastLetterHintOnly => "Last Letter Hint Only",
            Self::VowelBlackout => "Vowel Blackout",
            Self::ConsonantBlackout => "Consonant Blackout",
            Self::OneWordAtATime => "One Word At A Time",
            Self::TimedPerWord => "Timed Per Word",
            Self::DisappearingText => "Disappearing Text",
            Self::JumbledSentence => "Jumbled Sentence",
            Self::ReverseSentence => "Reverse Sentence",
            Self::CaseSensitivityMode => "Case Sensitivity Mode",
            Self::SilentLettersMode => "Silent Letters Mode",
            Self::ExtraLetterTrap => "Extra Letter Trap",
            Self::MissingLetterTrap => "Missing Letter Trap",
            Self::WordSwapTrick => "Word Swap Trick",
            Self::FadingPrompt => "Fading Prompt",
            Self::AcceleratingCursor => "Accelerating Cursor",
            Self::FreezePowerUp => "Freeze Power-up",
            Self::DoubleTimePowerUp => "Double Time Power-up",
            Self::WordShield => "Word Shield",
            Self::RicochetMode => "Ricochet Mode",
            Self::ComboBreakerWord => "Combo Breaker Word",
            Self::RandomRestartThreat => "Random Restart Threat",
            Self::EnduranceMode => "Endurance Mode",
            Self::PointBuyMode => "Point Buy Mode",
            Self::WordAuction => "Word Auction",
            Self::NegativeSpaceMode => "Negative Space Mode",
            Self::RelayMode => "Relay Mode",
            Self::PrecisionMode => "Precision Mode",
            Self::CrescendoMode => "Crescendo Mode",
            Self::PrestigeChallenge => "Prestige Challenge",
            Self::PowerMode => "Power Mode",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::LivesSystem => "Five lives; mistakes cost one life.",
            Self::ComboMultiplier => "Builds a score multiplier from your combo.",
            Self::WordRush => "Auto-submits each word on a short timer.",
            Self::CheckpointMode => "Requires higher WPM every 10 correct words.",
            Self::SuddenDeathPlus => "Ends the test on the first mistake.",
            Self::PracticeWordLock => "Missed words return later for more practice.",
            Self::StreakSaver => "Earns mistake blockers from clean streaks.",
            Self::GhostRace => "Shows progress against your saved best WPM.",
            Self::AdaptiveSpeed => "Hardens upcoming words when you type fast.",
            Self::ShrinkingWordPool => "Removes duplicate words from the pool.",
            Self::BossRush => "Adds very long boss words at intervals.",
            Self::PenaltyWords => "Adds trap words you should skip.",
            Self::BonusWords => "Adds bonus words that grant extra time.",
            Self::DoublePointsWord => "Adds one word that doubles your score multiplier.",
            Self::NoRepeatMode => "Prevents repeated words in a test.",
            Self::SpeedRamp => "Raises the required WPM over time.",
            Self::SlowStartMode => "Starts with shorter words before harder ones.",
            Self::RandomWordLengthBurst => "Adds bursts of longer words.",
            Self::MirrorTyping => "Shows each prompt reversed.",
            Self::AnagramMode => "Scrambles letters in each prompt.",
            Self::FirstLetterHintOnly => "Only the first letter stays visible.",
            Self::LastLetterHintOnly => "Only the last letter stays visible.",
            Self::VowelBlackout => "Masks vowels in the prompt.",
            Self::ConsonantBlackout => "Masks consonants in the prompt.",
            Self::OneWordAtATime => "Shows only the current word.",
            Self::TimedPerWord => "Forces a miss after 3 seconds per word.",
            Self::DisappearingText => "Correct typed characters disappear.",
            Self::JumbledSentence => "Shuffles word groups in the prompt.",
            Self::ReverseSentence => "Displays the prompt in reverse order.",
            Self::CaseSensitivityMode => "Mixes uppercase and lowercase targets.",
            Self::SilentLettersMode => "Marks one displayed letter to skip.",
            Self::ExtraLetterTrap => "Adds fake displayed letters.",
            Self::MissingLetterTrap => "Hides a letter you still need to type.",
            Self::WordSwapTrick => "Periodically swaps upcoming word pairs.",
            Self::FadingPrompt => "Turns later prompts into dots.",
            Self::AcceleratingCursor => "Speeds up timed tests as you progress.",
            Self::FreezePowerUp => "Adds time every 20 correct words.",
            Self::DoubleTimePowerUp => "Adds a larger time bonus every 30 words.",
            Self::WordShield => "Blocks one failed word.",
            Self::RicochetMode => "Wrong letters clear the current word.",
            Self::ComboBreakerWord => "Adds words that can reset your multiplier.",
            Self::RandomRestartThreat => "Sometimes requires typing safe quickly.",
            Self::EnduranceMode => "Uses a long survival run with lives.",
            Self::PointBuyMode => "Starts with shield, savers, and bonus time.",
            Self::WordAuction => "Mixes short, normal, and long word picks.",
            Self::NegativeSpaceMode => "Hides completed words as blank space.",
            Self::RelayMode => "Alternates normal words with code tokens.",
            Self::PrecisionMode => "Freezes input briefly after mistakes.",
            Self::CrescendoMode => "Extends the round with harder words.",
            Self::PrestigeChallenge => "Hardens short words and speeds the timer.",
            Self::PowerMode => "Shows combo status and burst visuals.",
        }
    }
}

pub fn normalize_features(features: &[GameplayFeature]) -> Vec<GameplayFeature> {
    features
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub fn feature_set(features: &[GameplayFeature]) -> BTreeSet<GameplayFeature> {
    features.iter().copied().collect()
}

pub fn contains(features: &BTreeSet<GameplayFeature>, feature: GameplayFeature) -> bool {
    features.contains(&feature)
}

pub fn prepare_words(
    words: Vec<String>,
    features: &BTreeSet<GameplayFeature>,
) -> Vec<PreparedWord> {
    let mut rng = rand::thread_rng();
    let mut words = if contains(features, GameplayFeature::NoRepeatMode)
        || contains(features, GameplayFeature::ShrinkingWordPool)
    {
        unique_words(words)
    } else {
        words
    };

    if contains(features, GameplayFeature::RelayMode) {
        apply_relay_blocks(&mut words);
    }
    if contains(features, GameplayFeature::WordAuction) {
        words = auction_pick(words);
    }
    if contains(features, GameplayFeature::JumbledSentence) {
        shuffle_sentence_chunks(&mut words, &mut rng);
    }
    if contains(features, GameplayFeature::SlowStartMode) {
        words = slow_start_order(words);
    }
    if contains(features, GameplayFeature::PrestigeChallenge) {
        words = words.into_iter().map(prestige_word).collect();
    }
    if contains(features, GameplayFeature::RandomWordLengthBurst) {
        words = insert_length_bursts(words);
    }
    if contains(features, GameplayFeature::BossRush) {
        words = insert_boss_words(words);
    }
    if contains(features, GameplayFeature::CrescendoMode)
        || contains(features, GameplayFeature::AdaptiveSpeed)
    {
        apply_progressive_difficulty(&mut words);
    }

    let double_points_index = if contains(features, GameplayFeature::DoublePointsWord) {
        Some(words.len().saturating_div(2))
    } else {
        None
    };

    words
        .into_iter()
        .enumerate()
        .map(|(index, word)| {
            let mut prepared = PreparedWord::from(apply_case_mode(word, features, &mut rng));
            prepared.kind = word_kind_for(index, double_points_index, features, &mut rng);
            prepared = apply_prompt_traps(prepared, index, features, &mut rng);
            prepared.display = apply_visual_modes(&prepared.display, index, features, &mut rng);
            prepared
        })
        .collect()
}

fn unique_words(words: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    words
        .into_iter()
        .filter(|word| seen.insert(word.to_lowercase()))
        .collect()
}

fn apply_relay_blocks(words: &mut [String]) {
    const CODE_WORDS: [&str; 12] = [
        "fn", "let", "mut", "impl", "match", "async", "await", "struct", "enum", "trait", "where",
        "loop",
    ];

    for (index, word) in words.iter_mut().enumerate() {
        if (index / 10) % 2 == 1 {
            *word = CODE_WORDS[index % CODE_WORDS.len()].into();
        }
    }
}

fn auction_pick(words: Vec<String>) -> Vec<String> {
    let mut short = words.clone();
    short.sort_by_key(|word| word.chars().count());
    let mut long = words.clone();
    long.sort_by_key(|word| std::cmp::Reverse(word.chars().count()));

    if words.len() < 3 {
        return words;
    }

    words
        .into_iter()
        .enumerate()
        .map(|(index, word)| match index % 3 {
            0 => short.get(index).cloned().unwrap_or(word),
            1 => word,
            _ => long.get(index).cloned().unwrap_or(word),
        })
        .collect()
}

fn shuffle_sentence_chunks(words: &mut [String], rng: &mut impl Rng) {
    for chunk in words.chunks_mut(8) {
        chunk.shuffle(rng);
    }
}

fn slow_start_order(mut words: Vec<String>) -> Vec<String> {
    words.sort_by_key(|word| word.chars().count());
    if words.len() <= 10 {
        return words;
    }

    let mut easy = words.drain(..10).collect::<Vec<_>>();
    words.sort_by_key(|word| std::cmp::Reverse(word.chars().count()));
    easy.extend(words);
    easy
}

fn prestige_word(word: String) -> String {
    if word.chars().count() >= 8 {
        word
    } else {
        harden_word(&word, 8)
    }
}

fn insert_length_bursts(words: Vec<String>) -> Vec<String> {
    let long_words = longest_words(&words, 5);
    let mut output = Vec::with_capacity(words.len() + words.len() / 15 * 5);
    for (index, word) in words.into_iter().enumerate() {
        output.push(word);
        if (index + 1) % 15 == 0 {
            output.extend(long_words.iter().cloned());
        }
    }
    output
}

fn insert_boss_words(words: Vec<String>) -> Vec<String> {
    let mut output = Vec::with_capacity(words.len() + words.len() / 25);
    for (index, word) in words.into_iter().enumerate() {
        output.push(word);
        if (index + 1) % 25 == 0 {
            output.push("ultramicroscopicsilicovolcanoconiosis".into());
        }
    }
    output
}

fn apply_progressive_difficulty(words: &mut [String]) {
    for (index, word) in words.iter_mut().enumerate() {
        let minimum = 4 + (index / 20).min(8);
        if word.chars().count() < minimum {
            *word = harden_word(word, minimum);
        }
    }
}

fn longest_words(words: &[String], count: usize) -> Vec<String> {
    let mut sorted = words.to_vec();
    sorted.sort_by_key(|word| std::cmp::Reverse(word.chars().count()));
    sorted
        .into_iter()
        .take(count)
        .map(|word| harden_word(&word, 12))
        .collect()
}

pub fn harden_word(word: &str, minimum_length: usize) -> String {
    const SUFFIX: &str = "complexity";
    let mut hardened = word.to_string();
    let mut suffix = SUFFIX.chars().cycle();
    while hardened.chars().count() < minimum_length {
        if let Some(character) = suffix.next() {
            hardened.push(character);
        }
    }
    hardened
}

fn apply_case_mode(
    word: String,
    features: &BTreeSet<GameplayFeature>,
    rng: &mut impl Rng,
) -> String {
    if !contains(features, GameplayFeature::CaseSensitivityMode) {
        return word;
    }

    word.chars()
        .map(|character| {
            if rng.gen_bool(0.5) {
                character.to_ascii_uppercase()
            } else {
                character.to_ascii_lowercase()
            }
        })
        .collect()
}

fn word_kind_for(
    index: usize,
    double_points_index: Option<usize>,
    features: &BTreeSet<GameplayFeature>,
    rng: &mut impl Rng,
) -> WordKind {
    if double_points_index == Some(index) {
        return WordKind::DoublePoints;
    }
    if contains(features, GameplayFeature::BossRush) && (index + 1).is_multiple_of(26) {
        return WordKind::Boss;
    }
    if contains(features, GameplayFeature::ComboBreakerWord)
        && index > 0
        && index.is_multiple_of(18)
    {
        return WordKind::ComboBreaker;
    }
    if contains(features, GameplayFeature::PenaltyWords) && rng.gen_ratio(1, 14) {
        return WordKind::Penalty;
    }
    if contains(features, GameplayFeature::BonusWords) && rng.gen_ratio(1, 12) {
        return WordKind::Bonus;
    }

    WordKind::Normal
}

fn apply_prompt_traps(
    mut word: PreparedWord,
    index: usize,
    features: &BTreeSet<GameplayFeature>,
    _rng: &mut impl Rng,
) -> PreparedWord {
    if contains(features, GameplayFeature::SilentLettersMode) && word.text.chars().count() > 3 {
        let char_index = index % word.text.chars().count();
        let mut expected = String::new();
        let mut display = String::new();
        for (current, character) in word.text.chars().enumerate() {
            if current == char_index {
                display.push('[');
                display.push(character);
                display.push(']');
            } else {
                expected.push(character);
                display.push(character);
            }
        }
        word.text = expected;
        word.display = display;
    }

    if contains(features, GameplayFeature::ExtraLetterTrap) && !word.display.is_empty() {
        let fake = (b'a' + (index % 26) as u8) as char;
        word.display = insert_char(&word.display, index, fake);
    }

    if contains(features, GameplayFeature::MissingLetterTrap) && word.display.chars().count() > 3 {
        word.display = remove_char(&word.display, index);
    }

    if word.kind == WordKind::Penalty {
        word.display = format!("!{}", word.display);
    }
    if word.kind == WordKind::Bonus {
        word.display = format!("+{}", word.display);
    }
    if word.kind == WordKind::DoublePoints {
        word.display = format!("x2:{}", word.display);
    }
    if word.kind == WordKind::Boss {
        word.display = format!("BOSS:{}", word.display);
    }
    if word.kind == WordKind::ComboBreaker {
        word.display = format!("CB:{}", word.display);
    }

    word
}

fn apply_visual_modes(
    display: &str,
    index: usize,
    features: &BTreeSet<GameplayFeature>,
    rng: &mut impl Rng,
) -> String {
    let mut display = display.to_string();

    if contains(features, GameplayFeature::AnagramMode) {
        let mut chars = display.chars().collect::<Vec<_>>();
        chars.shuffle(rng);
        display = chars.into_iter().collect();
    }
    if contains(features, GameplayFeature::MirrorTyping) {
        display = display.chars().rev().collect();
    }
    if contains(features, GameplayFeature::FirstLetterHintOnly) {
        display = first_letter_hint(&display);
    }
    if contains(features, GameplayFeature::LastLetterHintOnly) {
        display = last_letter_hint(&display);
    }
    if contains(features, GameplayFeature::VowelBlackout) {
        display = mask_matching(&display, is_vowel);
    }
    if contains(features, GameplayFeature::ConsonantBlackout) {
        display = mask_matching(&display, |character| {
            character.is_ascii_alphabetic() && !is_vowel(character)
        });
    }
    if contains(features, GameplayFeature::FadingPrompt) && index > 0 {
        display = display
            .chars()
            .map(|character| {
                if character.is_whitespace() {
                    character
                } else {
                    '.'
                }
            })
            .collect();
    }

    display
}

fn insert_char(value: &str, index: usize, character: char) -> String {
    let char_count = value.chars().count();
    let target = if char_count == 0 {
        0
    } else {
        index % char_count
    };
    let mut output = String::new();
    for (current, existing) in value.chars().enumerate() {
        if current == target {
            output.push(character);
        }
        output.push(existing);
    }
    output
}

fn remove_char(value: &str, index: usize) -> String {
    let char_count = value.chars().count();
    let target = index % char_count;
    value
        .chars()
        .enumerate()
        .filter_map(|(current, character)| (current != target).then_some(character))
        .collect()
}

fn first_letter_hint(value: &str) -> String {
    hint_only(value, true)
}

fn last_letter_hint(value: &str) -> String {
    hint_only(value, false)
}

fn hint_only(value: &str, keep_first: bool) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    let last = chars.len().saturating_sub(1);
    chars
        .into_iter()
        .enumerate()
        .map(|(index, character)| {
            let keep = if keep_first {
                index == 0
            } else {
                index == last
            };
            if keep || !character.is_ascii_alphabetic() {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn mask_matching(value: &str, matches: impl Fn(char) -> bool) -> String {
    value
        .chars()
        .map(|character| if matches(character) { '_' } else { character })
        .collect()
}

fn is_vowel(character: char) -> bool {
    matches!(character.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_repeat_mode_removes_duplicate_words() {
        let features = feature_set(&[GameplayFeature::NoRepeatMode]);
        let words = prepare_words(vec!["rust".into(), "Rust".into(), "type".into()], &features);

        assert_eq!(
            words.into_iter().map(|word| word.text).collect::<Vec<_>>(),
            vec!["rust", "type"]
        );
    }

    #[test]
    fn prestige_words_are_at_least_eight_characters() {
        let features = feature_set(&[GameplayFeature::PrestigeChallenge]);
        let words = prepare_words(vec!["go".into(), "rustacean".into()], &features);

        assert!(words.iter().all(|word| word.text.chars().count() >= 8));
    }

    #[test]
    fn boss_rush_inserts_boss_words_after_twenty_five_words() {
        let features = feature_set(&[GameplayFeature::BossRush]);
        let words = prepare_words(vec!["word".into(); 25], &features);

        assert!(words.iter().any(|word| word.kind == WordKind::Boss));
    }

    #[test]
    fn all_features_have_short_descriptions() {
        for feature in ALL_GAMEPLAY_FEATURES {
            assert!(!feature.description().is_empty());
        }
    }
}
