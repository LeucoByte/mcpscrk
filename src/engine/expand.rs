//! Per-word expansion: capitalization and leet speak.
//!
//! Two transformations, both designed to satisfy real-world complexity policies
//! the way a human would, while keeping the output set under control.

use std::collections::HashSet;

/// Above this many variable positions we stop doing the full cartesian product
/// and fall back to a minimal, "strategic" expansion. This is what keeps a long
/// word from exploding into millions of pointless variants.
const COMBINATORIAL_CAP: usize = 12;

/// Capitalization strategy applied to a word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapMode {
    /// Keep exactly what the user typed.
    Exact,
    /// lowercase, UPPERCASE and Title case only (the cheap, common forms).
    Minimal,
    /// Every per-letter upper/lower combination (ala, aLa, Ala, ALA, ...).
    Matrix,
}

impl CapMode {
    /// Parse the mode coming from the UI; unknown values fall back to `Exact`.
    pub fn parse(s: &str) -> CapMode {
        match s {
            "minimal" => CapMode::Minimal,
            "matrix" => CapMode::Matrix,
            _ => CapMode::Exact,
        }
    }
}

/// Common leet substitutions: each letter maps to the digits/symbols people
/// actually use to disguise it.
const LEET_TABLE: &[(char, &[char])] = &[
    ('a', &['4']),
    ('e', &['3']),
    ('i', &['1', '!']),
    ('o', &['0']),
    ('s', &['5', '$']),
    ('t', &['7', '+']),
    ('l', &['1']),
    ('b', &['8', '6']),
    ('g', &['6', '9']),
    ('z', &['2']),
];

/// Return the leet substitutes for a character, matched case-insensitively.
fn leet_subs(c: char) -> Option<&'static [char]> {
    let lower = c.to_ascii_lowercase();
    LEET_TABLE.iter().find(|(k, _)| *k == lower).map(|(_, v)| *v)
}

/// Push `value` into `out` only if it has not been seen yet, preserving order.
fn push_unique(out: &mut Vec<String>, seen: &mut HashSet<String>, value: String) {
    if seen.insert(value.clone()) {
        out.push(value);
    }
}

/// Generate the capitalization variants of a word for the given mode.
///
/// Non-alphabetic characters are always left untouched; only ASCII letters are
/// cased. The result is de-duplicated and order-stable.
pub fn capitalize(word: &str, mode: CapMode) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    match mode {
        CapMode::Exact => push_unique(&mut out, &mut seen, word.to_string()),
        CapMode::Minimal => {
            push_unique(&mut out, &mut seen, word.to_lowercase());
            push_unique(&mut out, &mut seen, word.to_uppercase());
            push_unique(&mut out, &mut seen, title_case(word));
        }
        CapMode::Matrix => {
            let chars: Vec<char> = word.chars().collect();
            let letters = chars.iter().filter(|c| c.is_ascii_alphabetic()).count();
            // Guard against blowup: 2^letters variants would be too many.
            if letters > COMBINATORIAL_CAP {
                return capitalize(word, CapMode::Minimal);
            }
            // Walk all 2^letters combinations using a bit per letter position.
            for mask in 0u32..(1u32 << letters) {
                let mut variant = String::with_capacity(word.len());
                let mut bit = 0;
                for &c in &chars {
                    if c.is_ascii_alphabetic() {
                        if (mask >> bit) & 1 == 1 {
                            variant.push(c.to_ascii_uppercase());
                        } else {
                            variant.push(c.to_ascii_lowercase());
                        }
                        bit += 1;
                    } else {
                        variant.push(c);
                    }
                }
                push_unique(&mut out, &mut seen, variant);
            }
        }
    }
    out
}

/// Title case: first letter uppercase, the rest lowercase.
fn title_case(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().chain(chars.flat_map(|c| c.to_lowercase())).collect(),
    }
}

/// Generate the leet variants of a single word, including the original.
///
/// When the word has few leetable characters we produce the full cartesian
/// product (every position kept as-is or replaced by each of its substitutes).
/// When there are too many, we fall back to a strategic expansion that only
/// touches the first leetable character, which is what most people do in
/// practice (e.g. `M4rco`).
pub fn leet_variants(word: &str) -> Vec<String> {
    let chars: Vec<char> = word.chars().collect();
    let positions: Vec<usize> = chars
        .iter()
        .enumerate()
        .filter(|(_, c)| leet_subs(**c).is_some())
        .map(|(i, _)| i)
        .collect();

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    push_unique(&mut out, &mut seen, word.to_string());

    if positions.is_empty() {
        return out;
    }

    if positions.len() > COMBINATORIAL_CAP {
        // Strategic: only mutate the first leetable character.
        let pos = positions[0];
        if let Some(subs) = leet_subs(chars[pos]) {
            for &sub in subs {
                let mut variant = chars.clone();
                variant[pos] = sub;
                push_unique(&mut out, &mut seen, variant.into_iter().collect());
            }
        }
        return out;
    }

    // Full product: each leetable position chooses "keep" or one substitute.
    // `choices[i]` is the number of options at position i (1 keep + substitutes).
    let choices: Vec<usize> = positions
        .iter()
        .map(|&p| 1 + leet_subs(chars[p]).map_or(0, <[char]>::len))
        .collect();

    let total: usize = choices.iter().product();
    for combo in 0..total {
        let mut variant = chars.clone();
        let mut rem = combo;
        for (idx, &pos) in positions.iter().enumerate() {
            let option = rem % choices[idx];
            rem /= choices[idx];
            if option > 0 {
                let subs = leet_subs(chars[pos]).unwrap();
                variant[pos] = subs[option - 1];
            }
        }
        push_unique(&mut out, &mut seen, variant.into_iter().collect());
    }
    out
}
