//! Cracking lab.
//!
//! mcpscrk does NOT reinvent hash handling. Detection and the attack itself are
//! delegated to the established tools that already do it best: `hashcat` and
//! `john` (John the Ripper). This module only orchestrates them - builds the
//! command line, runs it, and turns the result into something the UI can show.
//!
//! The craft lives upstream, in `engine`: forging the right dictionary.
//! Cracking is just where you test the build.

pub mod detect;
pub mod job;
pub mod rating;
pub mod runner;
