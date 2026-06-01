//! Shared, in-memory workbench state.
//!
//! Holds the current OSINT profile and the inventory of crafted blocks, plus
//! the permanent blocks that come by default: the auto-derived dates, the fixed
//! digit block (0-9), and three editable symbol blocks (separators, special
//! chars, and the union of both).
//! The blueprint order, length filter and output settings are passed per request.

use std::sync::{Arc, Mutex};

use crate::crack::job::CrackJob;
use crate::engine::{
    block::Block,
    dates,
    sets::{
        self, ProfileSets, DATE_BLOCK, DIGIT_BLOCK, SEPARATOR_BLOCK, SPECIAL_CHAR_BLOCK,
        SYMBOLS_BLOCK,
    },
};

/// The auditor's bench: profile data, crafted blocks and the fixed blocks.
pub struct Workshop {
    pub profile: ProfileSets,
    pub inventory: Vec<Block>,
    /// Auto-derived dates block, refreshed from the profile `dates` field.
    pub dates: Block,
    /// Fixed 0-9 digit block (not editable).
    pub digit: Block,
    /// Editable symbol blocks (separators / non-separator specials / both).
    pub separators: Block,
    pub specials: Block,
    pub symbols: Block,
}

/// Build a fixed block from its default contents.
fn fixed_block(name: &str) -> Block {
    Block {
        name: name.to_string(),
        source: "symbols".to_string(),
        values: sets::default_symbols(name),
    }
}

impl Default for Workshop {
    fn default() -> Self {
        Workshop {
            profile: ProfileSets::default(),
            inventory: Vec::new(),
            dates: Block {
                name: DATE_BLOCK.to_string(),
                source: "dates".to_string(),
                values: Vec::new(),
            },
            digit: Block {
                name: DIGIT_BLOCK.to_string(),
                source: "digits".to_string(),
                values: sets::default_digits(),
            },
            separators: fixed_block(SEPARATOR_BLOCK),
            specials: fixed_block(SPECIAL_CHAR_BLOCK),
            symbols: fixed_block(SYMBOLS_BLOCK),
        }
    }
}

impl Workshop {
    /// Rebuild the auto-derived dates block from the current profile.
    pub fn rebuild_dates(&mut self) {
        let raw = self.profile.get("dates").unwrap_or(&[]);
        self.dates.values = dates::expand_all(raw);
    }

    /// Edit one of the symbol blocks from a comma-separated string. An empty
    /// input restores that block's defaults. Unknown names are ignored.
    pub fn edit_symbols(&mut self, name: &str, csv: &str) {
        let target = match name {
            SEPARATOR_BLOCK => &mut self.separators,
            SPECIAL_CHAR_BLOCK => &mut self.specials,
            SYMBOLS_BLOCK => &mut self.symbols,
            _ => return,
        };
        let values = sets::parse_csv(csv);
        target.values = if values.is_empty() {
            sets::default_symbols(name)
        } else {
            values
        };
    }

    /// Find a block by name, including the permanent blocks.
    pub fn block(&self, name: &str) -> Option<&Block> {
        match name {
            DATE_BLOCK => Some(&self.dates),
            DIGIT_BLOCK => Some(&self.digit),
            SEPARATOR_BLOCK => Some(&self.separators),
            SPECIAL_CHAR_BLOCK => Some(&self.specials),
            SYMBOLS_BLOCK => Some(&self.symbols),
            _ => self.inventory.iter().find(|b| b.name == name),
        }
    }

    /// Resolve an ordered list of block names into block references, skipping
    /// any name that does not exist.
    pub fn resolve<'a>(&'a self, order: &[String]) -> Vec<&'a Block> {
        order.iter().filter_map(|n| self.block(n)).collect()
    }
}

/// Cheaply clonable application state shared across handlers.
#[derive(Clone, Default)]
pub struct AppState {
    pub workshop: Arc<Mutex<Workshop>>,
    /// The current (or last) crack job, polled by the UI for live progress.
    pub crack: Arc<Mutex<CrackJob>>,
}
