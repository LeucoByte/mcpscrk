//! Blueprint helpers.
//!
//! A blueprint is simply an ordered list of blocks. That order is the whole
//! point: it defines nested loops.
//!
//!     (A)(B)(C)  =>  for a in A { for b in B { for c in C { a + b + c } } }
//!
//! The first element of A is held while B is fully traversed, and for every B
//! the whole of C is traversed, before moving to the second element of A. The
//! auditor decides the order; the engine just iterates it (see `forge`).

use super::block::Block;

/// Total number of raw combinations a blueprint yields, before any length
/// filtering or de-duplication. This is the product of the block sizes and is
/// what the workbench shows as the live estimate.
///
/// Returns 0 when there are no blocks or any block is empty.
pub fn estimated_size(blocks: &[&Block]) -> u128 {
    if blocks.is_empty() {
        return 0;
    }
    blocks.iter().map(|b| b.len() as u128).product()
}
