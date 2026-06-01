//! Crack job orchestration.
//!
//! A single job at a time. We spawn hashcat/john over a wordlist, stream their
//! output to read **live progress** (done/total), allow **cancellation**, and
//! **fall back** to the other engine if the chosen one fails to run (e.g. an
//! unsupported flag on an older version). The shared `CrackJob` is polled by the
//! UI via `GET /api/crack/status`.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;

use super::rating::{self, Verdict};
use super::runner::{self, Engine, HASHCAT_BIN, JOHN_BIN};

/// Keep the streamed log bounded.
const LOG_CAP: usize = 12_000;

/// Live, snapshot-able state of the current (or last) crack job.
#[derive(Clone, Serialize)]
pub struct CrackJob {
    pub running: bool,
    pub finished: bool,
    pub cracked: bool,
    pub plaintext: Option<String>,
    /// Engine currently running / last used.
    pub engine: String,
    pub done: u64,
    pub total: u64,
    pub percent: f32,
    pub elapsed_secs: u64,
    pub log: String,
    pub error: Option<String>,
    pub verdict: Option<Verdict>,
    /// Informational note, e.g. an automatic engine fallback.
    pub note: Option<String>,
    #[serde(skip)]
    pub pid: Option<u32>,
    #[serde(skip)]
    pub cancelled: bool,
    #[serde(skip)]
    pub started: Option<Instant>,
}

impl Default for CrackJob {
    fn default() -> Self {
        CrackJob {
            running: false,
            finished: false,
            cracked: false,
            plaintext: None,
            engine: String::new(),
            done: 0,
            total: 0,
            percent: 0.0,
            elapsed_secs: 0,
            log: String::new(),
            error: None,
            verdict: None,
            note: None,
            pid: None,
            cancelled: false,
            started: None,
        }
    }
}

/// Reset the job to a fresh "running" state for a new attack.
pub fn begin(job: &Arc<Mutex<CrackJob>>, total: u64) {
    let mut j = job.lock().unwrap();
    *j = CrackJob {
        running: true,
        total,
        started: Some(Instant::now()),
        ..Default::default()
    };
}

/// Request cancellation: flag it and kill the running child, if any.
pub fn cancel(job: &Arc<Mutex<CrackJob>>) {
    let mut j = job.lock().unwrap();
    j.cancelled = true;
    if let Some(pid) = j.pid.take() {
        // SIGKILL the engine process. Safe: kill never touches our memory.
        unsafe { libc::kill(pid as i32, libc::SIGKILL) };
    }
}

/// Outcome of one engine attempt.
enum Attempt {
    Cracked(String),
    NotFound,
    Cancelled,
    /// The engine could not run properly (missing flag, bad version, ...). The
    /// caller may fall back to the other engine.
    Error(String),
}

/// Orchestrate the whole job: try the preferred engine, fall back to the other
/// (if installed) only when the first one *fails to run*.
pub async fn run(
    job: Arc<Mutex<CrackJob>>,
    available: (bool, bool),
    preferred: Engine,
    hash: String,
    mode: Option<u32>,
    john_format: Option<String>,
    wordlist: PathBuf,
) {
    let other = match preferred {
        Engine::Hashcat => Engine::John,
        Engine::John => Engine::Hashcat,
    };
    let other_ok = match other {
        Engine::Hashcat => available.0,
        Engine::John => available.1,
    };

    let mut order = vec![preferred];
    if other_ok {
        order.push(other);
    }

    let mut last_error = String::from("Could not run any engine.");
    for (i, engine) in order.iter().copied().enumerate() {
        if job.lock().unwrap().cancelled {
            finalize(&job, |j| j.error = Some("Cancelled.".into()));
            return;
        }
        {
            let mut j = job.lock().unwrap();
            j.engine = engine.as_str().to_string();
            j.done = 0;
            j.percent = 0.0;
            if i > 0 {
                j.note = Some(format!(
                    "{} failed to run; fell back to {}.",
                    order[i - 1].as_str(),
                    engine.as_str()
                ));
            }
        }

        match attempt(&job, engine, &hash, mode, john_format.as_deref(), &wordlist).await {
            Attempt::Cracked(plain) => {
                let verdict = rating::rate(&plain);
                finalize(&job, |j| {
                    j.cracked = true;
                    j.plaintext = Some(plain);
                    j.verdict = Some(verdict);
                });
                return;
            }
            Attempt::NotFound => {
                finalize(&job, |j| j.cracked = false);
                return;
            }
            Attempt::Cancelled => {
                finalize(&job, |j| j.error = Some("Cancelled.".into()));
                return;
            }
            Attempt::Error(e) => last_error = e,
        }
    }

    finalize(&job, |j| j.error = Some(last_error));
}

/// Apply final mutations and flip the job into the finished/idle state.
fn finalize(job: &Arc<Mutex<CrackJob>>, f: impl FnOnce(&mut CrackJob)) {
    let mut j = job.lock().unwrap();
    f(&mut j);
    j.running = false;
    j.finished = true;
    j.pid = None;
    if let Some(s) = j.started {
        j.elapsed_secs = s.elapsed().as_secs();
    }
    if j.percent < 100.0 && j.cracked {
        j.percent = 100.0;
    }
}

async fn attempt(
    job: &Arc<Mutex<CrackJob>>,
    engine: Engine,
    hash: &str,
    mode: Option<u32>,
    john_format: Option<&str>,
    wordlist: &Path,
) -> Attempt {
    match engine {
        Engine::Hashcat => attempt_hashcat(job, hash, mode, wordlist).await,
        Engine::John => attempt_john(job, hash, mode, john_format, wordlist).await,
    }
}

/// Drive hashcat in straight (wordlist) mode with machine-readable status so we
/// can read PROGRESS live.
async fn attempt_hashcat(
    job: &Arc<Mutex<CrackJob>>,
    hash: &str,
    mode: Option<u32>,
    wordlist: &Path,
) -> Attempt {
    let mode = match mode {
        Some(m) => m,
        None => return Attempt::Error("Select a hash mode first (run detection).".into()),
    };

    let hashfile = runner::temp_path("hc-hash");
    let outfile = runner::temp_path("hc-out");
    if tokio::fs::write(&hashfile, hash.trim()).await.is_err() {
        return Attempt::Error("Could not write the temporary hash file.".into());
    }
    let _ = tokio::fs::remove_file(&outfile).await;

    let mut child = match Command::new(HASHCAT_BIN)
        .arg("-m").arg(mode.to_string())
        .arg("-a").arg("0")
        .arg("--potfile-disable")
        .arg("--force")
        .arg("--status")
        .arg("--status-timer=1")
        .arg("--machine-readable")
        .arg("-o").arg(&outfile)
        .arg(&hashfile)
        .arg(wordlist)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return Attempt::Error(format!("Could not start hashcat ({e}).")),
    };

    set_pid(job, child.id());
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let p1 = stdout.map(|s| tokio::spawn(pump(s, job.clone(), Engine::Hashcat)));
    let p2 = stderr.map(|s| tokio::spawn(pump(s, job.clone(), Engine::Hashcat)));

    let status = child.wait().await;
    if let Some(t) = p1 { let _ = t.await; }
    if let Some(t) = p2 { let _ = t.await; }
    clear_pid(job);

    if job.lock().unwrap().cancelled {
        let _ = tokio::fs::remove_file(&hashfile).await;
        let _ = tokio::fs::remove_file(&outfile).await;
        return Attempt::Cancelled;
    }

    let cracked = tokio::fs::read_to_string(&outfile)
        .await
        .ok()
        .and_then(|s| runner::parse_cracked(&s));
    let _ = tokio::fs::remove_file(&hashfile).await;
    let _ = tokio::fs::remove_file(&outfile).await;

    if let Some(plain) = cracked {
        return Attempt::Cracked(plain);
    }
    // hashcat: exit 0 = cracked, 1 = exhausted (legitimately not found),
    // anything else = a real error -> let the caller fall back.
    match status.ok().and_then(|s| s.code()) {
        Some(0) | Some(1) => Attempt::NotFound,
        _ => Attempt::Error(format!(
            "hashcat exited abnormally. {}",
            tail(&job.lock().unwrap().log, 240)
        )),
    }
}

/// Drive john over the wordlist, printing periodic progress to stderr.
async fn attempt_john(
    job: &Arc<Mutex<CrackJob>>,
    hash: &str,
    mode: Option<u32>,
    john_format: Option<&str>,
    wordlist: &Path,
) -> Attempt {
    let hashfile = runner::temp_path("jtr-hash");
    let potfile = runner::temp_path("jtr-pot");
    let session = runner::temp_path("jtr-sess");
    if tokio::fs::write(&hashfile, hash.trim()).await.is_err() {
        return Attempt::Error("Could not write the temporary hash file.".into());
    }
    let _ = tokio::fs::remove_file(&potfile).await;

    let mut cmd = Command::new(JOHN_BIN);
    cmd.arg(format!("--wordlist={}", wordlist.display()))
        .arg(format!("--pot={}", potfile.display()))
        .arg(format!("--session={}", session.display()))
        .arg("--progress-every=1");
    let fmt = john_format
        .map(|s| s.to_string())
        .or_else(|| runner::john_format(mode).map(|s| s.to_string()));
    if let Some(ref f) = fmt {
        cmd.arg(format!("--format={f}"));
    }
    cmd.arg(&hashfile)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Attempt::Error(format!("Could not start john ({e}).")),
    };

    set_pid(job, child.id());
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let p1 = stdout.map(|s| tokio::spawn(pump(s, job.clone(), Engine::John)));
    let p2 = stderr.map(|s| tokio::spawn(pump(s, job.clone(), Engine::John)));

    let _ = child.wait().await;
    if let Some(t) = p1 { let _ = t.await; }
    if let Some(t) = p2 { let _ = t.await; }
    clear_pid(job);

    if job.lock().unwrap().cancelled {
        cleanup_john(&hashfile, &potfile, &session).await;
        return Attempt::Cancelled;
    }

    // Ask john what it cracked.
    let mut show = Command::new(JOHN_BIN);
    show.arg("--show").arg(format!("--pot={}", potfile.display()));
    if let Some(ref f) = fmt {
        show.arg(format!("--format={f}"));
    }
    show.arg(&hashfile).kill_on_drop(true);
    let plaintext = match show.output().await {
        Ok(out) => runner::parse_cracked(&String::from_utf8_lossy(&out.stdout)),
        Err(_) => None,
    };

    cleanup_john(&hashfile, &potfile, &session).await;

    if let Some(plain) = plaintext {
        return Attempt::Cracked(plain);
    }
    // Distinguish a real failure (so we can fall back) from a clean "not found".
    let log = job.lock().unwrap().log.clone();
    if log.contains("Unknown option")
        || log.contains("Unknown ciphertext format")
        || log.contains("No password hashes loaded")
        || log.contains("Unknown --format")
    {
        Attempt::Error(format!("john could not run this format. {}", tail(&log, 240)))
    } else {
        Attempt::NotFound
    }
}

async fn cleanup_john(hashfile: &Path, potfile: &Path, session: &Path) {
    let _ = tokio::fs::remove_file(hashfile).await;
    let _ = tokio::fs::remove_file(potfile).await;
    let _ = tokio::fs::remove_file(format!("{}.log", session.display())).await;
    let _ = tokio::fs::remove_file(format!("{}.rec", session.display())).await;
}

/// Stream a child's output line by line, appending to the log and updating
/// progress. The lock is only held per-line, never across the await.
async fn pump<R: AsyncRead + Unpin>(reader: R, job: Arc<Mutex<CrackJob>>, engine: Engine) {
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let mut j = job.lock().unwrap();
        append_log(&mut j.log, &line);
        update_progress(&mut j, engine, &line);
        if let Some(s) = j.started {
            j.elapsed_secs = s.elapsed().as_secs();
        }
    }
}

fn append_log(log: &mut String, line: &str) {
    log.push_str(line);
    log.push('\n');
    if log.len() > LOG_CAP {
        let cut = log.len() - LOG_CAP;
        *log = log[cut..].to_string();
    }
}

/// Parse a progress signal out of one output line.
fn update_progress(j: &mut CrackJob, engine: Engine, line: &str) {
    match engine {
        Engine::Hashcat => {
            // machine-readable: "... PROGRESS <done> <total> ..."
            let toks: Vec<&str> = line.split_whitespace().collect();
            if let Some(i) = toks.iter().position(|t| *t == "PROGRESS") {
                if let (Some(d), Some(t)) = (toks.get(i + 1), toks.get(i + 2)) {
                    if let (Ok(done), Ok(total)) = (d.parse::<u64>(), t.parse::<u64>()) {
                        j.done = done;
                        if total > 0 {
                            j.total = total;
                            j.percent = (done as f32 / total as f32) * 100.0;
                        }
                    }
                }
            }
        }
        Engine::John => {
            // status line carries a percentage like "12.34%"
            for tok in line.split_whitespace() {
                if let Some(num) = tok.strip_suffix('%') {
                    if let Ok(pct) = num.parse::<f32>() {
                        j.percent = pct.clamp(0.0, 100.0);
                        if j.total > 0 {
                            j.done = ((pct / 100.0) * j.total as f32) as u64;
                        }
                    }
                    break;
                }
            }
        }
    }
}

fn set_pid(job: &Arc<Mutex<CrackJob>>, pid: Option<u32>) {
    job.lock().unwrap().pid = pid;
}
fn clear_pid(job: &Arc<Mutex<CrackJob>>) {
    job.lock().unwrap().pid = None;
}

/// Last `n` characters of a string, trimmed, for compact error messages.
fn tail(s: &str, n: usize) -> String {
    let s = s.trim();
    if s.len() <= n {
        s.to_string()
    } else {
        format!("...{}", &s[s.len() - n..])
    }
}
