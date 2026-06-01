//! Command line interface.
//!
//! By design every meaningful setting (OSINT data, blocks, blueprint, filters,
//! output) is configured from the web workbench. The CLI therefore exposes a
//! single flag: the port to serve the workbench on.

use clap::Parser;

/// Parsed command line arguments.
#[derive(Parser, Debug)]
#[command(
    name = "mcpscrk",
    about = "Marco Calvo Password Cracker - an artisan workbench for OSINT wordlists",
    version
)]
pub struct Cli {
    /// Port the web workbench is served on.
    #[arg(short, long, default_value_t = 8787)]
    pub port: u16,
}
