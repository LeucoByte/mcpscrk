//! Date engine.
//!
//! Dates are prime password material. Given a flexible input
//! (`28/10/1997`, `1988`, `09/09/1998`, `2022`, ...) this module derives every
//! numeric form a human would realistically paste into a password: full and
//! short years, day, month, day+month and the common day+month+year mixes.

use std::collections::HashSet;

/// Expand a batch of raw dates into a single, de-duplicated set of forms.
pub fn expand_all(raw_dates: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for raw in raw_dates {
        for form in expand_date(raw) {
            if seen.insert(form.clone()) {
                out.push(form);
            }
        }
    }
    out
}

/// Expand a single raw date into all of its usable numeric forms.
pub fn expand_date(raw: &str) -> Vec<String> {
    let parts: Vec<&str> = raw
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .collect();

    let mut forms = Vec::new();
    match parts.as_slice() {
        // A lone token: a year (4 digits) or a bare number we pass through.
        [single] => {
            if single.len() == 4 {
                push_year(&mut forms, single);
            } else {
                forms.push((*single).to_string());
            }
        }
        // Two tokens: treat as day+month (also covers month+year loosely).
        [a, b] => {
            let (d, m) = (pad2(a), pad2(b));
            forms.push(d.clone());
            forms.push(m.clone());
            forms.push(format!("{d}{m}"));
            forms.push(format!("{m}{d}"));
        }
        // Three tokens: a full day/month/year date.
        [a, b, c] => {
            let (d, m) = (pad2(a), pad2(b));
            forms.push(d.clone());
            forms.push(m.clone());
            forms.push(format!("{d}{m}"));
            forms.push(format!("{m}{d}"));
            if let Some(year) = normalize_year(c) {
                let yy = &year[2..];
                forms.push(year.clone());
                forms.push(yy.to_string());
                forms.push(format!("{d}{m}{year}"));
                forms.push(format!("{d}{m}{yy}"));
            }
        }
        _ => {}
    }

    // De-duplicate while keeping order.
    let mut seen = HashSet::new();
    forms.into_iter().filter(|f| seen.insert(f.clone())).collect()
}

/// Push the full and short forms of a 4-digit year.
fn push_year(out: &mut Vec<String>, year: &str) {
    out.push(year.to_string());
    out.push(year[2..].to_string());
}

/// Pad a 1-digit token to two digits ("9" -> "09"); leave others as-is.
fn pad2(token: &str) -> String {
    if token.len() == 1 {
        format!("0{token}")
    } else {
        token.to_string()
    }
}

/// Normalize a year token to four digits ("97" -> "1997", "05" -> "2005").
fn normalize_year(token: &str) -> Option<String> {
    match token.len() {
        4 => Some(token.to_string()),
        2 => {
            let n: u32 = token.parse().ok()?;
            // Two-digit years <= 30 are read as 2000s, the rest as 1900s.
            Some(if n <= 30 {
                format!("20{token}")
            } else {
                format!("19{token}")
            })
        }
        _ => None,
    }
}
