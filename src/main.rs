//! mcpscrk - Marco Calvo Password Cracker.
//!
//! A single Rust binary that holds both the logic (the wordlist forge) and the
//! view (a web workbench). It boots with one flag, `-p/--port`, and serves the
//! workbench in the browser.
//!
//! Philosophy: there is no perfect wordlist produced by a blind algorithm.
//! The auditor crafts their own, block by block, the way *they* would crack a
//! given target. This tool is the bench; the craft is yours.

mod cli;
mod crack;
mod engine;
mod server;

use anyhow::Result;
use clap::Parser;

/// Whether the process is running as root.
fn is_root() -> bool {
    // Safe: geteuid has no preconditions and never fails.
    unsafe { libc::geteuid() == 0 }
}

/// Program entry point: parse the CLI and start the web workbench.
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_target(false).init();

    // Building wordlists needs no privileges, so we do not exit when unprivileged.
    // The cracking lab drives hashcat/john, which may require an installed,
    // properly set up toolchain (and sometimes elevated rights), so we warn.
    if !is_root() {
        tracing::warn!(
            "running without root: wordlist building works, but the cracking lab needs \
             hashcat/john installed (and possibly sudo). Re-run with sudo for the full lab."
        );
    }

    // Probe the external cracking engines up front so the operator knows, before
    // touching the browser, whether they can only craft wordlists or also crack.
    let (hashcat, john) = crack::runner::available().await;
    match (hashcat, john) {
        (true, true) => tracing::info!("cracking engines ready: hashcat + john (default: hashcat)."),
        (true, false) => tracing::warn!(
            "john not found on PATH: the cracking lab will use hashcat only."
        ),
        (false, true) => tracing::warn!(
            "hashcat not found on PATH: the cracking lab will use john only."
        ),
        (false, false) => tracing::warn!(
            "neither hashcat nor john found on PATH: you can craft wordlists but NOT crack. \
             Install them (e.g. `sudo apt install hashcat john`) to enable the cracking lab."
        ),
    }

    let args = cli::Cli::parse();
    server::run(args.port).await
}
