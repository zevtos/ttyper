//! Per-rank corpus recipes: the measurable difficulty knobs that make each
//! rank harder than the previous one. Knobs are interpolated across the 10
//! levels of a rank toward the next rank's floor values.

use super::{LevelId, Rank, LEVELS_PER_RANK};

/// Where raw words come from before transformations.
#[derive(Clone, Debug, PartialEq)]
pub enum WordSource {
    /// One embedded language corpus, e.g. "english200".
    Language(&'static str),
    /// Weighted interleave of corpora.
    Mix(&'static [(&'static str, f64)]),
    /// Algorithmic rank-S generator (see `generate::s_token`).
    SGenerator,
}

#[derive(Clone, Debug)]
pub struct CorpusRecipe {
    pub source: WordSource,
    pub mean_word_len_target: f64,
    /// Probability a word gets capitalization (first-letter or camel/snake).
    pub capitalization_rate: f64,
    /// Probability a word gets a sentence punctuation mark appended.
    pub sentence_punctuation: f64,
    /// Probability a word gets a symbol glued in.
    pub inline_symbol_rate: f64,
    /// Probability a standalone number token follows a word.
    pub number_rate: f64,
    /// Probability digits get glued into a word (e.g. `x86_64`).
    pub inline_number_rate: f64,
    /// Probability a finger-twister token is injected after a word.
    pub finger_twister_rate: f64,
    pub mixed_domain: bool,
    /// 1.0 = free repetition (muscle-memory help), 0.0 = near-unique words.
    pub repetition_rate: f64,
    /// Share of code-style tokens; also selects the heavy symbol alphabet.
    pub code_token_rate: f64,
}

const MIX_D: &[(&str, f64)] = &[("english1000", 0.7), ("english200", 0.3)];
const MIX_C: &[(&str, f64)] = &[("english-advanced", 0.7), ("english-ngrams", 0.3)];
const MIX_B: &[(&str, f64)] = &[
    ("rust", 0.25),
    ("sql", 0.15),
    ("javascript", 0.20),
    ("english-advanced", 0.40),
];
const MIX_A: &[(&str, f64)] = &[
    ("english-advanced", 0.35),
    ("rust", 0.20),
    ("javascript", 0.15),
    ("sql", 0.10),
    ("english-ngrams", 0.20),
];

fn base_recipe(rank: Rank) -> CorpusRecipe {
    match rank {
        Rank::G => CorpusRecipe {
            source: WordSource::Language("english200"),
            mean_word_len_target: 4.0,
            capitalization_rate: 0.0,
            sentence_punctuation: 0.0,
            inline_symbol_rate: 0.0,
            number_rate: 0.0,
            inline_number_rate: 0.0,
            finger_twister_rate: 0.0,
            mixed_domain: false,
            repetition_rate: 1.0,
            code_token_rate: 0.0,
        },
        Rank::F => CorpusRecipe {
            source: WordSource::Language("english1000"),
            mean_word_len_target: 4.5,
            capitalization_rate: 0.10,
            sentence_punctuation: 0.0,
            inline_symbol_rate: 0.0,
            number_rate: 0.0,
            inline_number_rate: 0.0,
            finger_twister_rate: 0.0,
            mixed_domain: false,
            repetition_rate: 0.8,
            code_token_rate: 0.0,
        },
        Rank::E => CorpusRecipe {
            source: WordSource::Language("english1000"),
            mean_word_len_target: 5.0,
            capitalization_rate: 0.30,
            sentence_punctuation: 0.25,
            inline_symbol_rate: 0.05,
            number_rate: 0.0,
            inline_number_rate: 0.0,
            finger_twister_rate: 0.05,
            mixed_domain: false,
            repetition_rate: 0.7,
            code_token_rate: 0.0,
        },
        Rank::D => CorpusRecipe {
            source: WordSource::Mix(MIX_D),
            mean_word_len_target: 5.5,
            capitalization_rate: 0.35,
            sentence_punctuation: 0.35,
            inline_symbol_rate: 0.15,
            number_rate: 0.20,
            inline_number_rate: 0.05,
            finger_twister_rate: 0.10,
            mixed_domain: false,
            repetition_rate: 0.6,
            code_token_rate: 0.0,
        },
        Rank::C => CorpusRecipe {
            source: WordSource::Mix(MIX_C),
            mean_word_len_target: 6.5,
            capitalization_rate: 0.40,
            sentence_punctuation: 0.50,
            inline_symbol_rate: 0.25,
            number_rate: 0.15,
            inline_number_rate: 0.10,
            finger_twister_rate: 0.25,
            mixed_domain: false,
            repetition_rate: 0.4,
            code_token_rate: 0.0,
        },
        Rank::B => CorpusRecipe {
            source: WordSource::Mix(MIX_B),
            mean_word_len_target: 7.0,
            capitalization_rate: 0.40,
            sentence_punctuation: 0.30,
            inline_symbol_rate: 0.60,
            number_rate: 0.20,
            inline_number_rate: 0.25,
            finger_twister_rate: 0.30,
            mixed_domain: false,
            repetition_rate: 0.35,
            code_token_rate: 0.60,
        },
        Rank::A => CorpusRecipe {
            source: WordSource::Mix(MIX_A),
            mean_word_len_target: 8.0,
            capitalization_rate: 0.45,
            sentence_punctuation: 0.50,
            inline_symbol_rate: 0.60,
            number_rate: 0.30,
            inline_number_rate: 0.30,
            finger_twister_rate: 0.45,
            mixed_domain: true,
            repetition_rate: 0.05,
            code_token_rate: 0.40,
        },
        Rank::S => CorpusRecipe {
            source: WordSource::SGenerator,
            mean_word_len_target: 9.0,
            capitalization_rate: 0.50,
            sentence_punctuation: 0.60,
            inline_symbol_rate: 0.85,
            number_rate: 0.40,
            inline_number_rate: 0.40,
            finger_twister_rate: 0.70,
            mixed_domain: true,
            repetition_rate: 0.0,
            code_token_rate: 0.50,
        },
    }
}

fn lerp(from: f64, to: f64, t: f64) -> f64 {
    from + (to - from) * t
}

/// Recipe for a specific level: the rank base, with every scalar knob
/// interpolated toward the next rank's base. S levels intensify in place.
pub fn recipe_for(id: LevelId) -> CorpusRecipe {
    let base = base_recipe(id.rank);
    let t = (id.level - 1) as f64 / f64::from(LEVELS_PER_RANK);

    let next = match id.rank.next() {
        Some(next_rank) => base_recipe(next_rank),
        // S intensifies toward its own ceiling.
        None => CorpusRecipe {
            mean_word_len_target: base.mean_word_len_target + 3.0,
            capitalization_rate: 0.6,
            sentence_punctuation: 0.7,
            inline_symbol_rate: 1.0,
            number_rate: 0.5,
            inline_number_rate: 0.5,
            finger_twister_rate: 1.0,
            repetition_rate: 0.0,
            code_token_rate: 0.6,
            ..base.clone()
        },
    };

    CorpusRecipe {
        source: base.source.clone(),
        mixed_domain: base.mixed_domain,
        mean_word_len_target: lerp(base.mean_word_len_target, next.mean_word_len_target, t),
        capitalization_rate: lerp(base.capitalization_rate, next.capitalization_rate, t),
        sentence_punctuation: lerp(base.sentence_punctuation, next.sentence_punctuation, t),
        inline_symbol_rate: lerp(base.inline_symbol_rate, next.inline_symbol_rate, t),
        number_rate: lerp(base.number_rate, next.number_rate, t),
        inline_number_rate: lerp(base.inline_number_rate, next.inline_number_rate, t),
        finger_twister_rate: lerp(base.finger_twister_rate, next.finger_twister_rate, t),
        repetition_rate: lerp(base.repetition_rate, next.repetition_rate, t),
        code_token_rate: lerp(base.code_token_rate, next.code_token_rate, t),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g1_recipe_is_plain_english200() {
        let recipe = recipe_for(LevelId::new(Rank::G, 1));
        assert_eq!(recipe.source, WordSource::Language("english200"));
        assert_eq!(recipe.capitalization_rate, 0.0);
        assert_eq!(recipe.sentence_punctuation, 0.0);
        assert_eq!(recipe.inline_symbol_rate, 0.0);
        assert_eq!(recipe.number_rate, 0.0);
        assert_eq!(recipe.repetition_rate, 1.0);
    }

    #[test]
    fn knobs_escalate_within_a_rank() {
        let early = recipe_for(LevelId::new(Rank::E, 1));
        let late = recipe_for(LevelId::new(Rank::E, 10));
        assert!(late.sentence_punctuation > early.sentence_punctuation);
        assert!(late.mean_word_len_target > early.mean_word_len_target);
        assert!(late.repetition_rate < early.repetition_rate);
    }

    #[test]
    fn s_levels_intensify() {
        let early = recipe_for(LevelId::new(Rank::S, 1));
        let late = recipe_for(LevelId::new(Rank::S, 10));
        assert_eq!(early.source, WordSource::SGenerator);
        assert!(late.finger_twister_rate > early.finger_twister_rate);
        assert!(late.inline_symbol_rate > early.inline_symbol_rate);
    }
}
