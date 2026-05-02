//! Embedded HTTP server: dashboard at http://127.0.0.1:18790/
//!
//! Endpoints:
//!   GET  /                    — single-page HTML
//!   GET  /api/status          — providers, default, cron count, telegram on/off
//!   GET  /api/cron            — list of jobs + their state
//!   GET  /api/cron/runs       — last 100 cron runs (jsonl tail)
//!   GET  /api/sessions        — list session files (with sizes)
//!   GET  /api/sessions/:name  — return one session jsonl as a JSON array
//!   GET  /api/tools           — last 100 tool audit entries
//!   POST /api/cron/run        — trigger a job: { "job_id": "..." }
//!   POST /api/ask             — { "prompt", "provider"?, "model"? } → reply
//!
//! Auth: required bearer token. main.rs auto-generates one at startup if
//! [ui].token is unset; the value is persisted at ~/wormhole/.token. The
//! INDEX_HTML page itself is unauthenticated (so the browser can bootstrap
//! and prompt for the token), but every /api/* route requires it.

use crate::brain::{Brain, RespondOpts};
use crate::config::Config;
use crate::cron::{CronRunner, Job};
use axum::{
    extract::{FromRequestParts, Path, State},
    http::{request::Parts, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use tower_http::cors::{AllowOrigin, CorsLayer};
use axum::http::{header, HeaderValue, Method};
use std::collections::HashSet;
use axum::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;

#[derive(Clone)]
pub struct UiState {
    pub cfg: Arc<Config>,
    pub brain: Arc<Brain>,
    pub cron: Arc<CronRunner>,
    pub telegram_on: bool,
}

pub fn router(state: UiState) -> Router {
    // CORS: hard allow-list from `ui.cors_origins` (default
    // `["http://127.0.0.1:18900"]`, the openbrain dashboard). Codex finding #41:
    // the previous `Any` (and later loopback-predicate) versions both meant a
    // random tab could become a control plane for Larry's mutating endpoints.
    // Now an origin must match the configured list exactly.
    let allowed: HashSet<String> = state.cfg.ui_cors_origins.iter().cloned().collect();
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin: &HeaderValue, _| {
            origin
                .to_str()
                .map(|s| allowed.contains(s))
                .unwrap_or(false)
        }))
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
        .allow_credentials(true);
    Router::new()
        .route("/", get(index))
        .route("/api/status", get(api_status))
        .route("/api/cron", get(api_cron))
        .route("/api/cron/runs", get(api_cron_runs))
        .route("/api/cron/run", post(api_cron_run))
        .route("/api/cron/reload", post(api_cron_reload))
        .route("/api/scout/actions", get(api_scout_actions))
        .route("/api/scout/action", post(api_scout_action))
        .route("/api/sessions", get(api_sessions))
        .route("/api/sessions/:name", get(api_session_one))
        .route("/api/threads", get(api_threads_list).post(api_threads_create))
        .route("/api/threads/:id", get(api_thread_one))
        .route("/api/threads/:id/compact", post(api_thread_compact))
        .route("/api/threads/:id/label", post(api_thread_label))
        .route("/api/tools", get(api_tools))
        .route("/api/skills/usage", get(api_skills_usage))
        .route("/api/bugs", get(api_bugs))
        .route("/api/memory/proposed", get(api_memory_proposed))
        .route("/api/memory/promote", post(api_memory_promote))
        .route("/api/research", get(api_research))
        .route("/api/ask", post(api_ask))
        .layer(cors)
        .with_state(state)
}

pub async fn serve(state: UiState) -> std::io::Result<()> {
    let bind = format!("{}:{}", state.cfg.ui_bind, state.cfg.ui_port);
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!(bind = %bind, "ui listening");
    let app = router(state);
    axum::serve(listener, app).await
}

/// Axum extractor: pulls the configured token from `UiState`, checks the
/// request's `Authorization: Bearer <token>` header (constant-time compare),
/// and short-circuits to 401 if invalid. Handlers that need auth take a
/// `_: Authed` parameter; handlers that don't (like the index page) skip it.
/// Replaces the per-handler `if !auth_ok(...) { return unauthorized() }` boilerplate
/// from the previous version (Codex review #7).
pub struct Authed;

#[async_trait]
impl FromRequestParts<UiState> for Authed {
    type Rejection = (StatusCode, Json<Value>);

    async fn from_request_parts(parts: &mut Parts, state: &UiState) -> std::result::Result<Self, Self::Rejection> {
        if auth_ok(state, &parts.headers) {
            Ok(Authed)
        } else {
            Err(unauthorized())
        }
    }
}

/// Bearer-token check. Token is **always required** for protected routes —
/// even on loopback, since a browser tab on the same machine can otherwise
/// be turned into a control plane via DNS rebinding or co-resident origins.
/// `main.rs::ensure_ui_token` guarantees `ui_token` is `Some(non-empty)` by
/// startup, but we still defensively reject when it isn't. Codex finding #73.
fn auth_ok(state: &UiState, headers: &HeaderMap) -> bool {
    let Some(want) = state.cfg.ui_token.as_deref() else {
        return false;
    };
    if want.is_empty() {
        return false;
    }
    let h = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("");
    if let Some(t) = h.strip_prefix("Bearer ") {
        // Constant-time compare to avoid timing oracles on a small token.
        if t.len() != want.len() {
            return false;
        }
        let mut diff: u8 = 0;
        for (a, b) in t.bytes().zip(want.bytes()) {
            diff |= a ^ b;
        }
        return diff == 0;
    }
    false
}

fn unauthorized() -> (StatusCode, Json<Value>) {
    (StatusCode::UNAUTHORIZED, Json(json!({ "error": "missing or bad bearer token; configure ui.token in config.toml or read ~/wormhole/.token" })))
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn api_status(State(state): State<UiState>, _: Authed) -> impl IntoResponse {
    let providers = state.brain.list_providers();
    let cron_jobs = state.cron.list_jobs().await.len();
    let resp = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "providers": providers,
        "default_provider": state.cfg.default_provider,
        "fallback_chain": state.cfg.fallback_chain,
        "max_tokens": state.cfg.max_tokens,
        "tools_enabled": state.brain.tools_enabled(),
        "tool_policy": state.brain.tool_policy(),
        "skills": state.brain.skills().names(),
        "cron_job_count": cron_jobs,
        "telegram": state.telegram_on,
        "larry_home": state.cfg.larry_home.display().to_string(),
        "workspace_root": state.cfg.workspace_root.display().to_string(),
    });
    Json(resp).into_response()
}

async fn api_cron(State(state): State<UiState>, _: Authed) -> impl IntoResponse {
    let jobs = state.cron.list_jobs().await;
    let state_path = state.cfg.larry_home.join("cron-state.json");
    let states: Value = fs::read_to_string(&state_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}));
    let body: Vec<Value> = jobs
        .into_iter()
        .map(|j: Job| {
            json!({
                "id": j.id,
                "name": j.name,
                "kind": j.kind,
                "schedule": j.schedule,
                "tz": "host-local",
                "deliver": j.deliver,
                "description": j.description,
                "state": states.get(&j.id).cloned().unwrap_or(Value::Null),
            })
        })
        .collect();
    Json(body).into_response()
}

#[derive(Deserialize)]
struct CronRunReq {
    job_id: String,
}

async fn api_cron_run(
    State(state): State<UiState>,
    _: Authed,
    Json(req): Json<CronRunReq>,
) -> impl IntoResponse {
    match state.cron.trigger(&req.job_id).await {
        Ok(()) => Json(json!({ "ok": true, "job_id": req.job_id, "status": "spawned" })).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": e.to_string() }))).into_response(),
    }
}

async fn api_cron_reload(State(state): State<UiState>, _: Authed) -> impl IntoResponse {
    match state.cron.reload_jobs().await {
        Ok((added, removed)) => Json(json!({
            "ok": true,
            "added": added,
            "removed": removed,
            "active": state.cron.list_jobs().await.len(),
        })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ).into_response(),
    }
}

// ---------- /api/scout/actions ----------
//
// Stores per-item "done"/"queue"/"dismiss" state for AI-scout findings (and
// any other feed-kind widget). Persists to ~/openbrain/data/scout-actions.json
// so Larry can also read it (e.g. the followup cron in cron.toml).

#[derive(Deserialize)]
struct ScoutActionReq {
    /// Stable identifier for the item — usually the URL. If absent, fall back to title.
    item_id: String,
    /// "done" | "queue" | "dismiss" | "clear"
    state: String,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    source: Option<String>,
    /// Which widget the item came from (for grouping). e.g. "ai-scout-rolling".
    #[serde(default)]
    widget_id: Option<String>,
}

fn scout_actions_path(_state: &UiState) -> std::path::PathBuf {
    // Sit alongside the widget data so Larry's read_file / cron jobs can reach it.
    // Defaults to ~/openbrain/data; future: pull from cfg.openbrain_data_dir.
    dirs::home_dir()
        .map(|h| h.join("openbrain").join("data").join("scout-actions.json"))
        .unwrap_or_else(|| std::path::PathBuf::from("openbrain/data/scout-actions.json"))
}

async fn api_scout_actions(State(state): State<UiState>, _: Authed) -> impl IntoResponse {
    let p = scout_actions_path(&state);
    let v: Value = fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({ "items": {} }));
    Json(v).into_response()
}

async fn api_scout_action(
    State(state): State<UiState>,
    _: Authed,
    Json(req): Json<ScoutActionReq>,
) -> impl IntoResponse {
    let p = scout_actions_path(&state);
    if let Some(parent) = p.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut current: Value = fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({ "items": {} }));
    if !current.get("items").map(|v| v.is_object()).unwrap_or(false) {
        current["items"] = json!({});
    }
    let now = chrono::Local::now().to_rfc3339();

    {
        let items = current["items"].as_object_mut().unwrap();
        if req.state == "clear" {
            items.remove(&req.item_id);
        } else {
            let mut entry = items.get(&req.item_id).cloned().unwrap_or_else(|| json!({}));
            entry["state"] = json!(req.state);
            entry["updated_at"] = json!(now);
            if let Some(notes) = req.notes.as_ref() { entry["notes"] = json!(notes); }
            if let Some(title) = req.title.as_ref() { entry["title"] = json!(title); }
            if let Some(source) = req.source.as_ref() { entry["source"] = json!(source); }
            if let Some(widget_id) = req.widget_id.as_ref() { entry["widget_id"] = json!(widget_id); }
            items.insert(req.item_id.clone(), entry);
        }
    }
    current["updated_at"] = json!(now);
    let count = current["items"].as_object().map(|o| o.len()).unwrap_or(0);

    match fs::write(&p, serde_json::to_string_pretty(&current).unwrap_or_default()) {
        Ok(()) => Json(json!({ "ok": true, "count": count })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        ).into_response(),
    }
}

async fn api_cron_runs(State(state): State<UiState>, _: Authed) -> impl IntoResponse {
    let path = state.cfg.logs_dir.join("cron-runs.jsonl");
    let lines = read_tail_jsonl(&path, 100);
    Json(lines).into_response()
}

// ---------- /api/bugs ----------
//
// Parses the human-edited BUGS.md tracker into structured cards for the
// dashboard. Format expected per bug:
//
//   ### BUG-NNN: <title>
//   - **Severity**: ...
//   - **Symptoms**: ...
//   - **Status**: OPEN | CLOSED
//
// We don't aim to be a full markdown parser — the file is hand-maintained, so
// any deviations show up as missing fields and the UI just renders blanks.

async fn api_bugs(State(state): State<UiState>, _: Authed) -> impl IntoResponse {
    let path = state.cfg.larry_home.join("BUGS.md");
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return Json(json!({ "bugs": [], "file_mtime": Value::Null })).into_response(),
    };
    let file_mtime = fs::metadata(&path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| chrono::DateTime::<chrono::Local>::from(std::time::UNIX_EPOCH + d).to_rfc3339());

    #[derive(Default)]
    struct B {
        id: String,
        title: String,
        severity: String,
        status: String,
        description: String,
    }
    let mut bugs: Vec<B> = Vec::new();
    let mut cur: Option<B> = None;

    for raw in text.lines() {
        let line = raw.trim_end();
        if let Some(rest) = line.strip_prefix("### BUG-") {
            if let Some(b) = cur.take() {
                bugs.push(b);
            }
            // rest looks like "001: Title goes here"
            if let Some((num, title)) = rest.split_once(':') {
                cur = Some(B {
                    id: format!("BUG-{}", num.trim()),
                    title: title.trim().to_string(),
                    ..Default::default()
                });
            } else {
                cur = Some(B {
                    id: format!("BUG-{}", rest.trim()),
                    ..Default::default()
                });
            }
            continue;
        }
        // A blank '### ' (non-BUG) or '## ' header ends the current bug.
        if line.starts_with("## ") || (line.starts_with("### ") && !line.starts_with("### BUG-")) {
            if let Some(b) = cur.take() {
                bugs.push(b);
            }
            continue;
        }
        let Some(ref mut b) = cur else { continue };
        if let Some(rest) = line.strip_prefix("- **Severity**:") {
            b.severity = rest.split_whitespace().next().unwrap_or("").trim_matches(|c: char| !c.is_alphanumeric()).to_string();
        } else if let Some(rest) = line.strip_prefix("- **Status**:") {
            let r = rest.trim();
            b.status = if r.contains("CLOSED") { "CLOSED".into() }
                else if r.contains("OPEN") { "OPEN".into() }
                else { r.split_whitespace().next().unwrap_or("").to_string() };
        } else if b.description.is_empty() {
            if let Some(rest) = line.strip_prefix("- **Symptoms**:") {
                b.description = rest.trim().to_string();
            }
        }
    }
    if let Some(b) = cur.take() {
        bugs.push(b);
    }

    let out: Vec<Value> = bugs.into_iter().map(|b| json!({
        "id": b.id,
        "title": b.title,
        "severity": b.severity,
        "status": b.status,
        "description": b.description,
    })).collect();

    Json(json!({ "bugs": out, "file_mtime": file_mtime })).into_response()
}

// ---------- /api/memory/proposed ----------
//
// Pulls confidence-tagged proposals from MEMORY-proposed.md (written by the
// memory-dreaming cron). Only lines beginning with `- [HIGH]`, `- [MEDIUM]`,
// or `- [LOW]` are surfaced; section headings and evidence sub-lines are
// skipped so the UI stays focused on actionable items.

async fn api_memory_proposed(State(state): State<UiState>, _: Authed) -> impl IntoResponse {
    let path = state.cfg.workspace_root.join("MEMORY-proposed.md");
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return Json(json!({ "items": [], "count": 0, "file_mtime": Value::Null })).into_response(),
    };
    let file_mtime = fs::metadata(&path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| chrono::DateTime::<chrono::Local>::from(std::time::UNIX_EPOCH + d).to_rfc3339());

    let mut items: Vec<Value> = Vec::new();
    for line in text.lines() {
        let l = line.trim_start();
        for level in ["HIGH", "MEDIUM", "LOW"] {
            let prefix = format!("- [{}]", level);
            if let Some(rest) = l.strip_prefix(&prefix) {
                items.push(json!({
                    "confidence": level,
                    "text": rest.trim().to_string(),
                }));
                break;
            }
        }
    }
    let count = items.len();
    Json(json!({ "items": items, "count": count, "file_mtime": file_mtime })).into_response()
}

async fn api_memory_promote(State(_state): State<UiState>, _: Authed) -> impl IntoResponse {
    Json(json!({ "ok": true, "message": "send promote all via Telegram" })).into_response()
}

// ---------- /api/research ----------
//
// Aggregates per-harness scoreboards from the autoresearch decisions.jsonl
// log. Each harness directory has a results/decisions.jsonl with one event
// per line ({"event": "baseline"|"kept"|"reverted", "score": ..., "ts": ...}).
// We compute baseline (latest baseline event), current (max kept score, or
// baseline if none), kept/reverted counts, and a stale flag for harnesses
// with no activity in the last 24 hours.

fn parse_research_dir(name: &str, dir: &std::path::Path) -> Option<Value> {
    let dec = dir.join("results").join("decisions.jsonl");
    let text = fs::read_to_string(&dec).ok()?;
    let mut baseline: Option<f64> = None;
    let mut best_kept: Option<f64> = None;
    let mut last_reverted: Option<f64> = None;
    let mut kept_count = 0u64;
    let mut reverted_count = 0u64;
    let mut last_ts = String::new();

    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let ev = v.get("event").and_then(|s| s.as_str()).unwrap_or("");
        let score = v.get("score").and_then(|s| s.as_f64());
        let ts = v.get("ts").and_then(|s| s.as_str()).unwrap_or("");
        match ev {
            "baseline" => {
                if let Some(s) = score { baseline = Some(s); }
            }
            "kept" => {
                kept_count += 1;
                if let Some(s) = score {
                    best_kept = Some(best_kept.map_or(s, |b| b.max(s)));
                }
            }
            "reverted" => {
                reverted_count += 1;
                if let Some(s) = score { last_reverted = Some(s); }
            }
            _ => {}
        }
        if !ts.is_empty() && ts > last_ts.as_str() {
            last_ts = ts.to_string();
        }
    }

    let baseline_v = baseline.unwrap_or(0.0);
    let current = best_kept.unwrap_or(baseline_v);
    let delta = current - baseline_v;

    // "stale" if no event in the last 24h. Be tolerant of varying ts forms by
    // trying RFC3339 first and skipping the staleness check on parse failure.
    let status = if last_ts.is_empty() {
        "stale"
    } else {
        match chrono::DateTime::parse_from_rfc3339(&last_ts) {
            Ok(t) => {
                let age = chrono::Utc::now().signed_duration_since(t.with_timezone(&chrono::Utc));
                if age.num_hours() > 24 { "stale" } else { "active" }
            }
            Err(_) => "active",
        }
    };

    Some(json!({
        "name": name,
        "path": dir.display().to_string(),
        "baseline": baseline_v,
        "current": current,
        "delta": delta,
        "kept": kept_count,
        "reverted": reverted_count,
        "last_reverted": last_reverted,
        "last_ts": last_ts,
        "status": status,
    }))
}

async fn api_research(State(state): State<UiState>, _: Authed) -> impl IntoResponse {
    let mut out: Vec<Value> = Vec::new();

    let root = state.cfg.workspace_root.join("autoresearch");
    if let Ok(rd) = fs::read_dir(&root) {
        for entry in rd.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(v) = parse_research_dir(&name, &p) {
                out.push(v);
            }
        }
    }

    out.sort_by(|a, b| {
        let da = a.get("delta").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let db = b.get("delta").and_then(|v| v.as_f64()).unwrap_or(0.0);
        db.partial_cmp(&da).unwrap_or(std::cmp::Ordering::Equal)
    });

    Json(out).into_response()
}

#[derive(Serialize)]
struct SessionMeta {
    name: String,
    size_bytes: u64,
    modified: String,
}

async fn api_sessions(State(state): State<UiState>, _: Authed) -> impl IntoResponse {
    let dir = &state.cfg.sessions_dir;
    let mut metas: Vec<SessionMeta> = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let md = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let name = entry.file_name().to_string_lossy().to_string();
            let size_bytes = md.len();
            let modified = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| chrono::DateTime::<chrono::Local>::from(std::time::UNIX_EPOCH + d).to_rfc3339())
                .unwrap_or_default();
            metas.push(SessionMeta { name, size_bytes, modified });
        }
    }
    metas.sort_by(|a, b| b.modified.cmp(&a.modified));
    Json(metas).into_response()
}

async fn api_session_one(
    State(state): State<UiState>,
    _: Authed,
    Path(name): Path<String>,
) -> impl IntoResponse {
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid name" }))).into_response();
    }
    let path = state.cfg.sessions_dir.join(&name);
    let lines = read_tail_jsonl(&path, 1000);
    Json(lines).into_response()
}

async fn api_tools(State(state): State<UiState>, _: Authed) -> impl IntoResponse {
    let path = state.cfg.logs_dir.join("tools.jsonl");
    let lines = read_tail_jsonl(&path, 100);
    Json(lines).into_response()
}

/// Skill usage snapshot for the curator dashboard / cron jobs. Returns every
/// loaded skill plus any unknown name that was ever passed to `load_skill`,
/// sorted by use count desc.
async fn api_skills_usage(State(state): State<UiState>, _: Authed) -> impl IntoResponse {
    let ranked = state.brain.skills().usage_ranked();
    let total: u64 = ranked.iter().map(|(_, c)| *c).sum();
    let skills: Vec<Value> = ranked
        .into_iter()
        .map(|(name, uses)| json!({ "name": name, "uses": uses }))
        .collect();
    Json(json!({ "total": total, "skills": skills })).into_response()
}

#[derive(Deserialize)]
struct AskReq {
    prompt: String,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
}

async fn api_ask(
    State(state): State<UiState>,
    _: Authed,
    Json(req): Json<AskReq>,
) -> impl IntoResponse {
    let session_id = req.session_id.unwrap_or_else(|| "ui:default".into());

    // Auto-label: if this is a ui:* session and no meta/label exists yet, derive
    // one from the first 60 chars of the opening message. Failures are warnings;
    // they shouldn't block the response.
    if session_id.starts_with("ui:") {
        let path = meta_path_for(&state, &session_id);
        let existing = read_meta(&path);
        let needs_label = existing
            .get("label")
            .and_then(|v| v.as_str())
            .map(|s| s.is_empty() || s == "new conversation")
            .unwrap_or(true);
        if needs_label {
            let label: String = req.prompt.trim().chars().take(60).collect();
            if !label.is_empty() {
                let mut meta = if existing.is_object() { existing } else { json!({}) };
                meta["session_id"] = json!(session_id);
                meta["label"] = json!(label);
                if meta.get("created").is_none() {
                    meta["created"] = json!(chrono::Local::now().to_rfc3339());
                }
                if let Err(e) = write_meta(&path, &meta) {
                    tracing::warn!(error=%e, "auto-label meta write failed");
                }
            }
        }
    }

    let opts = RespondOpts {
        source: "ui",
        provider_override: req.provider.as_deref(),
        model_override: req.model.as_deref(),
        extra_system: "",
        allow_tools: true,
    };
    match state.brain.respond(&req.prompt, &session_id, opts).await {
        Ok(r) => Json(json!({
            "ok": true,
            "text": r.text,
            "provider": r.provider,
            "model": r.model,
            "input_tokens": r.input_tokens,
            "output_tokens": r.output_tokens,
            "elapsed_ms": r.elapsed_ms,
        })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "ok": false, "error": e.to_string() }))).into_response(),
    }
}

// Sanitize a session id for use as a filename. Mirrors the rule in
// brain.rs::write_transcript so the lookup matches whatever brain wrote.
fn sanitize_session_id(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .take(80)
        .collect()
}

fn meta_path_for(state: &UiState, session_id: &str) -> std::path::PathBuf {
    state
        .cfg
        .sessions_dir
        .join(format!("{}.meta.json", sanitize_session_id(session_id)))
}

fn read_meta(path: &std::path::Path) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .unwrap_or_else(|| json!({}))
}

fn write_meta(path: &std::path::Path, v: &Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(v).unwrap_or_default())
}

#[derive(Default)]
struct ThreadAgg {
    message_count: usize,
    created: String,
    last_active: String,
    first_user: String,
}

// Iterate every jsonl session file and group lines by their session_id field
// (which preserves the original colon form). Filter to ids starting with "ui:".
fn aggregate_ui_threads(state: &UiState) -> HashMap<String, ThreadAgg> {
    let mut by_id: HashMap<String, ThreadAgg> = HashMap::new();
    let dir = &state.cfg.sessions_dir;
    let Ok(rd) = fs::read_dir(dir) else { return by_id };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&p) else { continue };
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        if lines.is_empty() {
            continue;
        }
        let Ok(first) = serde_json::from_str::<Value>(lines[0]) else { continue };
        let Some(sid) = first.get("session_id").and_then(|s| s.as_str()) else { continue };
        if !sid.starts_with("ui:") {
            continue;
        }
        let last_v: Value = serde_json::from_str(lines.last().unwrap_or(&"")).unwrap_or(Value::Null);
        let agg = by_id.entry(sid.to_string()).or_default();
        agg.message_count += lines.len();
        if let Some(ts) = first.get("ts").and_then(|s| s.as_str()) {
            if agg.created.is_empty() || ts < agg.created.as_str() {
                agg.created = ts.to_string();
                agg.first_user = first
                    .get("user")
                    .and_then(|u| u.as_str())
                    .unwrap_or("")
                    .to_string();
            }
        }
        if let Some(ts) = last_v.get("ts").and_then(|s| s.as_str()) {
            if ts > agg.last_active.as_str() {
                agg.last_active = ts.to_string();
            }
        }
    }
    by_id
}

async fn api_threads_list(State(state): State<UiState>, _: Authed) -> impl IntoResponse {
    let by_id = aggregate_ui_threads(&state);
    let mut out: Vec<Value> = Vec::with_capacity(by_id.len());
    for (sid, agg) in by_id {
        let meta = read_meta(&meta_path_for(&state, &sid));
        let label = meta
            .get("label")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| {
                let trimmed = agg.first_user.trim();
                trimmed.chars().take(60).collect()
            });
        let created = meta
            .get("created")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or(agg.created);
        out.push(json!({
            "id": sid,
            "label": label,
            "created": created,
            "last_active": agg.last_active,
            "message_count": agg.message_count,
        }));
    }
    // Empty threads (meta-only, no transcript yet) still need to show up.
    if let Ok(rd) = fs::read_dir(&state.cfg.sessions_dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if !name.ends_with(".meta.json") {
                continue;
            }
            let meta = read_meta(&p);
            let Some(sid) = meta.get("session_id").and_then(|v| v.as_str()) else { continue };
            if !sid.starts_with("ui:") {
                continue;
            }
            if out.iter().any(|t| t.get("id").and_then(|v| v.as_str()) == Some(sid)) {
                continue;
            }
            let label = meta.get("label").and_then(|v| v.as_str()).unwrap_or("new conversation").to_string();
            let created = meta.get("created").and_then(|v| v.as_str()).unwrap_or("").to_string();
            out.push(json!({
                "id": sid,
                "label": label,
                "created": created.clone(),
                "last_active": created,
                "message_count": 0,
            }));
        }
    }
    out.sort_by(|a, b| {
        let ka = a.get("last_active").and_then(|v| v.as_str()).unwrap_or("");
        let kb = b.get("last_active").and_then(|v| v.as_str()).unwrap_or("");
        kb.cmp(ka)
    });
    Json(out).into_response()
}

#[derive(Deserialize)]
struct CreateThreadReq {
    #[serde(default)]
    label: Option<String>,
}

async fn api_threads_create(
    State(state): State<UiState>,
    _: Authed,
    Json(req): Json<CreateThreadReq>,
) -> impl IntoResponse {
    let now = chrono::Local::now();
    let stamp = now.format("%Y-%m-%d-%H%M%S").to_string();
    let session_id = format!("ui:{}", stamp);
    let label = req
        .label
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "new conversation".into());
    let meta = json!({
        "session_id": session_id,
        "label": label,
        "created": now.to_rfc3339(),
    });
    if let Err(e) = write_meta(&meta_path_for(&state, &session_id), &meta) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        )
            .into_response();
    }
    Json(json!({ "session_id": session_id, "label": label })).into_response()
}

fn validate_ui_session_id(id: &str) -> Result<(), (StatusCode, Json<Value>)> {
    if !id.starts_with("ui:") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "session id must start with ui:" })),
        ));
    }
    let tail = &id[3..];
    if tail.is_empty()
        || tail
            .chars()
            .any(|c| !(c.is_alphanumeric() || c == '-' || c == '_'))
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "invalid session id characters" })),
        ));
    }
    Ok(())
}

async fn api_thread_one(
    State(state): State<UiState>,
    _: Authed,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = validate_ui_session_id(&id) {
        return e.into_response();
    }
    let mut turns: Vec<Value> = Vec::new();
    if let Ok(rd) = fs::read_dir(&state.cfg.sessions_dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&p) else { continue };
            for line in text.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
                if v.get("session_id").and_then(|s| s.as_str()) == Some(id.as_str()) {
                    turns.push(v);
                }
            }
        }
    }
    turns.sort_by(|a, b| {
        let ka = a.get("ts").and_then(|v| v.as_str()).unwrap_or("");
        let kb = b.get("ts").and_then(|v| v.as_str()).unwrap_or("");
        ka.cmp(kb)
    });
    let meta = read_meta(&meta_path_for(&state, &id));
    let label = meta.get("label").and_then(|v| v.as_str()).unwrap_or("").to_string();
    Json(json!({ "id": id, "label": label, "turns": turns })).into_response()
}

#[derive(Deserialize)]
struct LabelReq {
    label: String,
}

async fn api_thread_label(
    State(state): State<UiState>,
    _: Authed,
    Path(id): Path<String>,
    Json(req): Json<LabelReq>,
) -> impl IntoResponse {
    if let Err(e) = validate_ui_session_id(&id) {
        return e.into_response();
    }
    let label: String = req.label.trim().chars().take(120).collect();
    if label.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "label is empty" })),
        )
            .into_response();
    }
    let path = meta_path_for(&state, &id);
    let mut meta = read_meta(&path);
    if !meta.is_object() {
        meta = json!({});
    }
    meta["session_id"] = json!(id);
    meta["label"] = json!(label);
    if meta.get("created").is_none() {
        meta["created"] = json!(chrono::Local::now().to_rfc3339());
    }
    if let Err(e) = write_meta(&path, &meta) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        )
            .into_response();
    }
    Json(json!({ "ok": true, "label": label })).into_response()
}

// "Compact" the thread by counting prior turns and replacing the on-disk
// transcript with a single summary line. Real summarization happens in the
// brain in a richer implementation; this keeps the endpoint shape correct.
async fn api_thread_compact(
    State(state): State<UiState>,
    _: Authed,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(e) = validate_ui_session_id(&id) {
        return e.into_response();
    }
    let dir = &state.cfg.sessions_dir;
    let mut total_turns = 0usize;
    let mut matched: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&p) else { continue };
            let mut belongs = false;
            for line in text.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
                if v.get("session_id").and_then(|s| s.as_str()) == Some(id.as_str()) {
                    belongs = true;
                    total_turns += 1;
                }
            }
            if belongs {
                matched.push(p);
            }
        }
    }
    if total_turns == 0 {
        return Json(json!({ "ok": true, "compacted": 0, "session_id": id, "note": "no turns to compact" })).into_response();
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    for f in &matched {
        let ext = format!("compacted-{}.bak", stamp);
        let bak = f.with_extension(ext);
        if let Err(e) = fs::rename(f, &bak) {
            tracing::warn!(error=%e, path=%f.display(), "compact rename failed");
        }
    }
    let day = chrono::Local::now().date_naive().to_string();
    let safe = sanitize_session_id(&id);
    let new_path = dir.join(format!("{}__{}.jsonl", day, safe));
    let rec = json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "ts": chrono::Local::now().to_rfc3339(),
        "session_id": id,
        "source": "ui:compact",
        "user": "[compacted]",
        "assistant": format!("compacted {} turns from prior conversation", total_turns),
        "provider": "system",
        "model": "compact",
        "input_tokens": 0,
        "output_tokens": 0,
        "elapsed_ms": 0,
    });
    if let Err(e) = fs::write(&new_path, format!("{}\n", rec)) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": e.to_string() })),
        )
            .into_response();
    }
    Json(json!({ "ok": true, "compacted": total_turns, "session_id": id })).into_response()
}

fn read_tail_jsonl(path: &std::path::Path, n: usize) -> Vec<Value> {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(n);
    lines[start..]
        .iter()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect()
}

// ---------- HTML ----------
//
// The script avoids innerHTML for any dynamic data; user-supplied strings are
// always inserted via textContent or createTextNode, never via HTML parsing.
// Static page chrome is inert HTML; everything below the chrome is built with
// document.createElement.

const INDEX_HTML: &str = r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Larry</title>
<link rel="icon" href="data:image/svg+xml,<svg xmlns=%22http://www.w3.org/2000/svg%22 viewBox=%220 0 100 100%22><text y=%22.9em%22 font-size=%2290%22>🪱</text></svg>">
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>
  :root {
    --bg: #0d0e10; --fg: #d6d8dd; --muted: #8a8d94; --accent: #c8b780;
    --border: #1d1f23; --card: #15171b; --ok: #6ec07e; --err: #d96b6b;
  }
  html, body { margin: 0; padding: 0; background: var(--bg); color: var(--fg); font: 14px/1.5 ui-monospace, "JetBrains Mono", Consolas, monospace; }
  header { padding: 14px 22px; border-bottom: 1px solid var(--border); display: flex; align-items: baseline; gap: 16px; }
  header .title { font-weight: 700; color: var(--accent); }
  header .meta { color: var(--muted); font-size: 12px; }
  .row { display: grid; grid-template-columns: 360px 1fr; gap: 0; height: calc(100vh - 53px); }
  .sidebar { border-right: 1px solid var(--border); overflow-y: auto; padding: 12px; }
  .panel { padding: 16px 22px; overflow-y: auto; }
  h2 { font-size: 12px; letter-spacing: 0.06em; text-transform: uppercase; color: var(--muted); margin: 14px 0 6px; }
  h2:first-child { margin-top: 0; }
  .card { background: var(--card); border: 1px solid var(--border); border-radius: 6px; padding: 10px 12px; margin-bottom: 8px; }
  .small { font-size: 12px; color: var(--muted); }
  button { background: #1d1f23; border: 1px solid #2a2d33; color: var(--fg); padding: 4px 9px; border-radius: 4px; cursor: pointer; font: inherit; }
  button:hover { border-color: var(--accent); color: var(--accent); }
  button.run { color: var(--accent); }
  pre { background: #0a0b0c; border: 1px solid var(--border); border-radius: 4px; padding: 10px; overflow-x: auto; white-space: pre-wrap; word-break: break-word; margin: 6px 0; }
  code { color: var(--accent); }
  .ok { color: var(--ok); }
  .err { color: var(--err); }
  .nav { display: flex; gap: 8px; margin-bottom: 12px; flex-wrap: wrap; }
  .nav a { color: var(--muted); text-decoration: none; padding: 4px 9px; border: 1px solid var(--border); border-radius: 4px; cursor: pointer; }
  .nav a.on { color: var(--accent); border-color: var(--accent); }
  textarea { width: 100%; min-height: 80px; background: #0a0b0c; color: var(--fg); border: 1px solid var(--border); border-radius: 4px; padding: 8px; font: inherit; resize: vertical; }
  input, select { background: #0a0b0c; color: var(--fg); border: 1px solid var(--border); border-radius: 4px; padding: 4px 8px; font: inherit; }
  .session-card { cursor: pointer; }
  .session-card:hover { border-color: var(--accent); }
  .job-row { display: grid; grid-template-columns: 1fr auto; gap: 8px; align-items: center; }
  .thread-card.on { border-color: var(--accent); }
  .chat-panel { display: flex; flex-direction: column; height: 100%; padding: 0 !important; }
  .chat-header { padding: 10px 18px; border-bottom: 1px solid var(--border); display: flex; align-items: center; gap: 10px; }
  .chat-header input.label { flex: 1; background: transparent; border: 1px solid transparent; color: var(--fg); padding: 4px 6px; font: inherit; }
  .chat-header input.label:hover, .chat-header input.label:focus { border-color: var(--border); background: #0a0b0c; outline: none; }
  .chat-log { flex: 1; overflow-y: auto; padding: 16px 22px; display: flex; flex-direction: column; gap: 8px; }
  .bubble { padding: 8px 12px; border-radius: 8px; max-width: 78%; word-break: break-word; }
  .bubble.user { align-self: flex-end; background: #2a2d33; color: var(--fg); }
  .bubble.assistant { align-self: flex-start; background: #15171b; border: 1px solid var(--border); color: var(--fg); }
  .bubble pre { margin: 6px 0; }
  .bubble .ts { font-size: 11px; color: var(--muted); margin-top: 4px; }
  .bubble.user .ts { text-align: right; }
  .typing { color: var(--muted); font-style: italic; }
  .typing::after { content: '…'; animation: dots 1.2s steps(4, end) infinite; }
  @keyframes dots { 0%,20% { content: ''; } 40% { content: '.'; } 60% { content: '..'; } 80%,100% { content: '...'; } }
  .chat-input { display: flex; gap: 8px; padding: 10px 18px; border-top: 1px solid var(--border); }
  .chat-input textarea { flex: 1; min-height: 44px; max-height: 160px; }
  .new-thread-btn { width: 100%; margin-bottom: 10px; }
  .compact-btn { font-size: 11px; padding: 2px 6px; }
  .card.dismissable { position: relative; }
  .card .dismiss-x { position: absolute; top: 6px; right: 8px; background: transparent; border: 0; color: var(--muted); cursor: pointer; font-size: 16px; line-height: 1; padding: 0 4px; }
  .card .dismiss-x:hover { color: var(--err); }
  .card.dismissed { opacity: 0.45; text-decoration: line-through; }
  .badge { display: inline-block; padding: 1px 6px; border-radius: 3px; font-size: 11px; font-weight: 600; letter-spacing: 0.04em; margin-right: 6px; border: 1px solid var(--border); }
  .badge.sev-HIGH, .badge.sev-CRITICAL, .badge.status-OPEN { background: #2a1414; color: #d96b6b; border-color: #5a2828; }
  .badge.sev-MEDIUM { background: #2a2010; color: #d9a96b; border-color: #5a4628; }
  .badge.sev-LOW { background: #1a1c20; color: var(--muted); }
  .badge.status-CLOSED { background: #14241a; color: var(--ok); border-color: #285a3a; }
  .badge.conf-HIGH { background: #2a2410; color: var(--accent); border-color: #5a4e28; }
  .badge.conf-MEDIUM { background: #1a1c20; color: var(--muted); }
  .badge.conf-LOW { background: #15171b; color: #6a6d74; }
  .score-bar { display: flex; align-items: center; gap: 8px; margin: 6px 0; font-size: 12px; }
  .score-bar .track { flex: 1; height: 8px; background: #0a0b0c; border: 1px solid var(--border); border-radius: 4px; overflow: hidden; position: relative; }
  .score-bar .fill { height: 100%; background: var(--accent); }
  .delta-pos { color: var(--ok); font-weight: 600; }
  .delta-neg { color: var(--err); font-weight: 600; }
  .status-dot { display: inline-block; width: 8px; height: 8px; border-radius: 50%; margin-right: 4px; }
  .status-dot.active { background: var(--ok); }
  .status-dot.stale { background: var(--muted); }
  .toolbar { display: flex; gap: 8px; align-items: center; margin-bottom: 10px; flex-wrap: wrap; }
  .divider { border-top: 1px dashed var(--border); margin: 14px 0 8px; padding-top: 6px; color: var(--muted); font-size: 12px; }
</style>
</head>
<body>
<header>
  <div class="title">larry</div>
  <div class="meta" id="meta">loading...</div>
</header>
<div class="row">
  <aside class="sidebar">
    <nav class="nav">
      <a data-tab="ask" class="on">ask</a>
      <a data-tab="cron">cron</a>
      <a data-tab="sessions">sessions</a>
      <a data-tab="tools">tools</a>
      <a data-tab="bugs">bugs</a>
      <a data-tab="memory">memory</a>
      <a data-tab="research">research</a>
      <a data-tab="status">status</a>
    </nav>
    <div id="sidecontent"></div>
  </aside>
  <main class="panel" id="panel"></main>
</div>
<script>
'use strict';
const auth = localStorage.getItem('larry_token') || '';
function hdrs(extra) {
  const h = { 'content-type': 'application/json' };
  if (auth) h.authorization = 'Bearer ' + auth;
  if (extra) Object.assign(h, extra);
  return h;
}
async function fetchJ(url, opts) {
  opts = opts || {};
  opts.headers = hdrs(opts.headers);
  const r = await fetch(url, opts);
  return r.json();
}
function clear(node) { while (node.firstChild) node.removeChild(node.firstChild); }
function el(tag, attrs, kids) {
  const e = document.createElement(tag);
  if (attrs) {
    for (const k of Object.keys(attrs)) {
      const v = attrs[k];
      if (v === null || v === undefined) continue;
      if (k === 'class') e.className = v;
      else if (k === 'text') e.textContent = v;
      else if (k.startsWith('on')) e[k] = v;
      else if (k.startsWith('data-')) e.setAttribute(k, v);
      else e.setAttribute(k, v);
    }
  }
  if (kids) {
    if (!Array.isArray(kids)) kids = [kids];
    for (const k of kids) {
      if (k === null || k === undefined) continue;
      e.appendChild(typeof k === 'string' ? document.createTextNode(k) : k);
    }
  }
  return e;
}

let curTab = 'ask';
document.querySelectorAll('[data-tab]').forEach(function (a) {
  a.addEventListener('click', function (ev) {
    ev.preventDefault();
    document.querySelectorAll('[data-tab]').forEach(function (x) { x.classList.remove('on'); });
    a.classList.add('on');
    curTab = a.getAttribute('data-tab');
    render();
  });
});

async function loadStatus() {
  const s = await fetchJ('/api/status');
  document.getElementById('meta').textContent =
    'v' + s.version + ' | ' + s.providers.length + ' providers | ' + s.cron_job_count + ' crons | telegram ' +
    (s.telegram ? 'on' : 'off') + ' | tools ' + (s.tools_enabled ? 'on' : 'off');
  return s;
}

async function render() {
  const side = document.getElementById('sidecontent');
  const panel = document.getElementById('panel');
  clear(side);
  clear(panel);
  // The ask tab uses inline styles for a flex chat layout; reset between tabs
  // so other tabs get the default .panel padding/scroll back.
  panel.removeAttribute('style');
  panel.className = 'panel';
  panel.appendChild(el('div', { class: 'small', text: 'loading…' }));

  if (curTab === 'status') {
    const s = await loadStatus();
    clear(panel);
    panel.appendChild(el('h2', { text: 'status' }));
    panel.appendChild(el('pre', { text: JSON.stringify(s, null, 2) }));
    return;
  }

  if (curTab === 'ask') {
    await renderAskTab();
    return;
  }

  if (curTab === 'cron') {
    const jobs = await fetchJ('/api/cron');
    const runs = await fetchJ('/api/cron/runs');
    clear(side);
    side.appendChild(el('h2', { text: 'jobs' }));
    jobs.forEach(function (j) {
      const card = el('div', { class: 'card' });
      const row = el('div', { class: 'job-row' });
      const left = el('div');
      const ln1 = el('div');
      ln1.appendChild(el('code', { text: j.id }));
      ln1.appendChild(document.createTextNode(' '));
      ln1.appendChild(el('span', { class: 'small', text: j.kind }));
      left.appendChild(ln1);
      left.appendChild(el('div', { class: 'small', text: j.schedule + ' ' + j.tz }));
      left.appendChild(el('div', { class: 'small', text: j.name }));
      row.appendChild(left);
      const right = el('div');
      const btn = el('button', { class: 'run', text: 'run' });
      btn.onclick = async function () {
        btn.textContent = '…';
        const r = await fetchJ('/api/cron/run', { method: 'POST', body: JSON.stringify({ job_id: j.id }) });
        btn.textContent = r.ok ? '✓' : 'err';
        setTimeout(function () { btn.textContent = 'run'; }, 2000);
      };
      right.appendChild(btn);
      row.appendChild(right);
      card.appendChild(row);
      side.appendChild(card);
    });
    clear(panel);
    panel.appendChild(el('h2', { text: 'last runs' }));
    if (!runs.length) {
      panel.appendChild(el('div', { class: 'small', text: 'no runs yet' }));
    } else {
      runs.slice().reverse().forEach(function (r) {
        const card = el('div', { class: 'card' });
        const head = el('div');
        head.appendChild(el('code', { text: r.job_id || '' }));
        head.appendChild(document.createTextNode(' '));
        head.appendChild(el('span', { class: 'small', text: r.ts || '' }));
        head.appendChild(document.createTextNode(' '));
        head.appendChild(el('span', { class: r.ok ? 'ok' : 'err', text: r.ok ? 'ok' : 'err' }));
        head.appendChild(document.createTextNode(' ' + (r.elapsed_ms || 0) + 'ms'));
        card.appendChild(head);
        if (r.error) card.appendChild(el('div', { class: 'err small', text: r.error }));
        if (r.output_preview) card.appendChild(el('pre', { class: 'small', text: r.output_preview }));
        panel.appendChild(card);
      });
    }
    return;
  }

  if (curTab === 'sessions') {
    const list = await fetchJ('/api/sessions');
    clear(side);
    side.appendChild(el('h2', { text: 'sessions' }));
    list.forEach(function (s) {
      const card = el('div', { class: 'card session-card' });
      card.appendChild(el('div', { text: s.name }));
      card.appendChild(el('div', { class: 'small', text: (s.size_bytes/1024).toFixed(1) + ' KB · ' + s.modified }));
      card.onclick = async function () {
        const lines = await fetchJ('/api/sessions/' + encodeURIComponent(s.name));
        clear(panel);
        panel.appendChild(el('h2', { text: s.name }));
        lines.slice().reverse().forEach(function (t) {
          const c = el('div', { class: 'card' });
          c.appendChild(el('div', { class: 'small', text: (t.ts||'') + ' · ' + (t.provider||'') + '/' + (t.model||'') + ' · src=' + (t.source||'') }));
          const u = el('div'); u.appendChild(el('b', { text: 'user: ' })); u.appendChild(el('pre', { text: t.user || '' })); c.appendChild(u);
          const a = el('div'); a.appendChild(el('b', { text: 'assistant: ' })); a.appendChild(el('pre', { text: t.assistant || '' })); c.appendChild(a);
          panel.appendChild(c);
        });
      };
      side.appendChild(card);
    });
    clear(panel);
    panel.appendChild(el('div', { class: 'small', text: 'pick a session on the left' }));
    return;
  }

  if (curTab === 'bugs') {
    await renderBugsTab();
    return;
  }

  if (curTab === 'memory') {
    await renderMemoryTab();
    return;
  }

  if (curTab === 'research') {
    await renderResearchTab();
    return;
  }

  if (curTab === 'tools') {
    const list = await fetchJ('/api/tools');
    clear(panel);
    panel.appendChild(el('h2', { text: 'tool calls (last 100)' }));
    if (!list.length) {
      panel.appendChild(el('div', { class: 'small', text: 'no tool calls yet' }));
      return;
    }
    list.slice().reverse().forEach(function (t) {
      const card = el('div', { class: 'card' });
      const head = el('div');
      head.appendChild(el('code', { text: t.tool || '' }));
      head.appendChild(document.createTextNode(' '));
      head.appendChild(el('span', { class: 'small', text: t.ts || '' }));
      head.appendChild(document.createTextNode(' '));
      head.appendChild(el('span', { class: t.ok ? 'ok' : 'err', text: t.ok ? 'ok' : 'err' }));
      head.appendChild(document.createTextNode(' ' + (t.elapsed_ms || 0) + 'ms'));
      card.appendChild(head);
      card.appendChild(el('pre', { class: 'small', text: 'input: ' + JSON.stringify(t.input || {}) }));
      if (t.output_preview) card.appendChild(el('pre', { class: 'small', text: 'out: ' + t.output_preview }));
      panel.appendChild(card);
    });
    return;
  }
}

// ---------- ask tab (threaded chat) ----------
const askState = { threadId: null, label: '', threads: [], turns: [], busy: false };

function shortTs(s) {
  if (!s) return '';
  return String(s).replace('T', ' ').slice(0, 16);
}

// Light formatting: split on triple-backticks for code blocks, preserve
// newlines elsewhere. All user-supplied strings still go through textContent.
function renderText(s) {
  const wrap = el('div', { style: 'white-space:pre-wrap;word-break:break-word' });
  const text = String(s == null ? '' : s);
  const parts = text.split('```');
  parts.forEach(function (part, i) {
    if (i % 2 === 1) {
      const stripped = part.replace(/^[A-Za-z0-9_+\-]*\n/, '');
      wrap.appendChild(el('pre', { class: 'small', text: stripped }));
    } else if (part.length) {
      wrap.appendChild(document.createTextNode(part));
    }
  });
  return wrap;
}

function appendUserBubble(log, t) {
  const b = el('div', { class: 'bubble user' });
  b.appendChild(renderText(t.user || ''));
  if (t.ts) b.appendChild(el('div', { class: 'ts', text: shortTs(t.ts) }));
  log.appendChild(b);
}

function appendAssistantBubble(log, t) {
  const b = el('div', { class: 'bubble assistant' });
  b.appendChild(renderText(t.assistant || ''));
  const provModel = (t.provider || t.model) ? ((t.provider || '') + '/' + (t.model || '')) : '';
  const tail = [shortTs(t.ts || ''), provModel].filter(Boolean).join(' • ');
  if (tail) b.appendChild(el('div', { class: 'ts', text: tail }));
  log.appendChild(b);
}

async function refreshThreadList() {
  askState.threads = await fetchJ('/api/threads');
  renderThreadList();
}

function renderThreadList() {
  const side = document.getElementById('sidecontent');
  if (!side) return;
  clear(side);
  const newBtn = el('button', { class: 'new-thread-btn', text: '+ new conversation' });
  newBtn.onclick = async function () {
    const r = await fetchJ('/api/threads', { method: 'POST', body: JSON.stringify({}) });
    if (r && r.session_id) {
      askState.threadId = r.session_id;
      askState.label = r.label || '';
      askState.turns = [];
      await refreshThreadList();
      renderChatPanel();
    }
  };
  side.appendChild(newBtn);
  side.appendChild(el('h2', { text: 'conversations' }));
  if (!askState.threads.length) {
    side.appendChild(el('div', { class: 'small', text: 'no conversations yet' }));
    return;
  }
  askState.threads.forEach(function (t) {
    const card = el('div', { class: 'card thread-card session-card' });
    if (t.id === askState.threadId) card.classList.add('on');
    card.appendChild(el('div', { text: t.label || '(untitled)' }));
    const meta = el('div', { class: 'small' });
    meta.textContent = shortTs(t.last_active || t.created || '') + ' · ' + (t.message_count || 0) + ' msg';
    card.appendChild(meta);
    const row = el('div', { style: 'margin-top:6px;display:flex;gap:6px' });
    const compactBtn = el('button', { class: 'compact-btn', text: 'compact' });
    compactBtn.onclick = async function (ev) {
      ev.stopPropagation();
      compactBtn.textContent = '…';
      const r = await fetchJ('/api/threads/' + encodeURIComponent(t.id) + '/compact', { method: 'POST' });
      compactBtn.textContent = r && r.ok ? 'ok' : 'err';
      setTimeout(function () { compactBtn.textContent = 'compact'; }, 1500);
      if (askState.threadId === t.id) await loadThread(t.id);
      await refreshThreadList();
    };
    row.appendChild(compactBtn);
    card.appendChild(row);
    card.onclick = async function () {
      askState.threadId = t.id;
      await loadThread(t.id);
      renderThreadList();
    };
    side.appendChild(card);
  });
}

async function loadThread(id) {
  const r = await fetchJ('/api/threads/' + encodeURIComponent(id));
  askState.threadId = id;
  askState.label = (r && r.label) || '';
  askState.turns = (r && r.turns) || [];
  renderChatPanel();
}

function renderChatPanel() {
  const panel = document.getElementById('panel');
  if (!panel || curTab !== 'ask') return;
  clear(panel);
  panel.className = 'panel chat-panel';

  if (!askState.threadId) {
    panel.removeAttribute('style');
    panel.className = 'panel';
    panel.appendChild(el('div', { class: 'small', style: 'padding:18px', text: 'pick a conversation on the left, or start a new one.' }));
    return;
  }

  const header = el('div', { class: 'chat-header' });
  const labelInput = el('input', { class: 'label', value: askState.label || '' });
  labelInput.title = 'click to rename';
  labelInput.addEventListener('change', async function () {
    const next = labelInput.value.trim();
    if (!next) { labelInput.value = askState.label; return; }
    const r = await fetchJ('/api/threads/' + encodeURIComponent(askState.threadId) + '/label', {
      method: 'POST',
      body: JSON.stringify({ label: next }),
    });
    if (r && r.ok) {
      askState.label = r.label || next;
      await refreshThreadList();
    } else {
      labelInput.value = askState.label;
    }
  });
  header.appendChild(labelInput);
  header.appendChild(el('span', { class: 'small', text: askState.threadId }));
  panel.appendChild(header);

  const log = el('div', { class: 'chat-log', id: 'chatlog' });
  panel.appendChild(log);

  askState.turns.forEach(function (t) {
    if (t.user) appendUserBubble(log, t);
    if (t.assistant) appendAssistantBubble(log, t);
  });

  const inputRow = el('div', { class: 'chat-input' });
  const ta = el('textarea', { id: 'chatq', placeholder: 'message larry (ctrl+enter to send)' });
  const sendBtn = el('button', { id: 'chatsend', text: 'send' });
  ta.addEventListener('keydown', function (ev) {
    if (ev.key === 'Enter' && (ev.ctrlKey || ev.metaKey)) {
      ev.preventDefault();
      sendBtn.click();
    }
  });
  sendBtn.onclick = async function () {
    if (askState.busy) return;
    const prompt = ta.value.trim();
    if (!prompt) return;
    ta.value = '';
    askState.busy = true;
    sendBtn.disabled = true;
    const nowIso = new Date().toISOString();
    appendUserBubble(log, { user: prompt, ts: nowIso });
    const thinking = el('div', { class: 'bubble assistant' });
    thinking.appendChild(el('span', { class: 'typing', text: 'larry is thinking' }));
    log.appendChild(thinking);
    log.scrollTop = log.scrollHeight;
    try {
      const r = await fetchJ('/api/ask', {
        method: 'POST',
        body: JSON.stringify({ prompt: prompt, session_id: askState.threadId }),
      });
      if (thinking.parentNode) thinking.parentNode.removeChild(thinking);
      if (r && r.ok) {
        const turn = {
          user: prompt,
          assistant: r.text,
          ts: new Date().toISOString(),
          provider: r.provider,
          model: r.model,
        };
        askState.turns.push(turn);
        appendAssistantBubble(log, turn);
      } else {
        const errBubble = el('div', { class: 'bubble assistant' });
        errBubble.appendChild(el('div', { class: 'err', text: (r && r.error) || 'error' }));
        log.appendChild(errBubble);
      }
    } catch (e) {
      if (thinking.parentNode) thinking.parentNode.removeChild(thinking);
      const errBubble = el('div', { class: 'bubble assistant' });
      errBubble.appendChild(el('div', { class: 'err', text: 'fetch failed: ' + e }));
      log.appendChild(errBubble);
    }
    askState.busy = false;
    sendBtn.disabled = false;
    log.scrollTop = log.scrollHeight;
    refreshThreadList();
  };
  inputRow.appendChild(ta);
  inputRow.appendChild(sendBtn);
  panel.appendChild(inputRow);

  // Defer scroll so layout has settled.
  setTimeout(function () { log.scrollTop = log.scrollHeight; }, 0);
}

async function renderAskTab() {
  await refreshThreadList();
  if (askState.threadId) {
    // Re-load to pick up any new turns from other tabs / cron / compaction.
    await loadThread(askState.threadId);
  } else if (askState.threads.length) {
    askState.threadId = askState.threads[0].id;
    await loadThread(askState.threadId);
    renderThreadList();
  } else {
    renderChatPanel();
  }
}

// ---------- dismiss system (shared by bugs + memory) ----------
const dismissState = { showDismissed: {} };

function loadDismissed() {
  try {
    const raw = localStorage.getItem('larry_dismissed') || '[]';
    const arr = JSON.parse(raw);
    return new Set(Array.isArray(arr) ? arr : []);
  } catch (e) { return new Set(); }
}
function saveDismissed(set) {
  localStorage.setItem('larry_dismissed', JSON.stringify(Array.from(set)));
}
function dismissAdd(id) {
  const s = loadDismissed(); s.add(id); saveDismissed(s);
}
function dismissRemove(id) {
  const s = loadDismissed(); s.delete(id); saveDismissed(s);
}
function isDismissed(id) {
  return loadDismissed().has(id);
}
// Stable hash of first 50 chars — used as the dismiss key for memory items,
// which don't have stable ids of their own.
function memHash(text) {
  const s = String(text || '').slice(0, 50);
  let h = 0;
  for (let i = 0; i < s.length; i++) {
    h = ((h << 5) - h + s.charCodeAt(i)) | 0;
  }
  return 'mem-' + (h >>> 0).toString(16);
}
function attachDismissX(card, dismissId, onChange) {
  card.classList.add('dismissable');
  card.setAttribute('data-id', dismissId);
  const x = el('button', { class: 'dismiss-x', text: '×', title: 'dismiss' });
  x.onclick = function (ev) {
    ev.stopPropagation();
    if (isDismissed(dismissId)) dismissRemove(dismissId);
    else dismissAdd(dismissId);
    if (onChange) onChange();
  };
  card.appendChild(x);
}
function dismissToolbar(tab, onChange) {
  const bar = el('div', { class: 'toolbar' });
  const showing = !!dismissState.showDismissed[tab];
  const btn = el('button', { text: showing ? 'hide dismissed' : 'show dismissed' });
  btn.onclick = function () {
    dismissState.showDismissed[tab] = !showing;
    onChange();
  };
  bar.appendChild(btn);
  return bar;
}

// ---------- bugs tab ----------
async function renderBugsTab() {
  const side = document.getElementById('sidecontent');
  const panel = document.getElementById('panel');
  clear(side);
  clear(panel);
  panel.removeAttribute('style');
  panel.className = 'panel';
  const data = await fetchJ('/api/bugs');
  const bugs = (data && data.bugs) || [];
  const open = bugs.filter(function (b) { return b.status !== 'CLOSED'; });
  const closed = bugs.filter(function (b) { return b.status === 'CLOSED'; });

  side.appendChild(el('h2', { text: 'bugs' }));
  side.appendChild(el('div', { class: 'small', text: open.length + ' open · ' + closed.length + ' closed' }));
  if (data && data.file_mtime) {
    side.appendChild(el('div', { class: 'small', text: 'updated ' + shortTs(data.file_mtime) }));
  }

  panel.appendChild(el('h2', { text: open.length + ' open bugs' }));
  panel.appendChild(dismissToolbar('bugs', renderBugsTab));

  function renderBug(b) {
    const dismissId = 'bug-' + b.id;
    const dismissed = isDismissed(dismissId);
    if (dismissed && !dismissState.showDismissed.bugs) return null;
    const card = el('div', { class: 'card' });
    if (dismissed) card.classList.add('dismissed');
    const head = el('div');
    head.appendChild(el('code', { text: b.id }));
    head.appendChild(document.createTextNode(' '));
    head.appendChild(el('span', { text: b.title || '' }));
    card.appendChild(head);
    const badges = el('div', { style: 'margin-top:4px' });
    if (b.severity) badges.appendChild(el('span', { class: 'badge sev-' + b.severity, text: b.severity }));
    if (b.status) badges.appendChild(el('span', { class: 'badge status-' + (b.status === 'CLOSED' ? 'CLOSED' : 'OPEN'), text: b.status }));
    card.appendChild(badges);
    if (b.description) card.appendChild(el('div', { class: 'small', style: 'margin-top:6px', text: b.description }));
    attachDismissX(card, dismissId, renderBugsTab);
    return card;
  }

  open.forEach(function (b) { const c = renderBug(b); if (c) panel.appendChild(c); });
  if (closed.length) {
    panel.appendChild(el('div', { class: 'divider', text: 'closed' }));
    closed.forEach(function (b) { const c = renderBug(b); if (c) panel.appendChild(c); });
  }
  if (!open.length && !closed.length) {
    panel.appendChild(el('div', { class: 'small', text: 'no bugs tracked yet' }));
  }
}

// ---------- memory tab ----------
async function renderMemoryTab() {
  const side = document.getElementById('sidecontent');
  const panel = document.getElementById('panel');
  clear(side);
  clear(panel);
  panel.removeAttribute('style');
  panel.className = 'panel';
  const data = await fetchJ('/api/memory/proposed');
  const items = (data && data.items) || [];

  side.appendChild(el('h2', { text: 'memory' }));
  side.appendChild(el('div', { class: 'small', text: (data && data.count) + ' proposals' }));
  if (data && data.file_mtime) {
    side.appendChild(el('div', { class: 'small', text: 'updated ' + shortTs(data.file_mtime) }));
  }
  const promote = el('button', { text: 'promote all', style: 'margin-top:10px;width:100%' });
  promote.onclick = async function () {
    const r = await fetchJ('/api/memory/promote', { method: 'POST', body: JSON.stringify({}) });
    alert((r && r.message) || 'use Telegram: reply promote all');
  };
  side.appendChild(promote);

  panel.appendChild(el('h2', { text: items.length + ' proposals pending' }));
  if (data && data.file_mtime) {
    panel.appendChild(el('div', { class: 'small', text: 'last modified: ' + shortTs(data.file_mtime) }));
  }
  panel.appendChild(dismissToolbar('memory', renderMemoryTab));

  if (!items.length) {
    panel.appendChild(el('div', { class: 'small', text: 'no proposals yet — memory-dreaming cron writes here' }));
    return;
  }

  items.forEach(function (it) {
    const dismissId = memHash(it.text);
    const dismissed = isDismissed(dismissId);
    if (dismissed && !dismissState.showDismissed.memory) return;
    const card = el('div', { class: 'card' });
    if (dismissed) card.classList.add('dismissed');
    const conf = it.confidence || 'LOW';
    card.appendChild(el('span', { class: 'badge conf-' + conf, text: conf }));
    const full = String(it.text || '');
    const truncated = full.length > 200 ? full.slice(0, 200) + '…' : full;
    const body = el('span', { text: truncated });
    if (full.length > 200) body.title = full;
    card.appendChild(body);
    attachDismissX(card, dismissId, renderMemoryTab);
    panel.appendChild(card);
  });
}

// ---------- research tab ----------
async function renderResearchTab() {
  const side = document.getElementById('sidecontent');
  const panel = document.getElementById('panel');
  clear(side);
  clear(panel);
  panel.removeAttribute('style');
  panel.className = 'panel';
  const list = await fetchJ('/api/research');
  const lastTs = list.reduce(function (a, b) {
    return (b.last_ts && b.last_ts > a) ? b.last_ts : a;
  }, '');

  side.appendChild(el('h2', { text: 'research' }));
  side.appendChild(el('div', { class: 'small', text: list.length + ' harnesses' }));
  if (lastTs) side.appendChild(el('div', { class: 'small', text: 'last ' + shortTs(lastTs) }));

  panel.appendChild(el('h2', { text: list.length + ' harnesses · last run ' + (shortTs(lastTs) || '—') }));
  if (!list.length) {
    panel.appendChild(el('div', { class: 'small', text: 'no harnesses with decisions.jsonl yet' }));
    return;
  }

  list.forEach(function (h) {
    const card = el('div', { class: 'card' });
    const head = el('div');
    head.appendChild(el('span', { class: 'status-dot ' + (h.status || 'stale') }));
    head.appendChild(el('code', { text: h.name }));
    head.appendChild(document.createTextNode(' '));
    head.appendChild(el('span', { class: 'small', text: h.status || '' }));
    card.appendChild(head);

    const baseline = Number(h.baseline || 0);
    const current = Number(h.current || 0);
    const delta = Number(h.delta || 0);
    const max = Math.max(baseline, current, 1);
    const bar = el('div', { class: 'score-bar' });
    bar.appendChild(el('span', { class: 'small', text: baseline.toFixed(1) }));
    const track = el('div', { class: 'track' });
    const fill = el('div', { class: 'fill' });
    fill.style.width = (current / max * 100).toFixed(1) + '%';
    track.appendChild(fill);
    bar.appendChild(track);
    bar.appendChild(el('span', { text: current.toFixed(1) }));
    const sign = delta >= 0 ? '+' : '';
    bar.appendChild(el('span', {
      class: delta >= 0 ? 'delta-pos' : 'delta-neg',
      text: sign + delta.toFixed(1),
    }));
    card.appendChild(bar);

    card.appendChild(el('div', { class: 'small', text: 'kept ' + (h.kept || 0) + ' · reverted ' + (h.reverted || 0) + ' · last ' + shortTs(h.last_ts || '') }));
    panel.appendChild(card);
  });
}

loadStatus().then(render);
setInterval(loadStatus, 5000);
</script>
</body>
</html>"##;
