//! Turns a level recipe into a concrete word list.
//!
//! Non-S ranks compose existing language corpora and layer transformations
//! (caps, punctuation, symbols, numbers, twisters) at recipe-driven rates.
//! Rank S generates adversarial tokens from a curated seed of finger-twister
//! sequences so its content is near-infinite and unmemorizable.

use super::ladder::LevelSpec;
use super::recipe::{CorpusRecipe, WordSource};
use rand::{seq::SliceRandom, Rng};
use std::collections::HashSet;

/// Resource access for corpus generation; `main` implements this over the
/// embedded resources plus the user's config-dir language overrides.
pub trait ResourceLoader {
    /// `resource` is an embedded path like "language/english200" or
    /// "rank/seed-s".
    fn load_words(&self, resource: &str) -> Option<Vec<String>>;
}

const SENTENCE_MARKS: [char; 6] = ['.', ',', '!', '?', ';', ':'];
const LIGHT_SYMBOLS: [char; 9] = ['-', '_', '/', '(', ')', '@', '#', '&', '%'];
const HEAVY_SYMBOLS: [char; 24] = [
    '{', '}', '[', ']', '<', '>', '(', ')', ';', ':', '=', '+', '*', '|', '\\', '~', '^', '`', '&',
    '$', '#', '%', '!', '?',
];
const MIX_POOL_TARGET: usize = 3000;
const SEED_FALLBACK: &[&str] = &[
    "ki;ol", "ed-un", "{[(<", ">)]}", "pl;[", "qaz!", "zx-cv", "mu7m", "ny;ny", "ce_ce",
];

/// Generates the word list for a rank level. Errors only when a source
/// corpus cannot be loaded; callers fall back to the legacy path.
pub fn generate_rank_corpus(
    spec: &LevelSpec,
    word_count: usize,
    loader: &dyn ResourceLoader,
    rng: &mut impl Rng,
) -> Result<Vec<String>, String> {
    if word_count == 0 {
        return Ok(Vec::new());
    }
    let recipe = &spec.recipe;
    let twisters = load_twisters(loader);

    let base_words: Vec<String> = match &recipe.source {
        WordSource::SGenerator => {
            let level = spec.id.level;
            let rare = loader
                .load_words("language/english-advanced")
                .ok_or_else(|| "missing corpus 'english-advanced'".to_string())?;
            let ngrams = loader
                .load_words("language/english-ngrams")
                .ok_or_else(|| "missing corpus 'english-ngrams'".to_string())?;
            let mut seen = HashSet::new();
            let mut words = Vec::with_capacity(word_count);
            while words.len() < word_count {
                let token = s_token(level, &rare, &ngrams, &twisters, rng);
                // Repetition rate 0: dedup within the session, but never spin
                // forever if the pools are tiny.
                if seen.insert(token.clone()) || seen.len() > word_count * 4 {
                    words.push(token);
                }
            }
            return Ok(words);
        }
        source => {
            let pool = load_pool(source, loader, rng)?;
            sample_words(&pool, word_count, recipe, rng)
        }
    };

    let mut words = Vec::with_capacity(word_count + word_count / 4);
    for mut word in base_words {
        if rng.gen_bool(recipe.capitalization_rate.clamp(0.0, 1.0)) {
            word = capitalize(&word, recipe.code_token_rate, rng);
        }
        if rng.gen_bool(recipe.inline_number_rate.clamp(0.0, 1.0)) {
            word = glue_number(&word, rng);
        }
        if rng.gen_bool(recipe.inline_symbol_rate.clamp(0.0, 1.0)) {
            word = glue_symbol(&word, recipe.code_token_rate, rng);
        }
        if rng.gen_bool(recipe.sentence_punctuation.clamp(0.0, 1.0)) {
            if let Some(mark) = SENTENCE_MARKS.choose(rng) {
                word.push(*mark);
            }
        }
        words.push(word);

        if rng.gen_bool((recipe.number_rate * 0.5).clamp(0.0, 1.0)) {
            words.push(rng.gen_range(0..1000).to_string());
        }
        if rng.gen_bool((recipe.finger_twister_rate * 0.4).clamp(0.0, 1.0)) {
            if let Some(twister) = twisters.choose(rng) {
                words.push(twister.clone());
            }
        }
    }

    words.truncate(word_count);
    Ok(words)
}

fn load_pool(
    source: &WordSource,
    loader: &dyn ResourceLoader,
    rng: &mut impl Rng,
) -> Result<Vec<String>, String> {
    let pool = match source {
        WordSource::Language(name) => loader
            .load_words(&format!("language/{name}"))
            .ok_or_else(|| format!("missing corpus '{name}'"))?,
        WordSource::Mix(entries) => {
            let mut pool = Vec::new();
            for (name, weight) in entries.iter() {
                let mut words = loader
                    .load_words(&format!("language/{name}"))
                    .ok_or_else(|| format!("missing corpus '{name}'"))?;
                words.shuffle(rng);
                let take = ((MIX_POOL_TARGET as f64 * weight) as usize).max(1);
                pool.extend(words.into_iter().take(take));
            }
            pool
        }
        WordSource::SGenerator => unreachable!("S corpus handled by the generator"),
    };
    if pool.is_empty() {
        return Err("rank corpus source is empty".into());
    }
    Ok(pool)
}

/// Samples `word_count` words biased toward the recipe's target length,
/// preferring unseen words when the repetition rate is low.
fn sample_words(
    pool: &[String],
    word_count: usize,
    recipe: &CorpusRecipe,
    rng: &mut impl Rng,
) -> Vec<String> {
    let mut used: HashSet<usize> = HashSet::new();
    let mut words = Vec::with_capacity(word_count);
    for _ in 0..word_count {
        let mut index = pick_length_biased(pool, recipe.mean_word_len_target, rng);
        if used.contains(&index) && !rng.gen_bool(recipe.repetition_rate.clamp(0.0, 1.0)) {
            for _ in 0..8 {
                let candidate = pick_length_biased(pool, recipe.mean_word_len_target, rng);
                if !used.contains(&candidate) {
                    index = candidate;
                    break;
                }
            }
        }
        used.insert(index);
        words.push(pool[index].clone());
    }
    words
}

/// Rejection-samples an index, preferring words near the target length.
fn pick_length_biased(pool: &[String], target_len: f64, rng: &mut impl Rng) -> usize {
    let mut best = rng.gen_range(0..pool.len());
    let mut best_distance = (pool[best].chars().count() as f64 - target_len).abs();
    for _ in 0..3 {
        let candidate = rng.gen_range(0..pool.len());
        let distance = (pool[candidate].chars().count() as f64 - target_len).abs();
        if distance < best_distance {
            best = candidate;
            best_distance = distance;
        }
    }
    best
}

fn capitalize(word: &str, code_token_rate: f64, rng: &mut impl Rng) -> String {
    let chars: Vec<char> = word.chars().collect();
    if chars.is_empty() {
        return word.to_string();
    }
    if code_token_rate >= 0.3 && chars.len() >= 4 && rng.gen_bool(0.5) {
        // camelCase or snake_case an interior position.
        let position = rng.gen_range(1..chars.len());
        let mut out = String::with_capacity(word.len() + 1);
        for (i, c) in chars.iter().enumerate() {
            if i == position {
                if rng.gen_bool(0.5) {
                    out.extend(c.to_uppercase());
                    continue;
                }
                out.push('_');
            }
            out.push(*c);
        }
        out
    } else {
        let mut out: String = chars[0].to_uppercase().collect();
        out.extend(chars[1..].iter());
        out
    }
}

fn glue_number(word: &str, rng: &mut impl Rng) -> String {
    let number = rng.gen_range(0..100);
    if rng.gen_bool(0.4) {
        format!("{word}_{number}")
    } else {
        format!("{word}{number}")
    }
}

fn glue_symbol(word: &str, code_token_rate: f64, rng: &mut impl Rng) -> String {
    let symbol = if code_token_rate >= 0.3 {
        *HEAVY_SYMBOLS.choose(rng).expect("non-empty alphabet")
    } else {
        *LIGHT_SYMBOLS.choose(rng).expect("non-empty alphabet")
    };
    match rng.gen_range(0..3) {
        0 => format!("{symbol}{word}"),
        1 => format!("{word}{symbol}"),
        _ => {
            let chars: Vec<char> = word.chars().collect();
            if chars.len() < 2 {
                return format!("{word}{symbol}");
            }
            let position = rng.gen_range(1..chars.len());
            let mut out: String = chars[..position].iter().collect();
            out.push(symbol);
            out.extend(chars[position..].iter());
            out
        }
    }
}

fn load_twisters(loader: &dyn ResourceLoader) -> Vec<String> {
    loader
        .load_words("rank/seed-s")
        .filter(|seeds| !seeds.is_empty())
        .unwrap_or_else(|| SEED_FALLBACK.iter().map(ToString::to_string).collect())
}

/// One rank-S token. Mode weights shift from word-like (mutated rare words,
/// ngram chains) at S1 toward raw twister/symbol bursts at S10.
fn s_token(
    level: u32,
    rare: &[String],
    ngrams: &[String],
    twisters: &[String],
    rng: &mut impl Rng,
) -> String {
    let t = (level.saturating_sub(1)) as f64 / 9.0;
    let weights = [
        0.35 - 0.15 * t, // A: mutated rare word
        0.20 + 0.15 * t, // B: curated twister chain
        0.30 - 0.10 * t, // C: hard-ngram pseudo-word
        0.15 + 0.10 * t, // D: symbol/digit burst
    ];
    let roll: f64 = rng.gen::<f64>() * weights.iter().sum::<f64>();

    let token = if roll < weights[0] {
        mutated_rare_word(rare, rng)
    } else if roll < weights[0] + weights[1] {
        twister_chain(twisters, rng)
    } else if roll < weights[0] + weights[1] + weights[2] {
        ngram_chain(ngrams, rng)
    } else {
        symbol_digit_burst(rng)
    };

    // Push token length toward the 9..14 band.
    if token.chars().count() < 6 {
        let extra = match rng.gen_range(0..3) {
            0 => twisters.choose(rng).cloned(),
            1 => ngrams.choose(rng).cloned(),
            _ => Some(rng.gen_range(10..100).to_string()),
        };
        if let Some(extra) = extra {
            let glue = *['-', '_', ';', ':'].choose(rng).expect("non-empty glue");
            return format!("{token}{glue}{extra}");
        }
    }
    token
}

fn mutated_rare_word(rare: &[String], rng: &mut impl Rng) -> String {
    let Some(word) = rare.choose(rng) else {
        return symbol_digit_burst(rng);
    };
    let mut out = capitalize(word, 1.0, rng);
    for _ in 0..rng.gen_range(1..=3) {
        out = if rng.gen_bool(0.5) {
            glue_symbol(&out, 1.0, rng)
        } else {
            glue_number(&out, rng)
        };
    }
    out
}

fn twister_chain(twisters: &[String], rng: &mut impl Rng) -> String {
    let count = rng.gen_range(2..=4);
    let mut parts = Vec::with_capacity(count);
    for _ in 0..count {
        if let Some(seed) = twisters.choose(rng) {
            parts.push(seed.clone());
        }
    }
    if parts.is_empty() {
        return symbol_digit_burst(rng);
    }
    let glue = *['-', ';', '_', ':'].choose(rng).expect("non-empty glue");
    parts.join(&glue.to_string())
}

fn ngram_chain(ngrams: &[String], rng: &mut impl Rng) -> String {
    let count = rng.gen_range(3..=5);
    let mut out = String::new();
    for _ in 0..count {
        if let Some(ngram) = ngrams.choose(rng) {
            out.push_str(ngram);
        }
    }
    if out.is_empty() {
        return symbol_digit_burst(rng);
    }
    out
}

fn symbol_digit_burst(rng: &mut impl Rng) -> String {
    let length = rng.gen_range(4..=8);
    let mut out = String::with_capacity(length);
    for _ in 0..length {
        if rng.gen_bool(0.4) {
            out.push(char::from_digit(rng.gen_range(0..10), 10).expect("digit"));
        } else {
            out.push(*HEAVY_SYMBOLS.choose(rng).expect("non-empty alphabet"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rank::ladder::level_spec;
    use crate::rank::{LevelId, Rank};
    use rand::{rngs::StdRng, SeedableRng};
    use std::collections::HashMap;

    struct FakeLoader(HashMap<&'static str, Vec<String>>);

    impl FakeLoader {
        fn full() -> Self {
            let mut map = HashMap::new();
            for name in [
                "language/english200",
                "language/english1000",
                "language/english-advanced",
                "language/english-ngrams",
                "language/rust",
                "language/sql",
                "language/javascript",
            ] {
                map.insert(
                    name,
                    vec![
                        "alpha",
                        "beta",
                        "gamma",
                        "delta",
                        "epsilon",
                        "zeta",
                        "longerword",
                        "shorter",
                        "tiny",
                        "monumental",
                    ]
                    .into_iter()
                    .map(String::from)
                    .collect(),
                );
            }
            map.insert(
                "rank/seed-s",
                vec!["ki;ol".into(), "{[(<".into(), "ed-un".into()],
            );
            Self(map)
        }
    }

    impl ResourceLoader for FakeLoader {
        fn load_words(&self, resource: &str) -> Option<Vec<String>> {
            self.0.get(resource).cloned()
        }
    }

    #[test]
    fn g1_corpus_is_plain_lowercase() {
        let spec = level_spec(LevelId::new(Rank::G, 1));
        let mut rng = StdRng::seed_from_u64(7);
        let words = generate_rank_corpus(&spec, 50, &FakeLoader::full(), &mut rng).unwrap();
        assert_eq!(words.len(), 50);
        assert!(words
            .iter()
            .all(|word| word.chars().all(|c| c.is_ascii_lowercase())));
    }

    #[test]
    fn s_corpus_is_unique_and_hostile() {
        let spec = level_spec(LevelId::new(Rank::S, 5));
        let mut rng = StdRng::seed_from_u64(7);
        let words = generate_rank_corpus(&spec, 50, &FakeLoader::full(), &mut rng).unwrap();
        assert_eq!(words.len(), 50);
        let unique: HashSet<&String> = words.iter().collect();
        assert_eq!(unique.len(), words.len(), "S corpus should not repeat");
        let hostile = words
            .iter()
            .filter(|word| word.chars().any(|c| !c.is_ascii_lowercase()))
            .count();
        assert!(
            hostile * 2 > words.len(),
            "most S tokens should mix case/symbols/digits"
        );
    }

    #[test]
    fn missing_corpus_is_an_error_not_a_panic() {
        let spec = level_spec(LevelId::new(Rank::C, 1));
        let mut rng = StdRng::seed_from_u64(7);
        let error =
            generate_rank_corpus(&spec, 10, &FakeLoader(HashMap::new()), &mut rng).unwrap_err();
        assert!(error.contains("missing corpus"));
    }

    #[test]
    fn higher_levels_get_harder_content() {
        let mut rng = StdRng::seed_from_u64(7);
        let easy = generate_rank_corpus(
            &level_spec(LevelId::new(Rank::E, 1)),
            200,
            &FakeLoader::full(),
            &mut rng,
        )
        .unwrap();
        let hard = generate_rank_corpus(
            &level_spec(LevelId::new(Rank::B, 10)),
            200,
            &FakeLoader::full(),
            &mut rng,
        )
        .unwrap();
        let non_alpha = |words: &[String]| {
            words
                .iter()
                .filter(|w| w.chars().any(|c| !c.is_ascii_alphabetic()))
                .count()
        };
        assert!(non_alpha(&hard) > non_alpha(&easy));
    }
}
