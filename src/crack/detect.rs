//! Hash-type detection.
//!
//! Preferred path: ask `hashcat --identify`, which already knows every format.
//! Fallback (when hashcat is not installed): a compact, well-known table based
//! on length, alphabet and prefix. We never try to be cleverer than hashcat.

use serde::Serialize;
use tokio::process::Command;

use super::runner::{HASHCAT_BIN, JOHN_BIN};

/// A candidate hash type. `mode` is the hashcat `-m` value when known.
#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub mode: Option<u32>,
    pub name: String,
}

/// Full detection result: hashcat's structural candidates (authoritative, used
/// for the dropdown) plus John's independent guess as a cross-check.
#[derive(Debug, Clone, Serialize)]
pub struct Detection {
    /// Candidate hash types, most likely first. Drives the UI selection.
    pub candidates: Vec<Candidate>,
    /// Where `candidates` came from: "hashcat" or "builtin".
    pub source: String,
    /// John the Ripper's recognised format names (primary first), if installed.
    /// John's auto-detection of bare raw hex is weak (it tends to say "LM"), so
    /// this is shown as a second opinion, not used to override hashcat.
    pub john: Vec<String>,
}

/// Detect candidate hash types from every available angle.
pub async fn detect(hash: &str) -> Detection {
    let (mut candidates, source) = match detect_with_hashcat(hash).await {
        Some(found) if !found.is_empty() => (found, "hashcat".to_string()),
        _ => (detect_fallback(hash), "builtin".to_string()),
    };
    // hashcat --identify lists every structurally-compatible mode in an order
    // that is not by popularity (it can put MD4 before MD5). Float the modes
    // people actually meet in the wild to the front so the first one is a sane
    // default selection.
    reorder_by_popularity(&mut candidates);
    let john = detect_with_john(hash).await;
    Detection { candidates, source, john }
}

/// Most common modes, in the order we want them to win ties as the default.
const COMMON_MODES: &[u32] = &[
    0,    // MD5
    100,  // SHA1
    1400, // SHA256
    1700, // SHA512
    3200, // bcrypt
    1800, // sha512crypt
    500,  // md5crypt
    7400, // sha256crypt
    1000, // NTLM
    900,  // MD4
];

/// Stable-sort candidates so popular modes lead, then the rest by mode number.
fn reorder_by_popularity(candidates: &mut [Candidate]) {
    candidates.sort_by_key(|c| match c.mode {
        Some(m) => match COMMON_MODES.iter().position(|x| *x == m) {
            Some(p) => (0u8, p as u32),
            None => (1u8, m),
        },
        None => (2u8, 0),
    });
}

/// Run `hashcat --identify` and parse its candidate table. Returns `None` if
/// hashcat is missing or produced nothing usable.
async fn detect_with_hashcat(hash: &str) -> Option<Vec<Candidate>> {
    let dir = std::env::temp_dir();
    let hashfile = dir.join(format!("mcpscrk-id-{}.txt", std::process::id()));
    tokio::fs::write(&hashfile, hash.trim()).await.ok()?;

    let output = Command::new(HASHCAT_BIN)
        .arg("--identify")
        .arg(&hashfile)
        .arg("--quiet")
        .kill_on_drop(true)
        .output()
        .await
        .ok()?;

    let _ = tokio::fs::remove_file(&hashfile).await;
    let text = String::from_utf8_lossy(&output.stdout);

    // Lines look like:  "      0 | MD5 | Raw Hash". Keep mode + name.
    let candidates: Vec<Candidate> = text
        .lines()
        .filter_map(|line| {
            let mut cols = line.split('|');
            let mode: u32 = cols.next()?.trim().parse().ok()?;
            let name = cols.next()?.trim().to_string();
            if name.is_empty() {
                None
            } else {
                Some(Candidate { mode: Some(mode), name })
            }
        })
        .collect();

    if candidates.is_empty() {
        None
    } else {
        Some(candidates)
    }
}

/// Ask John for its opinion: load the hash with an empty wordlist so it only
/// performs format detection, then scrape the format names it reports. Returns
/// an empty list if John is not installed or said nothing useful.
async fn detect_with_john(hash: &str) -> Vec<String> {
    let dir = std::env::temp_dir();
    let pid = std::process::id();
    let hashfile = dir.join(format!("mcpscrk-jid-{pid}.txt"));
    let potfile = dir.join(format!("mcpscrk-jid-{pid}.pot"));
    let session = dir.join(format!("mcpscrk-jid-{pid}-sess"));
    if tokio::fs::write(&hashfile, hash.trim()).await.is_err() {
        return Vec::new();
    }
    let _ = tokio::fs::remove_file(&potfile).await;

    let output = Command::new(JOHN_BIN)
        .arg("--wordlist=/dev/null")
        .arg(format!("--pot={}", potfile.display()))
        .arg(format!("--session={}", session.display()))
        .arg(&hashfile)
        .kill_on_drop(true)
        .output()
        .await;

    let _ = tokio::fs::remove_file(&hashfile).await;
    let _ = tokio::fs::remove_file(&potfile).await;
    let _ = tokio::fs::remove_file(format!("{}.log", session.display())).await;

    let output = match output {
        Ok(o) => o,
        Err(_) => return Vec::new(), // john not installed / not runnable
    };

    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&output.stderr));

    let mut names: Vec<String> = Vec::new();
    let push = |name: String, names: &mut Vec<String>| {
        let n = name.trim().to_string();
        if !n.is_empty() && !names.iter().any(|e| e == &n) {
            names.push(n);
        }
    };

    for line in text.lines() {
        // "Loaded 1 password hash (bcrypt [Blowfish 32/64 X3])"
        if let Some(rest) = line.split_once("password hash") {
            if let Some(open) = rest.1.find('(') {
                let after = &rest.1[open + 1..];
                let name = after.split(['[', ')']).next().unwrap_or("");
                push(name.to_string(), &mut names);
            }
        }
        // 'detected hash type "X"' and 'also recognized as "Y"'
        for marker in ["detected hash type \"", "also recognized as \""] {
            if let Some(i) = line.find(marker) {
                let after = &line[i + marker.len()..];
                if let Some(end) = after.find('"') {
                    push(after[..end].to_string(), &mut names);
                }
            }
        }
    }
    names
}

/// Minimal built-in detection for when hashcat is not available. Covers the
/// common cases by prefix, then by length and alphabet.
fn detect_fallback(hash: &str) -> Vec<Candidate> {
    let h = hash.trim();

    // Prefixed crypt formats are unambiguous.
    if h.starts_with("$2a$") || h.starts_with("$2b$") || h.starts_with("$2y$") {
        return vec![Candidate { mode: Some(3200), name: "bcrypt".into() }];
    }
    if h.starts_with("$6$") {
        return vec![Candidate { mode: Some(1800), name: "sha512crypt $6$".into() }];
    }
    if h.starts_with("$5$") {
        return vec![Candidate { mode: Some(7400), name: "sha256crypt $5$".into() }];
    }
    if h.starts_with("$1$") {
        return vec![Candidate { mode: Some(500), name: "md5crypt $1$".into() }];
    }

    let is_hex = !h.is_empty() && h.chars().all(|c| c.is_ascii_hexdigit());
    if is_hex {
        match h.len() {
            32 => return vec![
                Candidate { mode: Some(0), name: "MD5".into() },
                Candidate { mode: Some(1000), name: "NTLM".into() },
            ],
            40 => return vec![Candidate { mode: Some(100), name: "SHA1".into() }],
            64 => return vec![Candidate { mode: Some(1400), name: "SHA256".into() }],
            96 => return vec![Candidate { mode: Some(10800), name: "SHA384".into() }],
            128 => return vec![Candidate { mode: Some(1700), name: "SHA512".into() }],
            _ => {}
        }
    }

    vec![Candidate { mode: None, name: "Unknown".into() }]
}
