//! Raw OSINT data grouped into semantic categories.
//!
//! Grouping prevents nonsensical mixes (e.g. crossing a football team with a
//! car at random) and lets the workbench present the data the way a human
//! profile is actually structured.
//!
//!   SET 1: Identity   -> firstname, lastnames, nicknames, usernames, emails, ids, phones
//!   SET 2: Relations  -> partners, children, pets, parents, siblings
//!   SET 3: Passions   -> teams, athletes, sports, artists, movies, games, hobbies, cars
//!   SET 4: Context    -> cities, places, companies, jobtitles, projects, words, ...
//!   SET 5: Numeric    -> dates, numbers, postcodes
//!   SET 6: Special    -> the most common special characters

use serde::{Deserialize, Serialize};

/// Semantic category a parameter belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Identity,
    Relations,
    Passions,
    Context,
    Numeric,
    Special,
}

impl Category {
    /// Human-readable label used by the UI.
    pub fn label(self) -> &'static str {
        match self {
            Category::Identity => "Identity",
            Category::Relations => "Relations",
            Category::Passions => "Passions",
            Category::Context => "Context",
            Category::Numeric => "Numeric",
            Category::Special => "Special",
        }
    }
}

/// The full set of common symbols (separators included), in rough order of
/// popularity. This is the default of the "All Symbols" block.
pub const DEFAULT_SPECIALS: &[&str] = &[
    "!", "@", "#", "$", "%", "^", "&", "*", "-", "_", "+", "=", ".", ",", "?", "/",
];

/// Typical weak separators people drop between two words.
pub const DEFAULT_SEPARATORS: &[&str] = &[".", "-", "_"];

/// Common symbols that are NOT separators (default of the "Special Chars" block).
pub const DEFAULT_SPECIAL_ONLY: &[&str] =
    &["!", "@", "#", "$", "%", "^", "&", "*", "+", "=", ",", "?", "/"];

/// Names of the permanent, editable symbol blocks that always live in the
/// inventory. They are not craftable materials and cannot be deleted, but their
/// contents can be edited (and reset to the defaults above).
pub const SEPARATORS_BLOCK: &str = "Separators";
pub const SPECIAL_BLOCK: &str = "Special Chars";
pub const SYMBOLS_BLOCK: &str = "All Symbols";

/// Name of the permanent, auto-derived dates block. Its contents are produced
/// from the profile `dates` field by the date engine and refresh on update.
pub const DATES_BLOCK: &str = "Dates";

/// Default contents for a given editable symbol block, by name.
pub fn default_symbols(block: &str) -> Vec<String> {
    let defaults: &[&str] = match block {
        SEPARATORS_BLOCK => DEFAULT_SEPARATORS,
        SPECIAL_BLOCK => DEFAULT_SPECIAL_ONLY,
        _ => DEFAULT_SPECIALS, // SYMBOLS_BLOCK and fallback
    };
    defaults.iter().map(|s| s.to_string()).collect()
}

/// A known OSINT parameter: its key, display label and category.
pub struct Catalog {
    pub key: &'static str,
    pub label: &'static str,
    pub category: Category,
}

/// The full catalog of supported parameters, in display order.
///
/// The catalog is the single source of truth: the UI renders inputs from it and
/// the engine looks up a parameter's category by its key.
pub fn catalog() -> &'static [Catalog] {
    use Category::*;
    &[
        // SET 1 - Identity
        Catalog { key: "firstname", label: "First name", category: Identity },
        Catalog { key: "lastnames", label: "Last names", category: Identity },
        Catalog { key: "nicknames", label: "Nicknames", category: Identity },
        Catalog { key: "usernames", label: "Usernames", category: Identity },
        Catalog { key: "emails", label: "Emails", category: Identity },
        Catalog { key: "ids", label: "IDs", category: Identity },
        Catalog { key: "phones", label: "Phones", category: Identity },
        // SET 2 - Relations
        Catalog { key: "partners", label: "Partners", category: Relations },
        Catalog { key: "children", label: "Children", category: Relations },
        Catalog { key: "pets", label: "Pets", category: Relations },
        Catalog { key: "parents", label: "Parents", category: Relations },
        Catalog { key: "siblings", label: "Siblings", category: Relations },
        // SET 3 - Passions
        Catalog { key: "teams", label: "Teams", category: Passions },
        Catalog { key: "athletes", label: "Athletes", category: Passions },
        Catalog { key: "sports", label: "Sports", category: Passions },
        Catalog { key: "artists", label: "Artists", category: Passions },
        Catalog { key: "movies", label: "Movies", category: Passions },
        Catalog { key: "games", label: "Games", category: Passions },
        Catalog { key: "hobbies", label: "Hobbies", category: Passions },
        Catalog { key: "cars", label: "Cars", category: Passions },
        // SET 4 - Context
        Catalog { key: "cities", label: "Cities", category: Context },
        Catalog { key: "places", label: "Places", category: Context },
        Catalog { key: "companies", label: "Companies", category: Context },
        Catalog { key: "jobtitles", label: "Job titles", category: Context },
        Catalog { key: "projects", label: "Projects", category: Context },
        Catalog { key: "words", label: "Words", category: Context },
        Catalog { key: "nationalities", label: "Nationalities", category: Context },
        Catalog { key: "faithterms", label: "Faith terms", category: Context },
        Catalog { key: "zodiac", label: "Zodiac", category: Context },
        // SET 5 - Numeric
        Catalog { key: "dates", label: "Dates", category: Numeric },
        Catalog { key: "numbers", label: "Numbers", category: Numeric },
        Catalog { key: "postcodes", label: "Postcodes", category: Numeric },
        // SET 6 - Special
        Catalog { key: "special", label: "Special characters", category: Special },
    ]
}

/// Look up the category of a parameter key. Unknown keys default to `Context`.
pub fn category_of(key: &str) -> Category {
    catalog()
        .iter()
        .find(|c| c.key == key)
        .map(|c| c.category)
        .unwrap_or(Category::Context)
}

/// A single parameter with the values the user entered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Param {
    pub key: String,
    pub category: Category,
    pub values: Vec<String>,
}

/// The complete OSINT profile of a target (the UI's "Profile").
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileSets {
    pub params: Vec<Param>,
}

impl ProfileSets {
    /// Replace the values of `key`, parsing a comma-separated string and
    /// dropping blanks and duplicates. An empty input removes the parameter.
    pub fn set_field(&mut self, key: &str, raw_csv: &str) {
        let values = parse_csv(raw_csv);
        self.params.retain(|p| p.key != key);
        if !values.is_empty() {
            self.params.push(Param {
                key: key.to_string(),
                category: category_of(key),
                values,
            });
        }
    }

    /// Return the values stored for `key`, if any.
    pub fn get(&self, key: &str) -> Option<&[String]> {
        self.params
            .iter()
            .find(|p| p.key == key)
            .map(|p| p.values.as_slice())
    }
}

/// Capitalize the first character of a string ("username" -> "Username").
pub fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().chain(chars).collect(),
    }
}

/// Split a comma-separated string into trimmed, non-empty, de-duplicated values.
///
/// Order is preserved so the user sees their input echoed back predictably.
pub fn parse_csv(raw: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter(|s| seen.insert(s.to_string()))
        .map(str::to_string)
        .collect()
}
