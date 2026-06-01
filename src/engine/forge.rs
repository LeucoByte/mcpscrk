//! The forge: lazy generation, filtering, de-duplication and output.
//!
//! Given the potential volume of combinations, the generator never materializes
//! the whole list. It walks the nested loops with an index "odometer", builds
//! each candidate into a reused buffer, applies the length filter, drops
//! duplicates and streams the survivors out. Callers decide what to do with each
//! survivor (write it to a file, collect a preview, ...).

use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::block::Block;
use super::filters::LengthFilter;

/// How the output file is opened.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WriteMode {
    /// Replace the file's contents.
    Overwrite,
    /// Add to the end of the file, without re-emitting lines already present.
    Append,
}

/// Statistics describing one generation run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ForgeStats {
    /// Combinations produced before filtering or de-duplication.
    pub generated: u64,
    /// Candidates dropped by the length filter.
    pub filtered: u64,
    /// Candidates dropped because they were duplicates.
    pub duplicates: u64,
    /// Unique candidates emitted.
    pub emitted: u64,
    /// Character class of the emitted set (e.g. "alphanum").
    pub kind: String,
}

/// Report returned after writing a wordlist to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeReport {
    pub stats: ForgeStats,
    pub path: String,
}

/// Tracks which character classes appear across the emitted set, so we can
/// label the wordlist the way `hashcat`/`john` users expect ("alphanum", ...).
#[derive(Default)]
struct CharClass {
    alpha: bool,
    digit: bool,
    special: bool,
}

impl CharClass {
    /// Fold a candidate's characters into the running classification.
    fn observe(&mut self, s: &str) {
        for c in s.chars() {
            if c.is_ascii_alphabetic() {
                self.alpha = true;
            } else if c.is_ascii_digit() {
                self.digit = true;
            } else {
                self.special = true;
            }
        }
    }

    /// Render the classification as a short label.
    fn label(&self) -> String {
        let core = match (self.alpha, self.digit) {
            (true, true) => "alphanum",
            (true, false) => "alpha",
            (false, true) => "numeric",
            (false, false) => "empty",
        };
        if self.special {
            format!("{core}+special")
        } else {
            core.to_string()
        }
    }
}

/// Walk the nested loops defined by `blocks`, in order, calling `sink` with each
/// unique, length-valid candidate. The `sink` returns `false` to stop early
/// (used by previews). `seen` carries de-duplication state and may be primed
/// with pre-existing lines (used by append mode).
///
/// Returns the run statistics.
fn generate<F>(
    blocks: &[&Block],
    filter: &LengthFilter,
    seen: &mut HashSet<String>,
    mut sink: F,
) -> ForgeStats
where
    F: FnMut(&str) -> bool,
{
    let mut stats = ForgeStats::default();
    let mut class = CharClass::default();

    // An empty blueprint, or any empty block, yields nothing.
    if blocks.is_empty() || blocks.iter().any(|b| b.is_empty()) {
        stats.kind = class.label();
        return stats;
    }

    let depth = blocks.len();
    let mut idx = vec![0usize; depth];
    let mut buf = String::new();

    loop {
        // Build the current candidate by concatenating the selected values.
        buf.clear();
        for (level, block) in blocks.iter().enumerate() {
            buf.push_str(&block.values[idx[level]]);
        }
        stats.generated += 1;

        if filter.accepts(&buf) {
            if seen.insert(buf.clone()) {
                class.observe(&buf);
                stats.emitted += 1;
                if !sink(&buf) {
                    stats.kind = class.label();
                    return stats;
                }
            } else {
                stats.duplicates += 1;
            }
        } else {
            stats.filtered += 1;
        }

        // Advance the odometer: increment the innermost loop, carry outward.
        let mut level = depth - 1;
        loop {
            idx[level] += 1;
            if idx[level] < blocks[level].values.len() {
                break;
            }
            idx[level] = 0;
            if level == 0 {
                stats.kind = class.label();
                return stats;
            }
            level -= 1;
        }
    }
}

/// Generate up to `limit` candidates without touching disk: used for the live
/// preview in the workbench. Returns the sample lines and the run statistics
/// (statistics reflect only the work done up to the limit).
pub fn preview(blocks: &[&Block], filter: &LengthFilter, limit: usize) -> (Vec<String>, ForgeStats) {
    let mut seen = HashSet::new();
    let mut lines = Vec::with_capacity(limit);
    let stats = generate(blocks, filter, &mut seen, |candidate| {
        lines.push(candidate.to_string());
        lines.len() < limit
    });
    (lines, stats)
}

/// Generate the full wordlist and stream it to `path`.
///
/// In append mode the existing lines are loaded first so the file never ends up
/// with duplicates. Returns a report with the run statistics.
pub fn forge(
    blocks: &[&Block],
    filter: &LengthFilter,
    mode: WriteMode,
    path: &Path,
) -> anyhow::Result<ForgeReport> {
    let mut seen = HashSet::new();

    // Append mode must not re-emit lines that are already on disk.
    if matches!(mode, WriteMode::Append) {
        if let Ok(existing) = std::fs::read_to_string(path) {
            for line in existing.lines() {
                seen.insert(line.to_string());
            }
        }
    }

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(matches!(mode, WriteMode::Overwrite))
        .append(matches!(mode, WriteMode::Append))
        .open(path)?;
    let mut writer = BufWriter::new(file);

    let mut write_error: Option<std::io::Error> = None;
    let stats = generate(blocks, filter, &mut seen, |candidate| {
        if let Err(e) = writeln!(writer, "{candidate}") {
            write_error = Some(e);
            return false;
        }
        true
    });

    if let Some(e) = write_error {
        return Err(e.into());
    }
    writer.flush()?;

    Ok(ForgeReport {
        stats,
        path: path.display().to_string(),
    })
}
