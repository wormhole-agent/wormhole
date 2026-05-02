//! Config loader: TOML file + env-var fallback + WormHole legacy locations.

use crate::error::{LarryError, Result};
use crate::tools::ToolConfig;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::{env, fs};

#[derive(Debug, Clone)]
pub struct ProviderCfg {
    pub name: String,
    pub enabled: bool,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub default_model: String,
    pub timeout_s: u64,
    /// "native" (provider's structured tool-calling), "text" (system-prompt
    /// injection + XML parsing), or "auto" (try native first, fall back to
    /// text on parse failure). Default: "auto".
    pub tools_style: String,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub larry_home: PathBuf,
    pub sessions_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub skills_dir: PathBuf,

    pub workspace_root: PathBuf,
    pub agents_md_name: String,
    pub memory_md_name: String,
    pub user_md_name: String,
    pub soul_md_name: String,
    pub daily_dir_name: String,
    pub daily_max_chars: usize,

    pub telegram_token: Option<String>,
    pub telegram_allowed_chats: Vec<i64>,
    pub telegram_default_chat: Option<i64>,

    pub providers: BTreeMap<String, ProviderCfg>,
    pub default_provider: String,
    pub fallback_chain: Vec<String>,
    pub max_tokens: u32,
    pub history_turns: usize,
    /// Model used by background work (cron jobs with tier="background"). When
    /// None, background jobs use the brain's default model — i.e., no tiering.
    pub background_model: Option<String>,

    pub delegate_claude_path: String,
    pub delegate_codex_path: String,
    /// Permission mode passed to `claude --permission-mode` for cron-delegated
    /// Claude jobs. Default `"bypassPermissions"` so cron jobs run unattended;
    /// tighten to e.g. `"acceptEdits"` once the daemon's subprocess policy is
    /// audited end-to-end. Codex review #cron328.
    pub cron_delegation_permission_mode: String,

    pub tools: ToolConfig,

    pub ui_enabled: bool,
    pub ui_bind: String,
    pub ui_port: u16,
    pub ui_token: Option<String>,
    /// Allow-list of origins permitted by the UI's CORS layer. Default:
    /// `["http://127.0.0.1:18900"]` (the openbrain dashboard). Anything
    /// else — including other loopback ports — is rejected, so a stray
    /// browser tab can't drive Larry's mutating endpoints. Codex review #41.
    pub ui_cors_origins: Vec<String>,

    pub dreaming: DreamingCfg,
    pub prompt_caching: PromptCachingCfg,
    pub diagnostics: DiagnosticsCfg,
}

#[derive(Debug, Clone, Default)]
pub struct DiagnosticsCfg {
    /// When true, the daemon writes a `logs/startup-<ts>.json` file with a
    /// per-phase timing breakdown each boot. The one-line summary is logged
    /// to tracing regardless. Useful when a slow boot needs attribution.
    pub startup_timeline: bool,
}

#[derive(Debug, Clone)]
pub struct PromptCachingCfg {
    /// Whether to attach `cache_control` blocks at all. Off skips the cache
    /// hint entirely — useful when debugging cache miss rates.
    pub enabled: bool,
    /// Anthropic cache TTL. `"ephemeral"` (~5 min, the default and currently
    /// the only value the provider actually wires through) or `"1h"` (1 hour,
    /// requires Anthropic's prompt-caching-2024-07-31 beta header — not yet
    /// implemented). Other values are accepted and logged-then-ignored so
    /// future config files don't break older binaries.
    pub cache_ttl: String,
}

impl Default for PromptCachingCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            cache_ttl: "ephemeral".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DreamingCfg {
    /// When true, the `dreaming` cron job actually runs. When false the cron
    /// fires but immediately returns a no-op message (lets you stage the job
    /// in cron.toml without having it churn).
    pub enabled: bool,
    /// Hour-of-day (0–23) in host local time at which to fire the pass.
    /// Default 3 → 3:00 AM, matching the legacy OpenClaw schedule. Kept for
    /// reference / external tooling — actual scheduling lives in cron.toml.
    pub run_hour_ct: u8,
    /// Provider name for the cheap "light sleep" pass that reads each daily
    /// file and extracts notable events.
    pub light_model: String,
    pub light_model_id: Option<String>,
    /// Provider name for the "REM" pass that synthesizes patterns and scores.
    pub rem_model: String,
    pub rem_model_id: Option<String>,
    /// Provider name for the "deep" pass that drafts MEMORY.md bullets.
    pub deep_model: String,
    pub deep_model_id: Option<String>,
    /// Items scoring at or above this in the REM phase get written to MEMORY.md.
    pub promotion_threshold: f64,
    /// How many days of history to scan in the light pass.
    pub window_days: u32,
}

// --- TOML schema ---

#[derive(Debug, Default, Deserialize)]
struct RawConfig {
    #[serde(default)]
    paths: RawPaths,
    #[serde(default)]
    memory: RawMemory,
    #[serde(default)]
    telegram: RawTelegram,
    #[serde(default)]
    brain: RawBrain,
    #[serde(default)]
    delegates: RawDelegates,
    #[serde(default)]
    cron: RawCron,
    #[serde(default)]
    providers: BTreeMap<String, RawProvider>,
    #[serde(default)]
    tools: RawTools,
    #[serde(default)]
    ui: RawUi,
    #[serde(default)]
    dreaming: RawDreaming,
    #[serde(default)]
    prompt_caching: RawPromptCaching,
    #[serde(default)]
    diagnostics: RawDiagnostics,
}

#[derive(Debug, Default, Deserialize)]
struct RawPromptCaching {
    enabled: Option<bool>,
    cache_ttl: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawDiagnostics {
    startup_timeline: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct RawDreaming {
    enabled: Option<bool>,
    run_hour_ct: Option<u8>,
    light_model: Option<String>,
    light_model_id: Option<String>,
    rem_model: Option<String>,
    rem_model_id: Option<String>,
    deep_model: Option<String>,
    deep_model_id: Option<String>,
    promotion_threshold: Option<f64>,
    window_days: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
struct RawTools {
    enabled: Option<bool>,
    max_iterations: Option<u32>,
    shell_enabled: Option<bool>,
    shell_block_patterns: Option<Vec<String>>,
    write_file_allowed_paths: Option<Vec<String>>,
    read_file_allowed_paths: Option<Vec<String>>,
    http_get_blocked_hosts: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct RawUi {
    enabled: Option<bool>,
    bind: Option<String>,
    port: Option<u16>,
    token: Option<String>,
    cors_origins: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
struct RawPaths {
    home: Option<String>,
    sessions_dir: Option<String>,
    logs_dir: Option<String>,
    skills_dir: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawMemory {
    workspace_root: Option<String>,
    agents_md: Option<String>,
    memory_md: Option<String>,
    user_md: Option<String>,
    soul_md: Option<String>,
    daily_dir: Option<String>,
    daily_max_chars: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
struct RawTelegram {
    bot_token: Option<String>,
    allowed_chat_ids: Option<Vec<i64>>,
    default_chat_id: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct RawBrain {
    default_provider: Option<String>,
    fallbacks: Option<Vec<String>>,
    max_tokens: Option<u32>,
    history_turns: Option<usize>,
    background_model: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawDelegates {
    claude: Option<String>,
    codex: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawCron {
    delegation_permission_mode: Option<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
struct RawProvider {
    enabled: Option<bool>,
    default_model: Option<String>,
    base_url: Option<String>,
    timeout_s: Option<u64>,
    api_key: Option<String>,
    tools_style: Option<String>,
}

// --- WormHole legacy helpers ---

fn wormhole_home() -> PathBuf {
    if let Ok(p) = env::var("WORMHOLE_HOME") {
        return PathBuf::from(p);
    }
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".wormhole")
}

/// Read a single API key from WormHole's `auth-profiles.json` by profile id
/// (e.g. `"anthropic:default"`, `"openai:default"`). Returns `None` if the file
/// is missing, can't be parsed, or the profile/key isn't there. Codex review #8
/// (was duplicated as `read_wormhole_anthropic_key` + `read_wormhole_openai_key`).
fn read_wormhole_profile_key(profile_id: &str) -> Option<String> {
    let p = wormhole_home()
        .join("agents")
        .join("main")
        .join("agent")
        .join("auth-profiles.json");
    let text = fs::read_to_string(p).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("profiles")?
        .get(profile_id)?
        .get("key")?
        .as_str()
        .map(String::from)
}

fn read_wormhole_telegram_token() -> Option<String> {
    let p = wormhole_home().join("wormhole.json");
    let text = fs::read_to_string(p).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("channels")?
        .get("telegram")?
        .get("botToken")?
        .as_str()
        .map(String::from)
}

// --- Loader ---

impl Config {
    pub fn larry_home_default() -> PathBuf {
        if let Ok(p) = env::var("LARRY_HOME") {
            return PathBuf::from(p);
        }
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join("larry")
    }

    pub fn load(config_path: Option<&Path>) -> Result<Self> {
        let larry_home = Self::larry_home_default();
        let path = config_path
            .map(PathBuf::from)
            .unwrap_or_else(|| larry_home.join("config.toml"));

        let raw: RawConfig = if path.exists() {
            let text = fs::read_to_string(&path)?;
            toml::from_str(&text)
                .map_err(|e| LarryError::Config(format!("parsing {}: {}", path.display(), e)))?
        } else {
            RawConfig::default()
        };

        let larry_home = raw
            .paths
            .home
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or(larry_home);

        let sessions_dir = raw
            .paths
            .sessions_dir
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| larry_home.join("sessions"));
        let logs_dir = raw
            .paths
            .logs_dir
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| larry_home.join("logs"));
        let skills_dir = raw
            .paths
            .skills_dir
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| larry_home.join("skills"));

        let workspace_root = raw
            .memory
            .workspace_root
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| wormhole_home().join("workspace"));

        let telegram_token = env::var("TELEGRAM_BOT_TOKEN")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| raw.telegram.bot_token.clone().filter(|s| !s.is_empty()))
            .or_else(read_wormhole_telegram_token);

        let mut providers: BTreeMap<String, ProviderCfg> = BTreeMap::new();
        let defaults: &[(&str, &str, Option<&str>)] = &[
            ("anthropic", "claude-sonnet-4-6", None),
            ("openai", "gpt-5.5", None),
            ("deepseek", "deepseek-chat", Some("https://api.deepseek.com")),
            ("ollama", "qwen3.6:27b", Some("http://127.0.0.1:11434")),
        ];

        for (name, default_model, default_base) in defaults {
            let entry = raw.providers.get(*name).cloned().unwrap_or_default();
            let api_key = match *name {
                "anthropic" => env::var("ANTHROPIC_API_KEY")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .or_else(|| entry.api_key.clone().filter(|s| !s.is_empty()))
                    .or_else(|| read_wormhole_profile_key("anthropic:default")),
                "openai" => env::var("OPENAI_API_KEY")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .or_else(|| entry.api_key.clone().filter(|s| !s.is_empty()))
                    .or_else(|| read_wormhole_profile_key("openai:default")),
                "deepseek" => env::var("DEEPSEEK_API_KEY")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .or_else(|| entry.api_key.clone().filter(|s| !s.is_empty())),
                _ => entry.api_key.clone(),
            };
            providers.insert(
                name.to_string(),
                ProviderCfg {
                    name: name.to_string(),
                    enabled: entry.enabled.unwrap_or(true),
                    api_key,
                    base_url: entry
                        .base_url
                        .clone()
                        .or_else(|| default_base.map(String::from)),
                    default_model: entry
                        .default_model
                        .clone()
                        .unwrap_or_else(|| default_model.to_string()),
                    timeout_s: entry.timeout_s.unwrap_or(120),
                    tools_style: entry
                        .tools_style
                        .clone()
                        .unwrap_or_else(|| "auto".into()),
                },
            );
        }

        Ok(Config {
            larry_home,
            sessions_dir,
            logs_dir,
            skills_dir,
            workspace_root,
            agents_md_name: raw.memory.agents_md.unwrap_or_else(|| "AGENTS.md".into()),
            memory_md_name: raw.memory.memory_md.unwrap_or_else(|| "MEMORY.md".into()),
            user_md_name: raw.memory.user_md.unwrap_or_else(|| "USER.md".into()),
            soul_md_name: raw.memory.soul_md.unwrap_or_else(|| "SOUL.md".into()),
            daily_dir_name: raw.memory.daily_dir.unwrap_or_else(|| "memory".into()),
            daily_max_chars: raw.memory.daily_max_chars.unwrap_or(60_000),
            telegram_token,
            telegram_allowed_chats: raw.telegram.allowed_chat_ids.unwrap_or_default(),
            telegram_default_chat: raw.telegram.default_chat_id,
            providers,
            default_provider: raw
                .brain
                .default_provider
                .unwrap_or_else(|| "anthropic".into()),
            fallback_chain: raw.brain.fallbacks.unwrap_or_default(),
            max_tokens: raw.brain.max_tokens.unwrap_or(4096),
            history_turns: raw.brain.history_turns.unwrap_or(8),
            background_model: raw
                .brain
                .background_model
                .filter(|s| !s.is_empty())
                .or_else(|| Some("claude-haiku-4-5".into())),
            delegate_claude_path: raw.delegates.claude.unwrap_or_else(|| "claude".into()),
            delegate_codex_path: raw.delegates.codex.unwrap_or_else(|| "codex".into()),
            cron_delegation_permission_mode: raw
                .cron
                .delegation_permission_mode
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "bypassPermissions".into()),
            tools: {
                let mut t = ToolConfig::default();
                if let Some(v) = raw.tools.enabled { t.enabled = v; }
                if let Some(v) = raw.tools.max_iterations { t.max_iterations = v; }
                if let Some(v) = raw.tools.shell_enabled { t.shell_enabled = v; }
                if let Some(v) = raw.tools.shell_block_patterns { t.shell_block_patterns = v; }
                if let Some(v) = raw.tools.write_file_allowed_paths {
                    t.write_file_allowed_paths = v.into_iter().map(PathBuf::from).collect();
                }
                if let Some(v) = raw.tools.read_file_allowed_paths {
                    t.read_file_allowed_paths = v.into_iter().map(PathBuf::from).collect();
                }
                if let Some(v) = raw.tools.http_get_blocked_hosts { t.http_get_blocked_hosts = v; }
                t
            },
            ui_enabled: raw.ui.enabled.unwrap_or(true),
            ui_bind: raw.ui.bind.unwrap_or_else(|| "127.0.0.1".into()),
            ui_port: raw.ui.port.unwrap_or(18790),
            ui_token: raw.ui.token,
            ui_cors_origins: raw
                .ui
                .cors_origins
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| vec!["http://127.0.0.1:18900".into()]),
            prompt_caching: PromptCachingCfg {
                enabled: raw.prompt_caching.enabled.unwrap_or(true),
                cache_ttl: raw
                    .prompt_caching
                    .cache_ttl
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "ephemeral".into()),
            },
            diagnostics: DiagnosticsCfg {
                startup_timeline: raw.diagnostics.startup_timeline.unwrap_or(false),
            },
            dreaming: DreamingCfg {
                enabled: raw.dreaming.enabled.unwrap_or(true),
                run_hour_ct: raw.dreaming.run_hour_ct.unwrap_or(3),
                light_model: raw.dreaming.light_model.unwrap_or_else(|| "ollama".into()),
                light_model_id: raw.dreaming.light_model_id,
                rem_model: raw.dreaming.rem_model.unwrap_or_else(|| "deepseek".into()),
                rem_model_id: raw.dreaming.rem_model_id,
                deep_model: raw.dreaming.deep_model.unwrap_or_else(|| "anthropic".into()),
                deep_model_id: raw.dreaming.deep_model_id,
                promotion_threshold: raw.dreaming.promotion_threshold.unwrap_or(0.85),
                window_days: raw.dreaming.window_days.unwrap_or(7),
            },
        })
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        for p in [&self.larry_home, &self.sessions_dir, &self.logs_dir, &self.skills_dir] {
            fs::create_dir_all(p)?;
        }
        Ok(())
    }
}
