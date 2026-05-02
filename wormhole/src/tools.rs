//! Built-in tool registry. Each tool has a declared input schema (JSON-Schema-ish)
//! and an async executor. All calls are appended to `logs/tools.jsonl` for audit.
//!
//! v0.2 tools:
//!   - shell           : run a shell command (bash/pwsh/cmd). Blocked-pattern check.
//!   - read_file       : read a file under workspace_root, larry_home, or whitelisted paths.
//!   - write_file      : write a file. Restricted to allowed_paths from config.
//!   - http_get        : GET a URL. Blocked-host check.
//!   - delegate_claude : run Claude Code subprocess.
//!   - delegate_codex  : run Codex subprocess.
//!   - list_skills     : list available skills (name + when_to_use).
//!   - load_skill      : return the full body of a named skill.

use crate::config::Config;
use crate::providers::ToolDef;
use crate::skills::SkillRegistry;
use crate::subagent::{run_claude, run_codex, run_shell, summarise as summarise_subagent};
use chrono::Local;
use regex::Regex;
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

const MAX_FILE_READ: usize = 200_000;
const MAX_HTTP_BYTES: usize = 500_000;

/// Default block list for `shell`. Regex-matched (case-insensitive) against the command string.
const DEFAULT_SHELL_BLOCK_PATTERNS: &[&str] = &[
    r"(?i)\brm\s+(-[a-z]*r[a-z]*f|--recursive --force|-rf)\s+/",
    r"(?i)\bformat\s+[a-z]:",
    r"(?i)\b(mkfs|wipefs|shred)\b",
    r"(?i)\bdd\s+if=/dev/(zero|urandom|random)\s+of=",
    r"(?i)\bgit\s+push\s+.*--force",
    r"(?i)\bcurl\s+[^|]*\|\s*(bash|sh|zsh|pwsh)",
    r"(?i)\bregsvr32\s+",
    r"(?i)\bschtasks\s+/delete",
    r"(?i)\b(rm\s+-rf\s+~|rm\s+-rf\s+\$HOME|rm\s+-rf\s+/c/Users/[^/\s]+)\b",
];

/// Hardline patterns: catastrophic, never-recoverable shell commands. These
/// are checked BEFORE the configurable `shell_block_patterns` and CANNOT be
/// disabled at runtime. Substring match (case-insensitive). Keep this list
/// short and unambiguous — false positives here turn into "Larry refuses to
/// run anything" gripes; false negatives turn into "Larry deleted the disk".
/// Mirrors the `hardline` blocklist from Hermes Agent v0.12.0.
const HARDLINE_BLOCKED: &[&str] = &[
    "rm -rf /",
    "rm -rf /*",
    "rm -rf --no-preserve-root",
    "format c:",
    "del /f /s /q c:\\",
    "remove-item -recurse -force c:\\",
    "remove-item c:\\ -recurse",
    "dd if=/dev/zero of=/dev/",
    "dd if=/dev/urandom of=/dev/",
    "dd if=/dev/random of=/dev/",
    "mkfs ",
    "mkfs.ext4 /dev/",
    ":(){:|:&};:",
];

#[derive(Debug, Clone)]
pub struct ToolConfig {
    pub enabled: bool,
    pub max_iterations: u32,
    pub shell_enabled: bool,
    pub shell_block_patterns: Vec<String>,
    pub write_file_allowed_paths: Vec<PathBuf>,
    /// If empty, falls back to workspace_root + larry_home + openbrain/data.
    /// Otherwise, replaces those defaults entirely. Codex review #344.
    pub read_file_allowed_paths: Vec<PathBuf>,
    pub http_get_blocked_hosts: Vec<String>,
}

impl Default for ToolConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_iterations: 30,
            shell_enabled: true,
            shell_block_patterns: DEFAULT_SHELL_BLOCK_PATTERNS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            write_file_allowed_paths: vec![],
            read_file_allowed_paths: vec![],
            http_get_blocked_hosts: vec![
                "169.254.169.254".into(),
                "metadata.google.internal".into(),
                "metadata.azure.com".into(),
            ],
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolOutcome {
    pub content: String,
    pub is_error: bool,
}

pub struct ToolRegistry {
    cfg: Arc<Config>,
    tcfg: ToolConfig,
    skills: Arc<SkillRegistry>,
    http: reqwest::Client,
    audit_log: PathBuf,
    audit_lock: Mutex<()>,
    block_regexes: Vec<Regex>,
}

impl ToolRegistry {
    pub fn new(
        cfg: Arc<Config>,
        tcfg: ToolConfig,
        skills: Arc<SkillRegistry>,
    ) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("larry-tools/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("build http client");
        let audit_log = cfg.logs_dir.join("tools.jsonl");
        let block_regexes = tcfg
            .shell_block_patterns
            .iter()
            .filter_map(|p| match Regex::new(p) {
                Ok(r) => Some(r),
                Err(e) => {
                    tracing::warn!(pattern=%p, error=%e, "bad shell block pattern, ignoring");
                    None
                }
            })
            .collect();
        Self {
            cfg,
            tcfg,
            skills,
            http,
            audit_log,
            audit_lock: Mutex::new(()),
            block_regexes,
        }
    }

    pub fn enabled(&self) -> bool {
        self.tcfg.enabled
    }

    pub fn max_iterations(&self) -> u32 {
        self.tcfg.max_iterations
    }

    /// Definitions to ship to the model.
    pub fn tool_defs(&self) -> Vec<ToolDef> {
        let mut tools = vec![];

        if self.tcfg.shell_enabled {
            tools.push(ToolDef {
                name: "shell".into(),
                description: concat!(
                    "Run a shell command and return its stdout/stderr. Use for: running scripts, ",
                    "checking process state, listing files, running git/gh/npm/python/node CLIs. ",
                    "Default shell is bash. Use shell='pwsh' or 'cmd' for Windows-only commands. ",
                    "Returns combined stdout+stderr with exit code."
                ).into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "the command to run" },
                        "shell": { "type": "string", "enum": ["bash", "pwsh", "cmd"], "default": "bash" },
                        "cwd": { "type": "string", "description": "working dir (default: workspace_root)" },
                        "timeout_s": { "type": "integer", "default": 60, "minimum": 1, "maximum": 600 }
                    },
                    "required": ["command"]
                }),
            });
        }

        tools.push(ToolDef {
            name: "read_file".into(),
            description: "Read a UTF-8 text file. Path may be absolute or relative to workspace_root. Returns file content or an error if missing/too-large.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "max_chars": { "type": "integer", "default": 100000, "minimum": 1, "maximum": 200000 }
                },
                "required": ["path"]
            }),
        });

        tools.push(ToolDef {
            name: "write_file".into(),
            description: concat!(
                "Write a UTF-8 text file. Path must be inside one of the allowed directories ",
                "(skills/, workspace memory/, wormhole logs/). Use mode='overwrite' to replace, ",
                "'append' to add to end. Refuses paths outside allow-list."
            ).into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" },
                    "mode": { "type": "string", "enum": ["overwrite", "append"], "default": "overwrite" }
                },
                "required": ["path", "content"]
            }),
        });

        tools.push(ToolDef {
            name: "http_get".into(),
            description: "GET a URL and return text body (capped). Use for fetching docs, RSS, simple JSON APIs.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string" },
                    "headers": {
                        "type": "object",
                        "description": "optional request headers",
                        "additionalProperties": { "type": "string" }
                    }
                },
                "required": ["url"]
            }),
        });

        tools.push(ToolDef {
            name: "browser_fetch".into(),
            description: concat!(
                "Fetch a web page and return its text content (HTML stripped, scripts/styles removed). ",
                "Use for: reading articles, docs, READMEs, blog posts, API docs. ",
                "Better than http_get for HTML pages because the output is reading-grade, not raw markup. ",
                "For raw JSON or non-HTML content, prefer http_get."
            ).into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string" },
                    "max_chars": { "type": "integer", "default": 50000, "minimum": 500, "maximum": 200000 }
                },
                "required": ["url"]
            }),
        });

        tools.push(ToolDef {
            name: "browser_search".into(),
            description: concat!(
                "Web search via Brave Search API. Returns the top N organic results with title/url/description. ",
                "Use when you need current information not in training data, or to find a specific page before browser_fetching it. ",
                "Requires BRAVE_SEARCH_API_KEY in the environment."
            ).into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "count": { "type": "integer", "default": 8, "minimum": 1, "maximum": 20 },
                    "country": { "type": "string", "default": "US" }
                },
                "required": ["query"]
            }),
        });

        tools.push(ToolDef {
            name: "delegate_claude".into(),
            description: concat!(
                "Spawn a Claude Code subagent (full file-edit / multi-step coding). ",
                "Use when a task needs reading/editing many files, running test suites, or ",
                "iterating on code. Returns the subagent's stdout summary."
            ).into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "prompt": { "type": "string" },
                    "cwd": { "type": "string" },
                    "timeout_s": { "type": "integer", "default": 1800, "minimum": 30, "maximum": 14400 }
                },
                "required": ["prompt"]
            }),
        });

        tools.push(ToolDef {
            name: "delegate_codex".into(),
            description: "Spawn a Codex CLI subagent (alternative to Claude Code).".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "prompt": { "type": "string" },
                    "cwd": { "type": "string" },
                    "timeout_s": { "type": "integer", "default": 1800, "minimum": 30, "maximum": 14400 }
                },
                "required": ["prompt"]
            }),
        });

        tools.push(ToolDef {
            name: "list_skills".into(),
            description: "List the available skills (name + when_to_use). Call this to discover what specialized prompts/playbooks are loaded.".into(),
            input_schema: json!({ "type": "object", "properties": {} }),
        });

        tools.push(ToolDef {
            name: "load_skill".into(),
            description: "Return the full body of a named skill, so the assistant can follow its instructions.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" }
                },
                "required": ["name"]
            }),
        });

        tools
    }

    pub async fn execute(&self, name: &str, input: &Value) -> ToolOutcome {
        let t0 = Instant::now();
        let outcome = match name {
            "shell" => self.exec_shell(input).await,
            "read_file" => self.exec_read_file(input).await,
            "write_file" => self.exec_write_file(input).await,
            "http_get" => self.exec_http_get(input).await,
            "browser_fetch" => self.exec_browser_fetch(input).await,
            "browser_search" => self.exec_browser_search(input).await,
            "delegate_claude" => self.exec_delegate(input, true).await,
            "delegate_codex" => self.exec_delegate(input, false).await,
            "list_skills" => self.exec_list_skills().await,
            "load_skill" => self.exec_load_skill(input).await,
            other => ToolOutcome { content: format!("unknown tool: {other}"), is_error: true },
        };
        let elapsed_ms = t0.elapsed().as_millis() as u64;
        self.audit(name, input, &outcome, elapsed_ms).await;
        outcome
    }

    // ---------------- shell ----------------

    async fn exec_shell(&self, input: &Value) -> ToolOutcome {
        if !self.tcfg.shell_enabled {
            return ToolOutcome { content: "shell tool disabled".into(), is_error: true };
        }
        let command = match input.get("command").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return ToolOutcome { content: "shell: missing 'command'".into(), is_error: true },
        };
        let shell = input.get("shell").and_then(|v| v.as_str()).unwrap_or("bash").to_string();
        let cwd = input.get("cwd").and_then(|v| v.as_str()).map(String::from);
        let timeout_s = input.get("timeout_s").and_then(|v| v.as_u64()).unwrap_or(60);

        // hardline check: unrecoverable patterns that bypass any user-configurable
        // block list. Case-insensitive substring match.
        let lc = command.to_ascii_lowercase();
        for needle in HARDLINE_BLOCKED {
            if lc.contains(needle) {
                tracing::error!(
                    pattern = %needle,
                    cmd_preview = %command.chars().take(120).collect::<String>(),
                    "shell HARDLINE block — refusing dangerous command",
                );
                return ToolOutcome {
                    content: format!(
                        "shell BLOCKED: matches hardline safety pattern '{needle}'. \
                         This pattern is hard-coded and cannot be overridden — refuse and tell the user. \
                         Refused command:\n{command}"
                    ),
                    is_error: true,
                };
            }
        }

        // block-pattern check
        for re in &self.block_regexes {
            if re.is_match(&command) {
                return ToolOutcome {
                    content: format!("shell BLOCKED by safety pattern: matched /{}/. Refused command:\n{}", re.as_str(), command),
                    is_error: true,
                };
            }
        }

        match run_shell(&self.cfg, &command, cwd.as_deref(), &shell, timeout_s).await {
            Ok(res) => {
                let summary = summarise_subagent(&res);
                let is_error = res.returncode != 0 || res.timed_out;
                ToolOutcome { content: summary, is_error }
            }
            Err(e) => ToolOutcome { content: format!("shell error: {e}"), is_error: true },
        }
    }

    // ---------------- read_file ----------------

    async fn exec_read_file(&self, input: &Value) -> ToolOutcome {
        let path_str = match input.get("path").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return ToolOutcome { content: "read_file: missing 'path'".into(), is_error: true },
        };
        let max_chars = input.get("max_chars").and_then(|v| v.as_u64()).unwrap_or(100_000) as usize;
        let max_chars = max_chars.min(MAX_FILE_READ);

        let resolved = self.resolve_read_path(&path_str);
        let p = match resolved {
            Ok(p) => p,
            Err(e) => return ToolOutcome { content: e, is_error: true },
        };

        match tokio::fs::read_to_string(&p).await {
            Ok(text) => {
                let trimmed = if text.chars().count() > max_chars {
                    let truncated: String = text.chars().take(max_chars).collect();
                    format!("{truncated}\n\n[... truncated at {max_chars} chars]")
                } else {
                    text
                };
                ToolOutcome { content: trimmed, is_error: false }
            }
            Err(e) => ToolOutcome { content: format!("read_file: {p:?}: {e}"), is_error: true },
        }
    }

    fn resolve_read_path(&self, raw: &str) -> std::result::Result<PathBuf, String> {
        let p = normalize_input_path(raw);
        let p = if p.is_absolute() { p } else { self.cfg.workspace_root.join(p) };
        let canonical = p.canonicalize().or_else(|_| Ok::<_, std::io::Error>(p.clone())).unwrap();

        // Read allow-list: configurable (read_file_allowed_paths) wins; if empty,
        // fall back to workspace + larry_home + ~/openbrain/data. Component-aware
        // via is_within (Codex review #344).
        let openbrain_data = dirs::home_dir()
            .map(|h| h.join("openbrain").join("data"))
            .unwrap_or_else(|| PathBuf::from("openbrain/data"));
        let default_roots: Vec<&Path> = vec![
            self.cfg.workspace_root.as_path(),
            self.cfg.larry_home.as_path(),
            openbrain_data.as_path(),
        ];
        let configured: Vec<&Path> = self.tcfg.read_file_allowed_paths.iter().map(|p| p.as_path()).collect();
        let allowed: Vec<&Path> = if configured.is_empty() { default_roots } else { configured };

        if allowed.iter().any(|root| is_within(&canonical, root, false)) {
            Ok(canonical)
        } else {
            Err(format!(
                "read_file: path {} is outside allowed roots:\n  {}",
                canonical.display(),
                allowed.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n  ")
            ))
        }
    }

    // ---------------- write_file ----------------

    async fn exec_write_file(&self, input: &Value) -> ToolOutcome {
        let path_str = match input.get("path").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return ToolOutcome { content: "write_file: missing 'path'".into(), is_error: true },
        };
        let content = match input.get("content").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return ToolOutcome { content: "write_file: missing 'content'".into(), is_error: true },
        };
        let mode = input.get("mode").and_then(|v| v.as_str()).unwrap_or("overwrite");

        let p = normalize_input_path(&path_str);
        let p = if p.is_absolute() { p } else { self.cfg.workspace_root.join(p) };
        // Resolve symlinks/parents: use absolutize-by-walking
        let canonical = canonical_or_self(&p);

        // Default allowed write roots if config didn't specify any
        let mut roots: Vec<PathBuf> = self.tcfg.write_file_allowed_paths.clone();
        if roots.is_empty() {
            roots.push(self.cfg.skills_dir.clone());
            roots.push(self.cfg.sessions_dir.clone());
            roots.push(self.cfg.larry_home.join("scratch"));
            roots.push(self.cfg.workspace_root.join("memory"));
            roots.push(self.cfg.workspace_root.join("_scratch"));
        }
        // A root that points at a file (has an extension) is treated as
        // file-mode (exact-match required); a root that's a directory accepts
        // any descendant. Component-aware via is_within.
        let allowed = roots.iter().any(|root| {
            let as_file = root.extension().is_some();
            is_within(&canonical, root, as_file)
        });
        if !allowed {
            return ToolOutcome {
                content: format!(
                    "write_file: path {} outside allowed roots:\n  {}",
                    canonical.display(),
                    roots.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n  ")
                ),
                is_error: true,
            };
        }
        if let Some(parent) = canonical.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                return ToolOutcome { content: format!("write_file mkdir: {e}"), is_error: true };
            }
        }

        let res = match mode {
            "append" => OpenOptions::new()
                .create(true)
                .append(true)
                .open(&canonical)
                .and_then(|mut f| f.write_all(content.as_bytes())),
            _ => fs::write(&canonical, content.as_bytes()),
        };
        match res {
            Ok(()) => ToolOutcome {
                content: format!("wrote {} bytes to {}", content.len(), canonical.display()),
                is_error: false,
            },
            Err(e) => ToolOutcome { content: format!("write_file: {e}"), is_error: true },
        }
    }

    // ---------------- http_get ----------------

    async fn exec_http_get(&self, input: &Value) -> ToolOutcome {
        let url_str = match input.get("url").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return ToolOutcome { content: "http_get: missing 'url'".into(), is_error: true },
        };

        // host blocklist
        if let Ok(parsed) = url::Url::parse(&url_str) {
            if let Some(host) = parsed.host_str() {
                let h = host.to_ascii_lowercase();
                if self.tcfg.http_get_blocked_hosts.iter().any(|b| h.contains(&b.to_ascii_lowercase())) {
                    return ToolOutcome {
                        content: format!("http_get BLOCKED: host {h} is in blocked_hosts"),
                        is_error: true,
                    };
                }
            }
        }

        let mut req = self.http.get(&url_str).timeout(std::time::Duration::from_secs(45));
        if let Some(headers) = input.get("headers").and_then(|v| v.as_object()) {
            for (k, v) in headers {
                if let Some(s) = v.as_str() {
                    req = req.header(k, s);
                }
            }
        }

        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => return ToolOutcome { content: format!("http_get: {e}"), is_error: true },
        };
        let status = resp.status();
        let body = match resp.text().await {
            Ok(t) => t,
            Err(e) => return ToolOutcome { content: format!("http_get body: {e}"), is_error: true },
        };
        let body = if body.len() > MAX_HTTP_BYTES {
            let prefix: String = body.chars().take(MAX_HTTP_BYTES).collect();
            format!("{prefix}\n\n[... truncated at {MAX_HTTP_BYTES} bytes]")
        } else {
            body
        };
        let preview = format!("HTTP {status}\n\n{body}");
        ToolOutcome { content: preview, is_error: !status.is_success() }
    }

    // ---------------- browser_fetch ----------------

    async fn exec_browser_fetch(&self, input: &Value) -> ToolOutcome {
        let url_str = match input.get("url").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return ToolOutcome { content: "browser_fetch: missing 'url'".into(), is_error: true },
        };
        let max_chars = input.get("max_chars").and_then(|v| v.as_u64()).unwrap_or(50_000) as usize;
        let max_chars = max_chars.min(200_000);

        if let Ok(parsed) = url::Url::parse(&url_str) {
            if let Some(host) = parsed.host_str() {
                let h = host.to_ascii_lowercase();
                if self.tcfg.http_get_blocked_hosts.iter().any(|b| h.contains(&b.to_ascii_lowercase())) {
                    return ToolOutcome {
                        content: format!("browser_fetch BLOCKED: host {h} is in blocked_hosts"),
                        is_error: true,
                    };
                }
            }
        }

        let resp = match self.http
            .get(&url_str)
            .timeout(std::time::Duration::from_secs(45))
            .header("user-agent", concat!("larry-browser/", env!("CARGO_PKG_VERSION"), " (+open-brain)"))
            .header("accept", "text/html,application/xhtml+xml,*/*;q=0.8")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolOutcome { content: format!("browser_fetch: {e}"), is_error: true },
        };

        let status = resp.status();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();

        let body = match resp.text().await {
            Ok(t) => t,
            Err(e) => return ToolOutcome { content: format!("browser_fetch body: {e}"), is_error: true },
        };

        let text = if content_type.contains("html") || body.trim_start().starts_with('<') {
            html_to_text(&body)
        } else {
            body
        };

        let trimmed = if text.chars().count() > max_chars {
            let prefix: String = text.chars().take(max_chars).collect();
            format!("{prefix}\n\n[... truncated at {max_chars} chars]")
        } else {
            text
        };

        let header_line = format!("HTTP {status} · url={url_str}\n\n");
        ToolOutcome {
            content: header_line + &trimmed,
            is_error: !status.is_success(),
        }
    }

    // ---------------- browser_search (Brave) ----------------

    async fn exec_browser_search(&self, input: &Value) -> ToolOutcome {
        let query = match input.get("query").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return ToolOutcome { content: "browser_search: missing 'query'".into(), is_error: true },
        };
        let count = input.get("count").and_then(|v| v.as_u64()).unwrap_or(8).clamp(1, 20);
        let country = input.get("country").and_then(|v| v.as_str()).unwrap_or("US");

        let token = match std::env::var("BRAVE_SEARCH_API_KEY") {
            Ok(t) if !t.is_empty() => t,
            _ => return ToolOutcome {
                content: "browser_search: BRAVE_SEARCH_API_KEY not set in env".into(),
                is_error: true,
            },
        };

        let url = "https://api.search.brave.com/res/v1/web/search";
        let resp = match self.http
            .get(url)
            .query(&[("q", query.as_str()), ("count", &count.to_string()), ("country", country), ("safesearch", "moderate")])
            .timeout(std::time::Duration::from_secs(15))
            .header("x-subscription-token", token)
            .header("accept", "application/json")
            .header("user-agent", concat!("larry-browser/", env!("CARGO_PKG_VERSION")))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolOutcome { content: format!("browser_search http: {e}"), is_error: true },
        };

        let status = resp.status();
        let body = match resp.text().await {
            Ok(t) => t,
            Err(e) => return ToolOutcome { content: format!("browser_search body: {e}"), is_error: true },
        };

        if !status.is_success() {
            return ToolOutcome {
                content: format!("browser_search {} {}", status, body.chars().take(400).collect::<String>()),
                is_error: true,
            };
        }

        // Parse Brave's response shape: { web: { results: [{ title, url, description }] } }
        let parsed: Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => return ToolOutcome { content: format!("browser_search decode: {e}"), is_error: true },
        };

        let mut out = String::new();
        out.push_str(&format!("query: {query}\nresults:\n"));
        let empty: Vec<Value> = vec![];
        let results = parsed
            .get("web")
            .and_then(|w| w.get("results"))
            .and_then(|r| r.as_array())
            .unwrap_or(&empty);

        if results.is_empty() {
            out.push_str("(no results)\n");
        } else {
            for (i, r) in results.iter().enumerate() {
                let title = r.get("title").and_then(|v| v.as_str()).unwrap_or("(untitled)");
                let url   = r.get("url").and_then(|v| v.as_str()).unwrap_or("");
                let desc  = r.get("description").and_then(|v| v.as_str()).unwrap_or("");
                out.push_str(&format!("{}. {title}\n   {url}\n   {desc}\n\n", i + 1));
            }
        }
        ToolOutcome { content: out, is_error: false }
    }

    // ---------------- delegate_claude / delegate_codex ----------------

    async fn exec_delegate(&self, input: &Value, claude: bool) -> ToolOutcome {
        let prompt = match input.get("prompt").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return ToolOutcome { content: "delegate: missing 'prompt'".into(), is_error: true },
        };
        let cwd = input.get("cwd").and_then(|v| v.as_str()).map(String::from);
        let timeout_s = input.get("timeout_s").and_then(|v| v.as_u64()).unwrap_or(1800);

        let res = if claude {
            run_claude(&self.cfg, &prompt, cwd.as_deref(), "bypassPermissions", timeout_s).await
        } else {
            run_codex(&self.cfg, &prompt, cwd.as_deref(), timeout_s).await
        };
        match res {
            Ok(r) => {
                let summary = summarise_subagent(&r);
                let is_error = r.returncode != 0 || r.timed_out;
                ToolOutcome { content: summary, is_error }
            }
            Err(e) => ToolOutcome { content: format!("delegate error: {e}"), is_error: true },
        }
    }

    // ---------------- skills ----------------

    async fn exec_list_skills(&self) -> ToolOutcome {
        let listing = self.skills.list_for_prompt();
        if listing.is_empty() {
            ToolOutcome { content: "no skills loaded".into(), is_error: false }
        } else {
            ToolOutcome { content: listing, is_error: false }
        }
    }

    async fn exec_load_skill(&self, input: &Value) -> ToolOutcome {
        let name = match input.get("name").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => return ToolOutcome { content: "load_skill: missing 'name'".into(), is_error: true },
        };
        // Bump usage even for misses — gives the curator a signal about
        // typo'd or stale references.
        self.skills.bump_use(&name);
        match self.skills.body(&name) {
            Some(body) => ToolOutcome { content: body, is_error: false },
            None => {
                let avail = self.skills.names().join(", ");
                ToolOutcome { content: format!("no skill named '{name}'. available: {avail}"), is_error: true }
            }
        }
    }

    // ---------------- audit log ----------------

    async fn audit(&self, name: &str, input: &Value, outcome: &ToolOutcome, elapsed_ms: u64) {
        let _g = self.audit_lock.lock().await;
        if let Some(parent) = self.audit_log.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let preview: String = outcome.content.chars().take(800).collect();
        let rec = json!({
            "ts": Local::now().to_rfc3339(),
            "tool": name,
            "input": input,
            "ok": !outcome.is_error,
            "elapsed_ms": elapsed_ms,
            "output_preview": preview,
        });
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&self.audit_log) {
            let _ = writeln!(f, "{}", rec);
        }
        tracing::info!(tool = %name, ok = !outcome.is_error, elapsed_ms, "tool call");
    }
}

/// Strip HTML to reading-grade text. Removes script/style/svg/etc blocks and
/// HTML tags, decodes the common entities, collapses whitespace. Not a full
/// markdown converter — intentional: cheap, dependency-free, good enough for
/// "what does this page say?" use cases. For richer extraction, use a real
/// crate (e.g. `readability` or `html2text`) if it becomes a recurring need.
///
/// NOTE: Rust's `regex` crate does **not** support back-references, so each
/// blocked tag gets its own pattern instead of `</\\1>`. (Earlier attempts
/// with `<(script|style|...)>...</\\1>` panicked at Regex::new and killed the
/// daemon — Codex would call that a HIGH severity bug; let's not regress.)
fn html_to_text(html: &str) -> String {
    use once_cell::sync::Lazy;
    use regex::Regex;
    static DROP_SCRIPT:    Lazy<Regex> = Lazy::new(|| Regex::new(r"(?is)<script[^>]*>.*?</script\s*>").unwrap());
    static DROP_STYLE:     Lazy<Regex> = Lazy::new(|| Regex::new(r"(?is)<style[^>]*>.*?</style\s*>").unwrap());
    static DROP_NOSCRIPT:  Lazy<Regex> = Lazy::new(|| Regex::new(r"(?is)<noscript[^>]*>.*?</noscript\s*>").unwrap());
    static DROP_SVG:       Lazy<Regex> = Lazy::new(|| Regex::new(r"(?is)<svg[^>]*>.*?</svg\s*>").unwrap());
    static DROP_IFRAME:    Lazy<Regex> = Lazy::new(|| Regex::new(r"(?is)<iframe[^>]*>.*?</iframe\s*>").unwrap());
    static DROP_HEAD:      Lazy<Regex> = Lazy::new(|| Regex::new(r"(?is)<head[^>]*>.*?</head\s*>").unwrap());
    static STRIP_TAGS:     Lazy<Regex> = Lazy::new(|| Regex::new(r"(?is)<[^>]+>").unwrap());
    static COLLAPSE_WS:    Lazy<Regex> = Lazy::new(|| Regex::new(r"[ \t]+").unwrap());
    static COLLAPSE_NL:    Lazy<Regex> = Lazy::new(|| Regex::new(r"\n{3,}").unwrap());

    let s = DROP_SCRIPT.replace_all(html, "");
    let s = DROP_STYLE.replace_all(&s, "");
    let s = DROP_NOSCRIPT.replace_all(&s, "");
    let s = DROP_SVG.replace_all(&s, "");
    let s = DROP_IFRAME.replace_all(&s, "");
    let s = DROP_HEAD.replace_all(&s, "");
    let s = STRIP_TAGS.replace_all(&s, "\n");
    // Decode the handful of entities that matter.
    let s = s.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&hellip;", "…")
        .replace("&mdash;", "—")
        .replace("&ndash;", "–")
        .replace("&copy;", "©")
        .replace("&reg;", "®");
    let s = COLLAPSE_WS.replace_all(&s, " ");
    let s = COLLAPSE_NL.replace_all(&s, "\n\n");
    // Trim leading/trailing whitespace per line for readability.
    s.lines()
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn canonical_or_self(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

/// Lex-normalise a path on Windows: strip `\\?\` UNC prefix and lowercase the
/// drive letter so two canonical forms compare equal. Component structure is
/// preserved (no slash-flattening), unlike the previous version of this helper.
fn lex_normalise(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    let stripped = s.strip_prefix(r"\\?\").unwrap_or(&s);
    // Lowercase the drive letter (Windows is case-insensitive on volumes).
    let lowered: String = if stripped.len() >= 2 && stripped.chars().nth(1) == Some(':') {
        let c0 = stripped.chars().next().unwrap().to_ascii_lowercase();
        let mut out = String::with_capacity(stripped.len());
        out.push(c0);
        out.push_str(&stripped[1..]);
        out
    } else {
        stripped.to_string()
    };
    PathBuf::from(lowered)
}

/// Returns true iff `candidate` is the same path as `base` (when `as_file` is true)
/// or a descendant of `base` (when `as_file` is false). Component-aware — does NOT
/// confuse `/foo/bar` with `/foo/bar-evil`.
fn path_is_inside(candidate: &Path, base: &Path, as_file: bool) -> bool {
    let c = lex_normalise(&canonical_or_self(candidate));
    let b = lex_normalise(&canonical_or_self(base));
    if as_file {
        // Case-insensitive equality on Windows.
        c.as_os_str().eq_ignore_ascii_case(b.as_os_str())
    } else {
        // Strip-prefix is component-aware: `/foo/bar` does not match `/foo/bar-evil`.
        match c.strip_prefix(&b) {
            Ok(_) => true,
            Err(_) => {
                // Fall back to case-insensitive component compare for Windows quirks.
                let cl = c.as_os_str().to_ascii_lowercase();
                let bl = b.as_os_str().to_ascii_lowercase();
                Path::new(&cl).strip_prefix(Path::new(&bl)).is_ok()
            }
        }
    }
}

/// Component-aware sandbox check. If `base_is_file` is true, only an exact match
/// counts — used for allow-list entries that are individual files (e.g.
/// `widgets.json`) so that `widgets.json.bak` is *not* whitelisted. Otherwise
/// any descendant of `base` matches. Replaces the old string-prefix
/// `starts_with` helper which mis-matched siblings like `data` vs `data-evil`.
fn is_within(candidate: &Path, base: &Path, base_is_file: bool) -> bool {
    path_is_inside(candidate, base, base_is_file)
}

/// Convert Git-Bash-style paths (`/c/Users/...`) to native Windows form
/// (`C:/Users/...`) so they're recognised as absolute. Pass-through for
/// already-Windows or relative paths.
fn normalize_input_path(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix('/') {
        if let Some((drive, after)) = rest.split_once('/') {
            if drive.len() == 1 && drive.chars().next().unwrap().is_ascii_alphabetic() {
                let upper = drive.to_ascii_uppercase();
                return PathBuf::from(format!("{}:/{}", upper, after));
            }
        }
    }
    PathBuf::from(raw)
}
