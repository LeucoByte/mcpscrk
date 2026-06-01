//! The wordlist forge: all the dictionary-building logic.
//!
//! Conceptual pipeline of the workbench:
//!
//!   1. `sets`     - Raw OSINT data, grouped into semantic categories.
//!   2. `expand`   - Per-word expansion: capitalization variants and strategic
//!                   leet speak, kept under control to avoid combinatorial blowup.
//!   3. `dates`    - Date engine: full/short years, days, months and the usual
//!                   numeric forms a human would actually type.
//!   4. `block`    - An "assembly piece": a named, already-mutated set of strings
//!                   ready to be slotted into the blueprint.
//!   5. `blueprint`- Linear assembly: the order of blocks defines nested loops
//!                   (Loop A -> B -> C).
//!   6. `forge`    - Lazy generation, length filtering, de-duplication, streaming
//!                   to a file, and an honest count.
//!   7. `filters`  - Min/max length constraints.

pub mod block;
pub mod blueprint;
pub mod dates;
pub mod expand;
pub mod filters;
pub mod forge;
pub mod sets;
