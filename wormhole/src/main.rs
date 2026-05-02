#![allow(dead_code)]

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

mod brain;
mod config;
mod cron;
mod dreaming;
mod error;
mod memory;
mod providers;
mod skills;
mod subagent;
mod telegram;
mod tools;
mod ui;

use crate::brain::{Brain, RespondOpts};
use crate::config::Config;
use crate::cron::{CronRunner, DeliverFn};
use crate::dreaming::DreamingScheduler;
use crate::error::Result;
use crate::providers::{build_providers, ChatMessage};
use crate::telegram::TelegramBot;

#[derive(Parser, Debug)]
#[command(name = "larry", version, about = "Larry — multi-provider personal assistant")]
struct Cli {
    /// path to config.toml
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// log level: trace|debug|info|warn|error
    #[arg(long, global = true, default_value = "info")]
    log_level: String,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// run telegram + cron daemon
    Serve,
    /// smoke-test providers
    Test {
        #[arg(long)]
        prompt: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        model: Option<String>,
    },
    /// list configured cron jobs
    CronList {
        #[arg(long)]
        cron_path: Option<PathBuf>,
    },
    /// trigger a single cron job manually
    CronRun { job_id: String },
    /// one-shot prompt (prints to stdout)
    Ask {
        prompt: String,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        model: Option<String>,
    },
    /// run the nightly dreaming pass once and exit (manual trigger)
    Dream,
}

/// Ensure a UI auth token exists. Returns the token to use. Persists at
/// ~/wormhole/.token so subsequent boots reuse it. If the user explicitly
/// set `ui.token` in config.toml, that wins.
fn ensure_ui_token(cfg: &Config) -> Result<Option<String>> {
    if let Some(t) = cfg.ui_token.as_deref() {
        if !t.is_empty() {
            return Ok(Some(t.to_string()));
        }
    }
    let token_path = cfg.larry_home.join(".token");
    if let Ok(existing) = std::fs::read_to_string(&token_path) {
        let t = existing.trim().to_string();
        if !t.is_empty() {
            return Ok(Some(t));
        }
    }
    // Generate a fresh token (32 url-safe chars).
    let token: String = uuid::Uuid::new_v4().simple().to_string();
    std::fs::create_dir_all(&cfg.larry_home).ok();
    let _ = std::fs::write(&token_path, &token);
    tracing::warn!(
        path = %token_path.display(),
        "generated new UI token (32 hex chars). Copy to localStorage 'larry_token' in your dashboard, or paste into nodes.json's larry node 'token' field."
    );
    Ok(Some(token))
}

/// Lightweight startup timing recorder. Phases are stamped at each milestone;
/// the total elapsed at the time of mark is recorded so a flat `Vec` of
/// `(name, ms_since_start)` is enough to build a Gantt-ish view later.
struct DiagTimer {
    start: Instant,
    phases: Vec<(String, u64)>,
}

impl DiagTimer {
    fn new() -> Self {
        Self {
            start: Instant::now(),
            phases: Vec::new(),
        }
    }

    fn mark(&mut self, name: &str) {
        let ms = self.start.elapsed().as_millis() as u64;
        self.phases.push((name.to_string(), ms));
    }

    /// Build a one-line summary like `config=10ms providers=24ms ...`. Phase
    /// values are deltas between adjacent marks, so the numbers add up to the
    /// total. Independent of the JSON-on-disk path.
    fn summary(&self) -> String {
        let mut prev_ms: u64 = 0;
        let mut parts: Vec<String> = Vec::new();
        for (name, total_ms) in &self.phases {
            let delta = total_ms.saturating_sub(prev_ms);
            parts.push(format!("{name}={delta}ms"));
            prev_ms = *total_ms;
        }
        let total = self.phases.last().map(|(_, ms)| *ms).unwrap_or(0);
        format!("startup complete in {total}ms: {}", parts.join(" "))
    }

    fn write_json(&self, logs_dir: &std::path::Path) -> std::io::Result<()> {
        let ts = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
        let path = logs_dir.join(format!("startup-{ts}.json"));
        let phases: Vec<serde_json::Value> = self
            .phases
            .iter()
            .scan(0u64, |prev, (name, total_ms)| {
                let delta = total_ms.saturating_sub(*prev);
                *prev = *total_ms;
                Some(serde_json::json!({
                    "phase": name,
                    "elapsed_ms": total_ms,
                    "delta_ms": delta,
                }))
            })
            .collect();
        let body = serde_json::json!({
            "ts": chrono::Local::now().to_rfc3339(),
            "total_ms": self.phases.last().map(|(_, ms)| *ms).unwrap_or(0),
            "phases": phases,
        });
        std::fs::write(&path, serde_json::to_string_pretty(&body)?)?;
        Ok(())
    }
}

fn setup_logging(cfg: &Config, level: &str) -> Result<()> {
    cfg.ensure_dirs()?;
    let log_path = cfg.logs_dir.join("larry.log");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let env_filter = tracing_subscriber::EnvFilter::try_new(level)
        .or_else(|_| tracing_subscriber::EnvFilter::try_new("info"))
        .unwrap();
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_writer(std::io::stdout);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_ansi(false)
        .with_writer(std::sync::Mutex::new(file));
    tracing_subscriber::registry()
        .with(env_filter)
        .with(stdout_layer)
        .with(file_layer)
        .init();
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load(cli.config.as_deref())?;
    setup_logging(&cfg, &cli.log_level)?;

    match cli.cmd.unwrap_or(Cmd::Serve) {
        Cmd::Serve => cmd_serve(cfg).await,
        Cmd::Test { prompt, provider, model } => cmd_test(cfg, prompt, provider, model).await,
        Cmd::CronList { cron_path } => cmd_cron_list(cfg, cron_path).await,
        Cmd::CronRun { job_id } => cmd_cron_run(cfg, job_id).await,
        Cmd::Ask { prompt, provider, model } => cmd_ask(cfg, prompt, provider, model).await,
        Cmd::Dream => cmd_dream(cfg).await,
    }
}

async fn cmd_dream(cfg: Config) -> Result<()> {
    let cfg = Arc::new(cfg);
    let brain = Arc::new(Brain::new(cfg.clone())?);
    let dreamer = DreamingScheduler::new(cfg, brain);
    let res = dreamer.run_nightly().await?;
    println!("{}", res.log_line);
    if let Some(msg) = res.telegram_msg {
        println!("(monday review) {msg}");
    }
    Ok(())
}

async fn cmd_serve(mut cfg: Config) -> Result<()> {
    let mut timer = DiagTimer::new();
    timer.mark("config_loaded");

    // Ensure a UI token exists. If config.toml didn't set one, auto-generate
    // and persist to ~/wormhole/.token so subsequent boots reuse it. Codex
    // findings #41/#73: the previous default was "no token" + permissive CORS.
    cfg.ui_token = ensure_ui_token(&cfg)?;
    timer.mark("ui_token");

    let cfg = Arc::new(cfg);
    let brain = Arc::new(Brain::new(cfg.clone())?);
    timer.mark("brain_init");
    tracing::info!(
        providers = ?brain.list_providers(),
        default = %cfg.default_provider,
        skills = ?brain.skills().names(),
        tools = brain.tools_enabled(),
        "brain ready"
    );

    let bot = if cfg.telegram_token.is_some() {
        Some(TelegramBot::new(cfg.clone(), brain.clone())?)
    } else {
        tracing::warn!("no Telegram token — running cron-only");
        None
    };
    timer.mark("telegram_init");

    let deliver: Option<DeliverFn> = bot.as_ref().map(|b| {
        let bot = b.clone();
        Arc::new(move |chat_id: i64, text: String| -> futures::future::BoxFuture<'static, ()> {
            let bot = bot.clone();
            Box::pin(async move {
                bot.send(chat_id, &text, None).await;
            })
        }) as DeliverFn
    });

    let runner = Arc::new(CronRunner::new(cfg.clone(), brain.clone(), deliver, None).await?);
    runner.start().await?;
    timer.mark("cron_started");
    let n_jobs = runner.list_jobs().await.len();
    tracing::info!(jobs = n_jobs, telegram = bot.is_some(), ui = cfg.ui_enabled, "Larry serving");

    // Echo the UI token to Telegram on first start (or whenever it changes).
    // Stored at ~/wormhole/.token; the dashboard's "settings" prompt lets
    // the user paste it once. If the token was loaded from disk and the user
    // already pasted it last time, the dashboard will already work.
    if let (Some(bot), Some(token), Some(chat)) = (
        bot.as_ref(),
        cfg.ui_token.as_deref(),
        cfg.telegram_default_chat,
    ) {
        let token_path = cfg.larry_home.join(".token");
        let msg = format!(
            "Larry UI started. Token (paste into open-brain settings once):\n\n{token}\n\nStored at: {}",
            token_path.display()
        );
        bot.send(chat, &msg, None).await;
    }

    let bot_handle = bot.clone().map(|bot| {
        tokio::spawn(async move {
            if let Err(e) = bot.run().await {
                tracing::error!(error=%e, "telegram bot exited with error");
            }
        })
    });

    let ui_handle = if cfg.ui_enabled {
        let ui_state = ui::UiState {
            cfg: cfg.clone(),
            brain: brain.clone(),
            cron: runner.clone(),
            telegram_on: bot.is_some(),
        };
        Some(tokio::spawn(async move {
            if let Err(e) = ui::serve(ui_state).await {
                tracing::error!(error=%e, "ui server exited with error");
            }
        }))
    } else {
        None
    };
    timer.mark("ui_spawned");

    // Always log a one-line summary; opt in to a per-boot JSON snapshot via
    // [diagnostics] startup_timeline = true.
    tracing::info!("{}", timer.summary());
    if cfg.diagnostics.startup_timeline {
        if let Err(e) = timer.write_json(&cfg.logs_dir) {
            tracing::warn!(error=%e, "startup timeline write failed");
        }
    }

    tokio::select! {
        _ = tokio::signal::ctrl_c() => tracing::info!("ctrl-c, shutting down"),
        _ = async {
            if let Some(h) = bot_handle { h.await.ok(); }
            else { futures::future::pending::<()>().await }
        } => tracing::warn!("telegram task ended"),
        _ = async {
            if let Some(h) = ui_handle { h.await.ok(); }
            else { futures::future::pending::<()>().await }
        } => tracing::warn!("ui task ended"),
    }
    Ok(())
}

async fn cmd_test(
    cfg: Config,
    prompt: Option<String>,
    provider_filter: Option<String>,
    model_override: Option<String>,
) -> Result<()> {
    let cfg = Arc::new(cfg);
    let providers = build_providers(&cfg);
    if providers.is_empty() {
        eprintln!("ERROR: no providers initialised. Check your config and credentials.");
        std::process::exit(2);
    }
    println!("providers initialised: {}", providers.keys().cloned().collect::<Vec<_>>().join(", "));
    println!("default: {}  fallbacks: {:?}", cfg.default_provider, cfg.fallback_chain);
    let user_msg = prompt.unwrap_or_else(|| "Reply with the single word: PONG".to_string());
    let sys = "You are Larry's smoke test. Be terse.";
    let messages = vec![ChatMessage::user(user_msg)];
    for (name, prov) in providers.iter() {
        if let Some(filter) = &provider_filter {
            if filter != name {
                continue;
            }
        }
        println!("\n--- {} / {} ---", name, prov.default_model());
        let model = if provider_filter.as_deref() == Some(name.as_str()) {
            model_override.as_deref()
        } else {
            None
        };
        match prov.complete(&messages, model, Some(sys), 512, None).await {
            Ok(r) => println!(
                "OK {}ms in={} out={}: {:?}",
                r.elapsed_ms, r.input_tokens, r.output_tokens, r.text
            ),
            Err(e) => println!("FAIL: {e}"),
        }
    }
    Ok(())
}

async fn cmd_cron_list(cfg: Config, cron_path: Option<PathBuf>) -> Result<()> {
    let path = cron_path.unwrap_or_else(|| cfg.larry_home.join("cron.toml"));
    let jobs = crate::cron::load_jobs(&path)?;
    if jobs.is_empty() {
        println!("no jobs in {}", path.display());
        return Ok(());
    }
    for j in jobs {
        let flag = if j.enabled { "ON " } else { "off" };
        println!(
            "  [{flag}] {:30}  {:7}  {:20}  -> {}",
            j.id, j.kind, j.schedule, j.name
        );
    }
    Ok(())
}

async fn cmd_cron_run(cfg: Config, job_id: String) -> Result<()> {
    let cfg = Arc::new(cfg);
    let brain = Arc::new(Brain::new(cfg.clone())?);
    let runner = CronRunner::new(cfg, brain, None, None).await?;
    // Don't start the scheduler — just register and trigger one.
    runner.start().await?;
    runner.trigger(&job_id).await?;
    // give the spawned task a moment to run before we exit
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    println!("triggered {job_id} (output in logs/cron-runs.jsonl)");
    Ok(())
}

async fn cmd_ask(
    cfg: Config,
    prompt: String,
    provider_override: Option<String>,
    model_override: Option<String>,
) -> Result<()> {
    let cfg = Arc::new(cfg);
    let brain = Arc::new(Brain::new(cfg.clone())?);
    let r = brain
        .respond(
            &prompt,
            "cli:ask",
            RespondOpts {
                source: "cli",
                provider_override: provider_override.as_deref(),
                model_override: model_override.as_deref(),
                extra_system: "",
                allow_tools: true,
                max_iterations_override: None,
                auto_retry_on_cap: false,
            },
        )
        .await?;
    println!(
        "[{}/{} {}ms in={} out={}]\n{}",
        r.provider, r.model, r.elapsed_ms, r.input_tokens, r.output_tokens, r.text
    );
    Ok(())
}
