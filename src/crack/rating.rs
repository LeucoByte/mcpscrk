//! Awareness rating for a recovered password.
//!
//! Once a password falls, we score how predictable it was and attach a profile
//! label. This is the whole educational point: show how fragile a hand-made
//! password is once someone has your public data.

use serde::Serialize;

/// Behavioural profile, from most to least careful. `Paranoid` is the ideal end
/// of the scale and is never assigned to a cracked password by design.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    Paranoid,
    Careful,
    Aware,
    Normal,
    Careless,
}

impl Profile {
    fn label(self) -> &'static str {
        match self {
            Profile::Paranoid => "Paranoid",
            Profile::Careful => "Careful",
            Profile::Aware => "Aware",
            Profile::Normal => "Normal",
            Profile::Careless => "Careless",
        }
    }
}

/// The verdict shown after a successful crack.
#[derive(Debug, Clone, Serialize)]
pub struct Verdict {
    pub score: f32,
    pub profile: &'static str,
    pub why: String,
}

/// Rate a recovered plaintext from 0 (trivial) to 10 (resisted well).
///
/// The score is deliberately simple and pessimistic: anything our wordlist
/// found is, by definition, derivable from public data, so it cannot score high.
pub fn rate(plaintext: &str) -> Verdict {
    let len = plaintext.chars().count();
    let has_lower = plaintext.chars().any(|c| c.is_ascii_lowercase());
    let has_upper = plaintext.chars().any(|c| c.is_ascii_uppercase());
    let has_digit = plaintext.chars().any(|c| c.is_ascii_digit());
    let has_special = plaintext.chars().any(|c| !c.is_ascii_alphanumeric());
    let classes = [has_lower, has_upper, has_digit, has_special]
        .iter()
        .filter(|b| **b)
        .count();

    // Start from length, add a little for character variety, then cap hard:
    // it was cracked from OSINT data, so it was never really strong.
    let mut score = (len as f32) * 0.45 + (classes as f32) * 0.6;
    score = score.clamp(0.5, 6.5);

    let profile = if score < 2.0 {
        Profile::Careless
    } else if score < 3.5 {
        Profile::Normal
    } else if score < 4.5 {
        Profile::Aware
    } else {
        Profile::Careful
    };

    let why = format!(
        "Recovered from a wordlist built out of public data: {len} chars, {classes} character class(es). \
         Anyone who can profile your footprint could reproduce it. Use a password manager."
    );

    Verdict {
        score: (score * 10.0).round() / 10.0,
        profile: profile.label(),
        why,
    }
}
