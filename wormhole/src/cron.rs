//! Cron jobs: TOML-defined scheduled prompts and shell tasks.

use crate::brain::{Brain, RespondOpts};
use crate::config::Config;
use crate::dreaming::DreamingScheduler;
use crate::error::{LarryError, Result};
use crate::subagent::{run_claude, run_codex, run_shell, summarise};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio_cron_scheduler::{Job as CronJob, JobScheduler};

#[derive(Debug, Clone, Deserialize)]
pub struct Job {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub schedule: String,
    // NOTE: `tz` was previously declared here but silently ignored by the
    // scheduler, which Codex review #cron26 (correctly) called out as a fake
    // guarantee. Removed. Existing TOML entries with `tz = "..."` are tolerated
    // — Serde ignores unknown fields by default — but they have no effect.
    // **Schedules run in the daemon's host local time.** Re-add a real `tz`
    // field only when the scheduler is actually timezone-aware (e.g. via
    // tokio-cron-scheduler's `JobBuilder::with_timezone`).
    #[serde(default = "default_kind")]
    pub kind: String, // prompt | shell | claude | codex
    #[serde(default = "default_shell")]
    pub shell: String,
    #[serde(default)]
    pub body: String,
    #[serde(default = "default_deliver")]
    pub deliver: String,
    #[serde(default)]
    pub deliver_chat_id: Option<i64>,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_timeout")]
    pub timeout_s: u64,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    /// "background" (default) routes prompt jobs to the cheap/fast model
    /// (configurable via [brain] background_model). Set to "foreground" to
    /// keep using the brain's default_model. Has no effect if `model` is
    /// already explicitly set on the job.
    #[serde(default = "default_tier")]
    pub tier: String,
    /// Working directory for `shell` / `claude` / `codex` jobs. When set, the
    /// shell job is run with `cwd` set to this path, and subagents see it as
    /// their `--cwd`. Ignored for `prompt` and `dream` jobs (which have no
    /// filesystem context). Lets a single cron config drive jobs across
    /// multiple project trees without the body string having to `cd` first.
    #[serde(default)]
    pub workdir: Option<String>,
    /// ID of another job whose last successful output should be prepended to
    /// this job's prompt body. Lets you chain cron jobs — e.g. job A scrapes
    /// data, job B summarises A's output. Truncated to ~2000 chars to keep
    /// the prompt size sane.
    #[serde(default)]
    pub context_from: Option<String>,
    /// Per-job override for the brain's tool-loop iteration cap. When set,
    /// replaces the global `[tools].max_iterations` for this job's invocation
    /// only. Auto-retry on cap (50% bump, hard ceiling 60) still applies for
    /// cron-triggered runs. Codex review #cron-iter.
    #[serde(default)]
    pub max_iterations: Option<u32>,
}

fn default_true() -> bool { true }
fn default_kind() -> String { "prompt".into() }
fn default_shell() -> String { "pwsh".into() }
fn default_deliver() -> String { "telegram".into() }
fn default_timeout() -> u64 { 600 }
fn default_tier() -> String { "background".into() }

#[derive(Deserialize)]
struct CronFile {
    #[serde(default)]
    job: Vec<Job>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct JobState {
    pub last_run_ts: Option<String>,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub last_duration_ms: u64,
    pub consecutive_errors: u64,
    /// Last successful output (truncated to ~2000 chars). Used by
    /// `context_from` to chain jobs together. `#[serde(default)]` lets older
    /// cron-state.json files load without `last_output`.
    #[serde(default)]
    pub last_output: Option<String>,
}

const LAST_OUTPUT_CAP: usize = 2000;

pub fn load_jobs(path: &Path) -> Result<Vec<Job>> {
    let mut out: Vec<Job> = Vec::new();
    if path.exists() {
        let text = fs::read_to_string(path)?;
        let parsed: CronFile = toml::from_str(&text)
            .map_err(|e| LarryError::Config(format!("cron toml ({}): {e}", path.display())))?;
        out.extend(parsed.job);
    }
    // Also load every *.toml in <larry_home>/cron.d/ so widgets / ad-hoc jobs
    // can be added without touching the master cron.toml.
    let cron_d = path.with_file_name("cron.d");
    if cron_d.is_dir() {
        if let Ok(rd) = fs::read_dir(&cron_d) {
            for entry in rd.flatten() {
                let p = entry.path();
                if p.extension().and_then(|s| s.to_str()) != Some("toml") {
                    continue;
                }
                let text = match fs::read_to_string(&p) {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::warn!(file=%p.display(), error=%e, "cron.d read failed");
                        continue;
                    }
                };
                match toml::from_str::<CronFile>(&text) {
                    Ok(parsed) => out.extend(parsed.job),
                    Err(e) => tracing::warn!(file=%p.display(), error=%e, "cron.d parse failed"),
                }
            }
        }
    }
    Ok(out)
}

#[derive(Clone)]
pub struct CronContext {
    pub cfg: Arc<Config>,
    pub brain: Arc<Brain>,
    pub deliver: Option<DeliverFn>,
    pub state: Arc<Mutex<HashMap<String, JobState>>>,
    pub state_path: PathBuf,
    pub runs_log: PathBuf,
}

pub type DeliverFn = Arc<
    dyn Fn(i64, String) -> futures::future::BoxFuture<'static, ()> + Send + Sync,
>;

pub struct CronRunner {
    pub ctx: CronContext,
    scheduler: JobScheduler,
    /// id -> (Job, scheduler UUID). UUID is needed to remove jobs on reload.
    jobs: Mutex<HashMap<String, (Job, uuid::Uuid)>>,
    cron_path: PathBuf,
}

impl CronRunner {
    pub async fn new(
        cfg: Arc<Config>,
        brain: Arc<Brain>,
        deliver: Option<DeliverFn>,
        cron_path: Option<PathBuf>,
    ) -> Result<Self> {
        let cron_path = cron_path.unwrap_or_else(|| cfg.larry_home.join("cron.toml"));
        let state_path = cfg.larry_home.join("cron-state.json");
        let runs_log = cfg.logs_dir.join("cron-runs.jsonl");
        let state = load_state(&state_path).unwrap_or_default();
        let scheduler = JobScheduler::new()
            .await
            .map_err(|e| LarryError::Config(format!("scheduler init: {e}")))?;
        Ok(Self {
            ctx: CronContext {
                cfg,
                brain,
                deliver,
                state: Arc::new(Mutex::new(state)),
                state_path,
                runs_log,
            },
            scheduler,
            jobs: Mutex::new(HashMap::new()),
            cron_path,
        })
    }

    pub async fn start(&self) -> Result<()> {
        self.reload_jobs().await?;
        self.scheduler
            .start()
            .await
            .map_err(|e| LarryError::Config(format!("scheduler start: {e}")))?;
        let count = self.jobs.lock().await.len();
        tracing::info!(jobs = count, "cron started");
        Ok(())
    }

    pub async fn list_jobs(&self) -> Vec<Job> {
        self.jobs.lock().await.values().map(|(j, _)| j.clone()).collect()
    }

    pub async fn trigger(&self, job_id: &str) -> Result<()> {
        let job = {
            let map = self.jobs.lock().await;
            map.get(job_id).map(|(j, _)| j.clone())
        };
        let job = job.ok_or_else(|| LarryError::Permanent(format!("no job {job_id}")))?;
        let ctx = self.ctx.clone();
        tokio::spawn(async move {
            run_one(ctx, job).await;
        });
        Ok(())
    }

    /// Re-read cron.toml + cron.d/, drop jobs that disappeared, schedule new ones.
    /// Returns (added, removed) counts.
    pub async fn reload_jobs(&self) -> Result<(usize, usize)> {
        let new_jobs = load_jobs(&self.cron_path)?;
        let new_ids: std::collections::HashSet<String> = new_jobs
            .iter()
            .filter(|j| j.enabled && !j.schedule.trim().is_empty())
            .map(|j| j.id.clone())
            .collect();

        let mut map = self.jobs.lock().await;

        // Remove jobs that disappeared or got disabled.
        let to_remove: Vec<(String, uuid::Uuid)> = map
            .iter()
            .filter(|(id, _)| !new_ids.contains(*id))
            .map(|(id, (_, u))| (id.clone(), *u))
            .collect();
        let removed = to_remove.len();
        for (id, u) in to_remove {
            let _ = self.scheduler.remove(&u).await;
            map.remove(&id);
            tracing::info!(id=%id, "removed from scheduler");
        }

        // Add (or re-add to refresh) every enabled job.
        let mut added = 0usize;
        for j in new_jobs {
            if !j.enabled || j.schedule.trim().is_empty() {
                continue;
            }
            // Replace existing same-id schedule: drop the old UUID first.
            if let Some((_, old_uuid)) = map.get(&j.id) {
                let _ = self.scheduler.remove(old_uuid).await;
            }
            let cron_expr = match to_cron_expr(&j.schedule) {
                Ok(e) => e,
                Err(e) => {
                    tracing::error!(job=%j.id, schedule=%j.schedule, error=%e, "bad schedule, skipping");
                    continue;
                }
            };
            let job_clone = j.clone();
            let ctx_clone = self.ctx.clone();
            let cron_job = match CronJob::new_async(cron_expr.as_str(), move |_uuid, _l| {
                let job = job_clone.clone();
                let ctx = ctx_clone.clone();
                Box::pin(async move {
                    run_one(ctx, job).await;
                })
            }) {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!(job=%j.id, error=%e, "build CronJob failed");
                    continue;
                }
            };
            let new_uuid = match self.scheduler.add(cron_job).await {
                Ok(u) => u,
                Err(e) => {
                    tracing::error!(job=%j.id, error=%e, "scheduler.add failed");
                    continue;
                }
            };
            tracing::info!(id=%j.id, schedule=%j.schedule, kind=%j.kind, "scheduled");
            map.insert(j.id.clone(), (j, new_uuid));
            added += 1;
        }
        Ok((added, removed))
    }
}

/// Translate the user-facing schedule string into a 6-field cron expression
/// understood by tokio-cron-scheduler (sec min hour day month weekday).
///
/// Accepted inputs:
///   * 5-field cron ("*/30 8-23 * * *") — we prepend "0" for seconds.
///   * "every Ns" / "every Nm" / "every Nh" — converted to interval-style cron.
///   * "@daily", "@hourly", "@nightly", "@weekly" — convenience aliases.
fn to_cron_expr(schedule: &str) -> Result<String> {
    let s = schedule.trim();
    if let Some(rest) = s.strip_prefix("every ") {
        let r = rest.trim().to_ascii_lowercase();
        if let Some(num_str) = r.strip_suffix('s') {
            let n: u64 = num_str.parse().map_err(|e| LarryError::Config(format!("bad seconds: {e}")))?;
            return Ok(format!("*/{n} * * * * *"));
        }
        if let Some(num_str) = r.strip_suffix('m') {
            let n: u64 = num_str.parse().map_err(|e| LarryError::Config(format!("bad minutes: {e}")))?;
            return Ok(format!("0 */{n} * * * *"));
        }
        if let Some(num_str) = r.strip_suffix('h') {
            let n: u64 = num_str.parse().map_err(|e| LarryError::Config(format!("bad hours: {e}")))?;
            return Ok(format!("0 0 */{n} * * *"));
        }
        let n: u64 = r.parse().map_err(|e| LarryError::Config(format!("bad interval: {e}")))?;
        return Ok(format!("*/{n} * * * * *"));
    }
    let s = match s {
        "@hourly" => "0 0 * * * *",
        "@daily" | "@nightly" => "0 0 0 * * *",
        "@weekly" => "0 0 0 * * 0",
        other => other,
    };
    let parts: Vec<&str> = s.split_whitespace().collect();
    match parts.len() {
        5 => Ok(format!("0 {}", s)),  // prepend seconds
        6 => Ok(s.to_string()),
        _ => Err(LarryError::Config(format!("schedule needs 5 or 6 fields: {s}"))),
    }
}

async fn run_one(ctx: CronContext, job: Job) {
    // Deterministic per-job jitter (0-29s). Two jobs scheduled at "0 * * * *"
    // would otherwise both fire at :00 and contend for the brain / shell /
    // Anthropic connection pool. Jitter spreads them across the minute.
    let jitter_s = job.id.bytes().fold(0u64, |a, b| a.wrapping_add(b as u64)) % 30;
    if jitter_s > 0 {
        tokio::time::sleep(std::time::Duration::from_secs(jitter_s)).await;
    }

    let t0 = Instant::now();
    tracing::info!(id=%job.id, name=%job.name, kind=%job.kind, tier=%job.tier, jitter_s, "cron start");
    let mut ok = false;
    let mut err: Option<String> = None;
    let mut output_for_delivery = String::new();

    // Resolve effective model: explicit job.model wins; otherwise tier=background
    // picks the configured background model, tier=foreground falls through to
    // the brain default (model_override = None).
    let resolved_model: Option<String> = job.model.clone().or_else(|| {
        if job.tier == "background" {
            ctx.cfg.background_model.clone()
        } else {
            None
        }
    });

    // If `context_from` is set, look up that job's last output and prepend it
    // to this job's body. Empty / missing source output falls through silently
    // — chained jobs shouldn't fail just because the parent hasn't run yet.
    let body_with_context: String = if let Some(src_id) = job.context_from.as_deref() {
        let prior = {
            let map = ctx.state.lock().await;
            map.get(src_id).and_then(|s| s.last_output.clone())
        };
        match prior {
            Some(prev) if !prev.trim().is_empty() => {
                format!("[Output from {src_id}]:\n{prev}\n\n{body}", body = job.body)
            }
            _ => job.body.clone(),
        }
    } else {
        job.body.clone()
    };

    match job.kind.as_str() {
        "prompt" => {
            let session = format!("cron:{}", job.id);
            let source = format!("cron:{}", job.id);
            match ctx
                .brain
                .respond(
                    &body_with_context,
                    &session,
                    RespondOpts {
                        source: &source,
                        provider_override: job.provider.as_deref(),
                        model_override: resolved_model.as_deref(),
                        extra_system: "",
                        allow_tools: true,
                        max_iterations_override: job.max_iterations,
                        auto_retry_on_cap: true,
                    },
                )
                .await
            {
                Ok(r) => {
                    output_for_delivery = r.text;
                    ok = true;
                }
                Err(e) => {
                    err = Some(e.to_string());
                }
            }
        }
        "claude" => match run_claude(
            &ctx.cfg,
            &body_with_context,
            job.workdir.as_deref(),
            &ctx.cfg.cron_delegation_permission_mode,
            job.timeout_s,
        )
        .await
        {
            Ok(res) => {
                output_for_delivery = summarise(&res);
                ok = res.returncode == 0 && !res.timed_out;
                if !ok {
                    err = Some(format!("claude rc={} timed_out={}", res.returncode, res.timed_out));
                }
            }
            Err(e) => err = Some(e.to_string()),
        },
        "codex" => match run_codex(&ctx.cfg, &body_with_context, job.workdir.as_deref(), job.timeout_s).await {
            Ok(res) => {
                output_for_delivery = summarise(&res);
                ok = res.returncode == 0 && !res.timed_out;
                if !ok {
                    err = Some(format!("codex rc={} timed_out={}", res.returncode, res.timed_out));
                }
            }
            Err(e) => err = Some(e.to_string()),
        },
        "shell" => match run_shell(
            &ctx.cfg,
            &body_with_context,
            job.workdir.as_deref(),
            &job.shell,
            job.timeout_s,
        )
        .await
        {
            Ok(res) => {
                output_for_delivery = summarise(&res);
                ok = res.returncode == 0 && !res.timed_out;
                if !ok {
                    err = Some(format!("shell rc={} timed_out={}", res.returncode, res.timed_out));
                }
            }
            Err(e) => err = Some(e.to_string()),
        },
        "dream" => {
            let dreamer = DreamingScheduler::new(ctx.cfg.clone(), ctx.brain.clone());
            match dreamer.run_nightly().await {
                Ok(res) => {
                    tracing::info!(job = %job.id, summary = %res.log_line, "dream summary");
                    // Quietly send the Monday review ping out-of-band when
                    // there's something to review — independent of the job's
                    // own `deliver` setting, so the dreaming cron itself can
                    // stay silent on the other six days.
                    if let Some(msg) = res.telegram_msg.as_ref() {
                        if let Some(deliver) = &ctx.deliver {
                            if let Some(cid) =
                                job.deliver_chat_id.or(ctx.cfg.telegram_default_chat)
                            {
                                deliver(cid, msg.clone()).await;
                            }
                        }
                    }
                    output_for_delivery = res.log_line.clone();
                    ok = true;
                }
                Err(e) => err = Some(e.to_string()),
            }
        }
        other => {
            err = Some(format!("unknown kind: {other}"));
        }
    }

    if job.deliver == "telegram" && !output_for_delivery.is_empty() {
        if let Some(deliver) = &ctx.deliver {
            let chat_id = job.deliver_chat_id.or(ctx.cfg.telegram_default_chat);
            if let Some(cid) = chat_id {
                let msg = format!("[cron:{}] {}", job.id, output_for_delivery);
                deliver(cid, msg).await;
            }
        }
    }

    let elapsed_ms = t0.elapsed().as_millis() as u64;
    record_run(&ctx, &job, ok, err.as_deref(), elapsed_ms, &output_for_delivery).await;
}

async fn record_run(
    ctx: &CronContext,
    job: &Job,
    ok: bool,
    err: Option<&str>,
    elapsed_ms: u64,
    output: &str,
) {
    let ts = Local::now().to_rfc3339();
    // Truncate the cached output to LAST_OUTPUT_CAP chars so a verbose job
    // doesn't bloat cron-state.json or downstream `context_from` prompts.
    let truncated_output: Option<String> = if ok && !output.trim().is_empty() {
        let s: String = output.chars().take(LAST_OUTPUT_CAP).collect();
        Some(s)
    } else {
        None
    };
    {
        let mut map = ctx.state.lock().await;
        let st = map.entry(job.id.clone()).or_default();
        st.last_run_ts = Some(ts.clone());
        st.last_status = Some(if ok { "ok".into() } else { "error".into() });
        st.last_error = err.map(String::from);
        st.last_duration_ms = elapsed_ms;
        st.consecutive_errors = if ok { 0 } else { st.consecutive_errors + 1 };
        if let Some(o) = truncated_output {
            st.last_output = Some(o);
        }
        if let Err(e) = save_state(&ctx.state_path, &map) {
            tracing::warn!(error=%e, "save cron state failed");
        }
    }
    let preview: String = output.chars().take(500).collect();
    let rec = serde_json::json!({
        "ts": ts,
        "job_id": job.id,
        "name": job.name,
        "kind": job.kind,
        "ok": ok,
        "error": err,
        "elapsed_ms": elapsed_ms,
        "output_preview": preview,
    });
    if let Some(parent) = ctx.runs_log.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&ctx.runs_log) {
        let _ = writeln!(f, "{}", rec);
    }
}

fn load_state(path: &Path) -> Result<HashMap<String, JobState>> {
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let text = fs::read_to_string(path)?;
    let v: HashMap<String, JobState> = serde_json::from_str(&text).unwrap_or_default();
    Ok(v)
}

fn save_state(path: &Path, state: &HashMap<String, JobState>) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(state)?;
    fs::write(path, text)?;
    Ok(())
}
