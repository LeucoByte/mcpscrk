//! HTTP routes: the JSON API plus the embedded frontend.
//!
//! API map:
//!   GET  /                      -> the workbench (index.html)
//!   GET  /static/style.css|app.js
//!   POST /api/profile           -> store OSINT fields, return available materials
//!   GET  /api/materials         -> list available raw materials
//!   GET  /api/expand            -> preview a single value through the block pipeline
//!   POST /api/block             -> craft a block from a material + rules
//!   GET  /api/blocks            -> the block inventory (fixed specials first)
//!   POST /api/block/delete      -> remove a crafted block
//!   POST /api/block/peek        -> first N values of a block (the info popup)
//!   POST /api/metrics           -> estimated size for a blueprint order
//!   POST /api/preview           -> first N generated candidates
//!   POST /api/forge             -> generate the wordlist to a file

use std::collections::HashMap;
use std::path::PathBuf;

use axum::{
    extract::{Multipart, Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::crack::{self, detect, runner};
use crate::engine::{
    block::{Block, BlockRules},
    blueprint,
    expand::CapMode,
    filters::LengthFilter,
    forge::{self, ForgeStats, WriteMode},
    sets::{self, DATES_BLOCK, SEPARATORS_BLOCK, SPECIAL_BLOCK, SYMBOLS_BLOCK},
};

use super::state::{AppState, Workshop};

// Frontend embedded in the binary: one self-contained executable, no assets.
const INDEX_HTML: &str = include_str!("../../web/index.html");
const STYLE_CSS: &str = include_str!("../../web/style.css");
const APP_JS: &str = include_str!("../../web/app.js");

/// Assemble the application router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(|| async { Html(INDEX_HTML) }))
        .route("/static/style.css", get(serve_css))
        .route("/static/app.js", get(serve_js))
        .route("/api/profile", post(set_profile))
        .route("/api/materials", get(list_materials))
        .route("/api/expand", get(expand_word))
        .route("/api/block", post(create_block))
        .route("/api/blocks", get(list_blocks))
        .route("/api/block/delete", post(delete_block))
        .route("/api/block/peek", post(peek_block))
        .route("/api/material/peek", post(peek_material))
        .route("/api/specials", post(set_specials))
        .route("/api/metrics", post(metrics))
        .route("/api/preview", post(preview))
        .route("/api/forge", post(forge_wordlist))
        .route("/api/download", get(download))
        .route("/api/crack/engines", get(crack_engines))
        .route("/api/crack/detect", post(crack_detect))
        .route("/api/crack/upload", post(crack_upload))
        .route("/api/crack/start", post(crack_start))
        .route("/api/crack/status", get(crack_status))
        .route("/api/crack/cancel", post(crack_cancel))
        .with_state(state)
}

// --- static asset handlers ---------------------------------------------------

/// Serve the stylesheet with the correct content type.
async fn serve_css() -> ([(axum::http::HeaderName, &'static str); 1], &'static str) {
    ([(axum::http::header::CONTENT_TYPE, "text/css")], STYLE_CSS)
}

/// Serve the frontend script with the correct content type.
async fn serve_js() -> ([(axum::http::HeaderName, &'static str); 1], &'static str) {
    (
        [(axum::http::header::CONTENT_TYPE, "application/javascript")],
        APP_JS,
    )
}

// --- data transfer objects ---------------------------------------------------

/// A raw material available to craft blocks from.
#[derive(Serialize)]
struct MaterialDto {
    key: String,
    label: String,
    category: String,
    count: usize,
    /// A few raw values, so the UI can pre-fill the "test a word" helper.
    sample: Vec<String>,
}

/// A block in the inventory, with a small sample for the UI.
#[derive(Serialize)]
struct BlockDto {
    name: String,
    source: String,
    count: usize,
    /// True for the permanent specials block: it cannot be edited or deleted.
    fixed: bool,
    sample: Vec<String>,
}

impl BlockDto {
    /// Build a DTO from an engine block, sampling its first few values.
    fn from(block: &Block, fixed: bool) -> Self {
        BlockDto {
            name: block.name.clone(),
            source: block.source.clone(),
            count: block.len(),
            fixed,
            sample: block.values.iter().take(12).cloned().collect(),
        }
    }
}

/// Build the inventory listing: the fixed blocks first (dates, then the three
/// editable symbol blocks), then crafted blocks in creation order.
fn inventory_dto(ws: &Workshop) -> Vec<BlockDto> {
    let mut blocks = vec![
        BlockDto::from(&ws.dates, true),
        BlockDto::from(&ws.separators, true),
        BlockDto::from(&ws.specials, true),
        BlockDto::from(&ws.symbols, true),
    ];
    blocks.extend(ws.inventory.iter().map(|b| BlockDto::from(b, false)));
    blocks
}

/// Whether a name belongs to a permanent (non-craftable) block.
fn is_reserved(name: &str) -> bool {
    matches!(name, DATES_BLOCK | SEPARATORS_BLOCK | SPECIAL_BLOCK | SYMBOLS_BLOCK)
}

// --- profile / materials -----------------------------------------------------

/// Body of `POST /api/profile`: a map of parameter key -> comma-separated input.
#[derive(Deserialize)]
struct ProfileBody {
    fields: HashMap<String, String>,
}

/// Store the OSINT fields and return the resulting list of materials.
async fn set_profile(State(state): State<AppState>, Json(body): Json<ProfileBody>) -> Json<Vec<MaterialDto>> {
    let mut ws = state.workshop.lock().unwrap();
    for (key, raw) in &body.fields {
        ws.profile.set_field(key, raw);
    }
    // The dates block is auto-derived, so refresh it whenever the profile changes.
    ws.rebuild_dates();
    Json(collect_materials(&ws.profile))
}

/// Return the current list of available materials.
async fn list_materials(State(state): State<AppState>) -> Json<Vec<MaterialDto>> {
    let ws = state.workshop.lock().unwrap();
    Json(collect_materials(&ws.profile))
}

/// Build the material list from the profile, in catalog order.
///
/// Special characters and dates are deliberately excluded here: they are
/// exposed as permanent blocks in the inventory, not as craftable materials.
fn collect_materials(profile: &sets::ProfileSets) -> Vec<MaterialDto> {
    let mut materials = Vec::new();
    for entry in sets::catalog() {
        if entry.key == "special" || entry.key == "dates" {
            continue;
        }
        if let Some(values) = profile.get(entry.key) {
            if !values.is_empty() {
                materials.push(MaterialDto {
                    key: entry.key.to_string(),
                    label: entry.label.to_string(),
                    category: entry.category.label().to_string(),
                    count: values.len(),
                    sample: values.iter().take(3).cloned().collect(),
                });
            }
        }
    }
    materials
}

// --- single-value expansion preview -----------------------------------------

/// Query of `GET /api/expand`.
#[derive(Deserialize)]
struct ExpandQuery {
    word: String,
    /// Optional source key, so a `dates` value runs through the date engine,
    /// exactly like a real block would.
    #[serde(default)]
    source: String,
    #[serde(default)]
    cap: String,
    #[serde(default)]
    leet: bool,
}

/// Response of `GET /api/expand`.
#[derive(Serialize)]
struct ExpandResponse {
    variants: Vec<String>,
    count: usize,
}

/// Preview every form a single value expands into under the given rules.
///
/// This is a one-value run through the exact block pipeline, so what the user
/// sees here is what a block built from that value would contain.
async fn expand_word(Query(q): Query<ExpandQuery>) -> Json<ExpandResponse> {
    let rules = BlockRules {
        cap: CapMode::parse(&q.cap),
        leet: q.leet,
    };
    let block = Block::build("preview", &q.source, std::slice::from_ref(&q.word), rules);

    let count = block.values.len();
    let mut variants = block.values;
    variants.truncate(1000); // keep the preview payload bounded
    Json(ExpandResponse { variants, count })
}

// --- blocks ------------------------------------------------------------------

/// Body of `POST /api/block`.
#[derive(Deserialize)]
struct CreateBlockBody {
    name: String,
    source: String,
    #[serde(default)]
    cap: String,
    #[serde(default)]
    leet: bool,
}

/// Response carrying the full inventory (plus an optional error message).
#[derive(Serialize)]
struct BlocksResponse {
    blocks: Vec<BlockDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Craft a block from a material and a set of rules, adding it to the inventory.
/// A block with the same name is replaced. An empty name defaults to the source
/// key. The reserved specials name cannot be used.
async fn create_block(State(state): State<AppState>, Json(body): Json<CreateBlockBody>) -> Json<BlocksResponse> {
    let mut ws = state.workshop.lock().unwrap();

    let raw = match ws.profile.get(&body.source) {
        Some(values) => values.to_vec(),
        None => {
            return Json(BlocksResponse {
                blocks: inventory_dto(&ws),
                error: Some(format!("Material '{}' has no values.", body.source)),
            });
        }
    };

    // An empty name defaults to the capitalized source key ("username" -> "Username").
    let name = if body.name.trim().is_empty() {
        sets::capitalize_first(&body.source)
    } else {
        body.name.trim().to_string()
    };

    if is_reserved(&name) {
        return Json(BlocksResponse {
            blocks: inventory_dto(&ws),
            error: Some(format!("'{name}' is a reserved, fixed block.")),
        });
    }

    let rules = BlockRules {
        cap: CapMode::parse(&body.cap),
        leet: body.leet,
    };
    let block = Block::build(&name, &body.source, &raw, rules);

    ws.inventory.retain(|b| b.name != name);
    ws.inventory.push(block);

    Json(BlocksResponse {
        blocks: inventory_dto(&ws),
        error: None,
    })
}

/// Return the current block inventory.
async fn list_blocks(State(state): State<AppState>) -> Json<BlocksResponse> {
    let ws = state.workshop.lock().unwrap();
    Json(BlocksResponse {
        blocks: inventory_dto(&ws),
        error: None,
    })
}

/// Body of `POST /api/block/delete`.
#[derive(Deserialize)]
struct DeleteBlockBody {
    name: String,
}

/// Remove a crafted block. The fixed specials block is never removed.
async fn delete_block(State(state): State<AppState>, Json(body): Json<DeleteBlockBody>) -> Json<BlocksResponse> {
    let mut ws = state.workshop.lock().unwrap();
    if body.name != SPECIAL_BLOCK {
        ws.inventory.retain(|b| b.name != body.name);
    }
    Json(BlocksResponse {
        blocks: inventory_dto(&ws),
        error: None,
    })
}

/// Body of `POST /api/specials`: edit one editable symbol block by name.
#[derive(Deserialize)]
struct SpecialsBody {
    name: String,
    values: String,
}

/// Replace the contents of an editable symbol block (empty input restores its
/// defaults).
async fn set_specials(State(state): State<AppState>, Json(body): Json<SpecialsBody>) -> Json<BlocksResponse> {
    let mut ws = state.workshop.lock().unwrap();
    ws.edit_symbols(&body.name, &body.values);
    Json(BlocksResponse {
        blocks: inventory_dto(&ws),
        error: None,
    })
}

/// Body of `POST /api/block/peek`.
#[derive(Deserialize)]
struct PeekBody {
    name: String,
    #[serde(default = "default_peek")]
    limit: usize,
}

/// Default number of values returned by the info popup.
fn default_peek() -> usize {
    50
}

/// Response of `POST /api/block/peek`.
#[derive(Serialize)]
struct PeekResponse {
    name: String,
    count: usize,
    values: Vec<String>,
}

/// Return the first `limit` values of a block, for the inventory/blueprint info
/// popup.
async fn peek_block(State(state): State<AppState>, Json(body): Json<PeekBody>) -> Json<PeekResponse> {
    let ws = state.workshop.lock().unwrap();
    match ws.block(&body.name) {
        Some(b) => Json(PeekResponse {
            name: b.name.clone(),
            count: b.len(),
            values: b.values.iter().take(body.limit).cloned().collect(),
        }),
        None => Json(PeekResponse {
            name: body.name,
            count: 0,
            values: Vec::new(),
        }),
    }
}

/// Body of `POST /api/material/peek`.
#[derive(Deserialize)]
struct PeekMaterialBody {
    key: String,
    #[serde(default = "default_peek")]
    limit: usize,
}

/// Return the first `limit` raw values of a profile material, for its info popup.
async fn peek_material(State(state): State<AppState>, Json(body): Json<PeekMaterialBody>) -> Json<PeekResponse> {
    let ws = state.workshop.lock().unwrap();
    let values = ws.profile.get(&body.key).unwrap_or(&[]);
    Json(PeekResponse {
        name: body.key.clone(),
        count: values.len(),
        values: values.iter().take(body.limit).cloned().collect(),
    })
}

// --- metrics / preview / forge ----------------------------------------------

/// Body of `POST /api/metrics`.
#[derive(Deserialize)]
struct MetricsBody {
    order: Vec<String>,
}

/// Per-block size and the estimated total for a blueprint.
#[derive(Serialize)]
struct MetricsResponse {
    blocks: Vec<MetricBlock>,
    /// Estimated total as a string: it can exceed JavaScript's safe integer.
    estimated: String,
}

#[derive(Serialize)]
struct MetricBlock {
    name: String,
    count: usize,
}

/// Compute the estimated wordlist size for a given block order.
async fn metrics(State(state): State<AppState>, Json(body): Json<MetricsBody>) -> Json<MetricsResponse> {
    let ws = state.workshop.lock().unwrap();
    let resolved = ws.resolve(&body.order);
    let estimated = blueprint::estimated_size(&resolved);
    Json(MetricsResponse {
        blocks: resolved
            .iter()
            .map(|b| MetricBlock {
                name: b.name.clone(),
                count: b.len(),
            })
            .collect(),
        estimated: estimated.to_string(),
    })
}

/// Build a length filter, guarding against an inverted or zero range.
fn length_filter(min: usize, max: usize) -> LengthFilter {
    let lo = min.max(1);
    let hi = max.max(lo);
    LengthFilter { min: lo, max: hi }
}

/// Body of `POST /api/preview`.
#[derive(Deserialize)]
struct PreviewBody {
    order: Vec<String>,
    min: usize,
    max: usize,
    #[serde(default = "default_limit")]
    limit: usize,
}

/// Default number of preview lines.
fn default_limit() -> usize {
    50
}

/// Response of `POST /api/preview`.
#[derive(Serialize)]
struct PreviewResponse {
    lines: Vec<String>,
    estimated: String,
    stats: ForgeStats,
}

/// Generate the first `limit` candidates without writing to disk.
async fn preview(State(state): State<AppState>, Json(body): Json<PreviewBody>) -> Json<PreviewResponse> {
    let ws = state.workshop.lock().unwrap();
    let resolved = ws.resolve(&body.order);
    let filter = length_filter(body.min, body.max);
    let estimated = blueprint::estimated_size(&resolved).to_string();
    let (lines, stats) = forge::preview(&resolved, &filter, body.limit.min(2000));
    Json(PreviewResponse { lines, estimated, stats })
}

/// Body of `POST /api/forge`.
#[derive(Deserialize)]
struct ForgeBody {
    order: Vec<String>,
    min: usize,
    max: usize,
    mode: WriteMode,
    path: String,
}

/// Response of `POST /api/forge`.
#[derive(Serialize)]
struct ForgeResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    stats: Option<ForgeStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Resolve a user-supplied output name to a temp file. We always keep wordlists
/// under the system temp dir, keyed by their bare file name, so the same name
/// can be forged (overwrite/append) and later downloaded without juggling paths.
fn resolve_output(name: &str) -> PathBuf {
    let base = std::path::Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .map(sanitize_filename)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "wordlist.txt".to_string());
    std::env::temp_dir().join(base)
}

/// Generate the full wordlist to a temp file named after `body.path`.
async fn forge_wordlist(State(state): State<AppState>, Json(body): Json<ForgeBody>) -> Json<ForgeResponse> {
    let ws = state.workshop.lock().unwrap();
    let resolved = ws.resolve(&body.order);

    if resolved.is_empty() {
        return Json(ForgeResponse {
            stats: None,
            path: None,
            error: Some("The blueprint is empty: add at least one block.".into()),
        });
    }

    let filter = length_filter(body.min, body.max);
    let path = resolve_output(&body.path);

    match forge::forge(&resolved, &filter, body.mode, &path) {
        Ok(report) => Json(ForgeResponse {
            stats: Some(report.stats),
            path: Some(report.path),
            error: None,
        }),
        Err(e) => Json(ForgeResponse {
            stats: None,
            path: None,
            error: Some(e.to_string()),
        }),
    }
}

// --- download ---------------------------------------------------------------

/// Query of `GET /api/download`.
#[derive(Deserialize)]
struct DownloadQuery {
    path: String,
}

/// Serve a generated wordlist as a file download. The query carries the bare
/// file name; the file lives under the temp dir (same place `forge` writes it).
async fn download(Query(q): Query<DownloadQuery>) -> impl IntoResponse {
    let path = resolve_output(&q.path);
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("wordlist.txt")
        .to_string();

    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "text/plain; charset=utf-8".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{filename}\""),
                ),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            format!("No wordlist named '{filename}' yet - forge it first."),
        )
            .into_response(),
    }
}

// --- cracking lab ------------------------------------------------------------

/// Report which cracking engines are installed.
#[derive(Serialize)]
struct EnginesResponse {
    hashcat: bool,
    john: bool,
}

/// `GET /api/crack/engines`: which engines are available on this machine.
async fn crack_engines() -> Json<EnginesResponse> {
    let (hashcat, john) = runner::available().await;
    Json(EnginesResponse { hashcat, john })
}

/// Body of `POST /api/crack/detect`.
#[derive(Deserialize)]
struct DetectBody {
    hash: String,
}

/// Detect candidate hash types. Returns hashcat's structural candidates (the
/// dropdown) plus John's independent opinion as a cross-check.
async fn crack_detect(Json(body): Json<DetectBody>) -> Json<detect::Detection> {
    Json(detect::detect(&body.hash).await)
}

/// Response of `POST /api/crack/upload`.
#[derive(Serialize)]
struct UploadResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    lines: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Receive an uploaded wordlist (multipart) and store it in a temp file.
async fn crack_upload(mut multipart: Multipart) -> Json<UploadResponse> {
    while let Ok(Some(field)) = multipart.next_field().await {
        let filename = field.file_name().unwrap_or("wordlist.txt").to_string();
        let safe = sanitize_filename(&filename);
        let data = match field.bytes().await {
            Ok(d) => d,
            Err(e) => {
                return Json(UploadResponse {
                    path: None,
                    lines: 0,
                    error: Some(format!("Upload failed: {e}")),
                })
            }
        };
        let dest = std::env::temp_dir().join(format!("mcpscrk-wl-{safe}"));
        if let Err(e) = tokio::fs::write(&dest, &data).await {
            return Json(UploadResponse {
                path: None,
                lines: 0,
                error: Some(format!("Could not save upload: {e}")),
            });
        }
        let lines = data.iter().filter(|b| **b == b'\n').count().max(1);
        return Json(UploadResponse {
            path: Some(dest.display().to_string()),
            lines,
            error: None,
        });
    }
    Json(UploadResponse {
        path: None,
        lines: 0,
        error: Some("No file received.".into()),
    })
}

/// Keep only safe characters in an uploaded file name.
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' { c } else { '_' })
        .collect()
}

/// Body of `POST /api/crack/start`.
#[derive(Deserialize)]
struct RunBody {
    hash: String,
    engine: runner::Engine,
    mode: Option<u32>,
    wordlist: String,
}

/// Simple acknowledgement for start/cancel.
#[derive(Serialize)]
struct AckResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Count the entries (non-empty lines) of the wordlist, for the progress bar.
async fn count_entries(path: &std::path::Path) -> u64 {
    match tokio::fs::read(path).await {
        Ok(bytes) => bytes
            .split(|b| *b == b'\n')
            .filter(|l| !l.is_empty())
            .count() as u64,
        Err(_) => 0,
    }
}

/// Launch the attack in the background. The UI then polls `/api/crack/status`.
async fn crack_start(State(state): State<AppState>, Json(body): Json<RunBody>) -> Json<AckResponse> {
    // Refuse to start a second job while one is running.
    if state.crack.lock().unwrap().running {
        return Json(AckResponse {
            ok: false,
            error: Some("A crack is already running. Cancel it first.".into()),
        });
    }

    let wordlist = PathBuf::from(&body.wordlist);
    if tokio::fs::metadata(&wordlist).await.is_err() {
        return Json(AckResponse {
            ok: false,
            error: Some("Wordlist not found - upload one first.".into()),
        });
    }

    let available = runner::available().await;
    let total = count_entries(&wordlist).await;

    crack::job::begin(&state.crack, total);

    let crack = state.crack.clone();
    let RunBody { hash, engine, mode, .. } = body;
    tokio::spawn(async move {
        crack::job::run(crack, available, engine, hash, mode, wordlist).await;
    });

    Json(AckResponse { ok: true, error: None })
}

/// Snapshot of the current/last job for the live progress UI.
async fn crack_status(State(state): State<AppState>) -> Json<crack::job::CrackJob> {
    Json(state.crack.lock().unwrap().clone())
}

/// Cancel the running job (kills the engine process).
async fn crack_cancel(State(state): State<AppState>) -> Json<AckResponse> {
    crack::job::cancel(&state.crack);
    Json(AckResponse { ok: true, error: None })
}
