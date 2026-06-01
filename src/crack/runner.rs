//! Shared cracking-engine primitives: which engines exist, their binary names,
//! and small helpers for mapping/parsing. The actual attack orchestration (with
//! live progress and cancellation) lives in `job.rs`.
//!
//! Both tools are external system dependencies. mcpscrk never pretends to crack
//! anything itself; it only drives hashcat / john and reports what they say.

use serde::{Deserialize, Serialize};
use tokio::process::Command;

/// Binary names, looked up on PATH.
pub const HASHCAT_BIN: &str = "hashcat";
pub const JOHN_BIN: &str = "john";

/// Which engine to drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Hashcat,
    John,
}

impl Engine {
    pub fn as_str(self) -> &'static str {
        match self {
            Engine::Hashcat => "hashcat",
            Engine::John => "john",
        }
    }
}

/// Report which engines are available on this machine.
pub async fn available() -> (bool, bool) {
    (is_runnable(HASHCAT_BIN).await, is_runnable(JOHN_BIN).await)
}

/// Whether a binary can be executed (responds to `--version`/`--help`).
pub async fn is_runnable(bin: &str) -> bool {
    Command::new(bin)
        .arg("--version")
        .kill_on_drop(true)
        .output()
        .await
        .is_ok()
}

/// Build a unique temp path tagged with the process id.
pub fn temp_path(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("mcpscrk-{tag}-{}.txt", std::process::id()))
}

/// Map a hashcat mode to a john format name for the common cases.
pub fn john_format(mode: Option<u32>) -> Option<&'static str> {
    match mode? {
        0 => Some("raw-md5"),
        100 => Some("raw-sha1"),
        1400 => Some("raw-sha256"),
        1700 => Some("raw-sha512"),
        1000 => Some("nt"),
        3200 => Some("bcrypt"),
        1800 => Some("sha512crypt"),
        500 => Some("md5crypt"),
        _ => None,
    }
}

/// Extract the plaintext from a "something:plaintext" line, taking the first
/// non-empty match. Works for both hashcat output and `john --show`.
pub fn parse_cracked(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.contains("password hash") || line.ends_with("cracked") {
            continue; // skip john's summary lines like "1 password hash cracked"
        }
        if let Some((_, plain)) = line.split_once(':') {
            if !plain.is_empty() {
                return Some(plain.to_string());
            }
        }
    }
    None
}
