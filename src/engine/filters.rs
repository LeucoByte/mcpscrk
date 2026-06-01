//! Output filters.
//!
//! A min/max length constraint, used to discard candidates on the fly that
//! could not satisfy the target's password policy (many sites ask for 8-16).

/// Inclusive length range a candidate must fall within.
#[derive(Debug, Clone, Copy)]
pub struct LengthFilter {
    pub min: usize,
    pub max: usize,
}

impl Default for LengthFilter {
    fn default() -> Self {
        Self { min: 1, max: 64 }
    }
}

impl LengthFilter {
    /// Whether a candidate passes the length filter (counted in characters).
    /// A min of 0 means no lower bound; a max of 0 means no upper bound.
    pub fn accepts(&self, candidate: &str) -> bool {
        let len = candidate.chars().count();
        if self.min > 0 && len < self.min {
            return false;
        }
        if self.max > 0 && len > self.max {
            return false;
        }
        true
    }
}
