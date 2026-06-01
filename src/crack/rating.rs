//! Target profile rating for a recovered password.
//!
//! Scored from the *attacker's* point of view: how much did this target think
//! about what they were doing? A cracked password always means the build worked,
//! but the profile tells you whether the target was ridiculous or careful.

use serde::Serialize;

/// How the target behaves, seen through the attacker's lens.
/// `Paranoid` is the unreachable ideal - a cracked password never earns it.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    Paranoid,
    Careful,
    Aware,
    Normal,
    Careless,
    Ridiculous,
}

impl Profile {
    fn label(self) -> &'static str {
        match self {
            Profile::Paranoid => "Paranoid",
            Profile::Careful => "Careful",
            Profile::Aware => "Aware",
            Profile::Normal => "Normal",
            Profile::Careless => "Careless",
            Profile::Ridiculous => "Ridiculous",
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

/// Rate how sophisticated the target looks from 0 (absurd) to 10 (tried hard).
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

    // Length tier: short passwords scream "did not think twice".
    let mut score = match len {
        0..=6 => 0.4,
        7..=8 => 0.9,
        9..=11 => 1.8,
        12..=15 => 3.2,
        16..=19 => 4.8,
        20..=24 => 6.2,
        _ => 7.5 + (len.min(40) as f32 - 24.0) * 0.15,
    };

    score += (classes as f32) * 0.7;

    // Short + low diversity = footprint paste. Hammer it down.
    if len <= 10 && classes <= 2 {
        score *= 0.45;
    } else if len <= 12 && classes <= 2 {
        score *= 0.65;
    }

    // Long mixed passwords: target put real effort in, still fell.
    if len >= 16 && classes >= 4 {
        score += 1.8;
    }
    if len >= 20 && classes >= 3 {
        score += 1.0;
    }

    score = score.clamp(0.2, 9.8);

    let profile = if score < 1.3 {
        Profile::Ridiculous
    } else if score < 2.5 {
        Profile::Careless
    } else if score < 4.2 {
        Profile::Normal
    } else if score < 6.8 {
        Profile::Aware
    } else {
        Profile::Careful
    };

    let why = match profile {
        Profile::Ridiculous => format!(
            "{len} chars, {classes} class(es) - straight from the footprint, zero thought. \
             This target did not think twice."
        ),
        Profile::Careless => format!(
            "Short and predictable ({len} chars, {classes} classes). \
             The target barely tried - public data was enough."
        ),
        Profile::Normal => format!(
            "Average build ({len} chars, {classes} classes). \
             Typical footprint password - the target did not go out of their way."
        ),
        Profile::Aware => format!(
            "Some effort visible ({len} chars, {classes} classes), \
             but still reconstructible from OSINT. The target thought a little, not enough."
        ),
        Profile::Careful => format!(
            "Strong-looking password ({len} chars, {classes} classes) - \
             the target clearly tried - yet your wordlist still rebuilt it from public facts."
        ),
        Profile::Paranoid => unreachable!(),
    };

    Verdict {
        score: (score * 10.0).round() / 10.0,
        profile: profile.label(),
        why,
    }
}
