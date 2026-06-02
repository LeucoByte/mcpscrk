//! Assembly block ("piece").
//!
//! A block is a puzzle piece: a named set of strings that has already been
//! mutated (capitalization, leet, and/or reversal) and is ready to be slotted into the
//! blueprint. It is built once, in the modifier lab, and reused.
//!
//! Invariant: a block's contents are always a *set* - no duplicates - which is
//! exactly what keeps the later combinatorics clean.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::dates;
use super::expand::{self, CapMode};

/// The mutation rules applied when a block is created.
#[derive(Debug, Clone, Copy)]
pub struct BlockRules {
    pub cap: CapMode,
    pub leet: bool,
    pub reverse: bool,
}

/// A ready-to-assemble piece.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// User-facing name, e.g. "firstname_leet".
    pub name: String,
    /// The parameter key this block was built from.
    pub source: String,
    /// The final, de-duplicated set of strings.
    pub values: Vec<String>,
}

impl Block {
    /// Build a block from raw values and a set of rules.
    ///
    /// The `source` key drives one special case: a `dates` source is first run
    /// through the date engine. After that, capitalization, leet, and reversal
    /// are applied uniformly and the result is collapsed into a duplicate-free set.
    pub fn build(name: &str, source: &str, raw: &[String], rules: BlockRules) -> Block {
        let base: Vec<String> = if source == "dates" {
            dates::expand_all(raw)
        } else {
            raw.to_vec()
        };

        let mut values = Vec::new();
        let mut seen = HashSet::new();

        for token in &base {
            for cased in expand::capitalize(token, rules.cap) {
                let mut batch = if rules.leet {
                    expand::leet_variants(&cased)
                } else {
                    vec![cased]
                };
                if rules.reverse {
                    batch = expand::with_reverse(batch);
                }
                for variant in batch {
                    if seen.insert(variant.clone()) {
                        values.push(variant);
                    }
                }
            }
        }

        Block {
            name: name.to_string(),
            source: source.to_string(),
            values,
        }
    }

    /// Number of distinct strings in the block.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the block is empty (produces nothing).
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}
