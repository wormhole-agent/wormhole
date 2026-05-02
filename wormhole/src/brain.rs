//! Brain: builds context, calls providers (with optional tool loop), persists transcript.

use crate::config::{Config, MAX_ITERATIONS_HARD_CEILING};
use crate::error::{LarryError, Result};
use crate::memory::{append_daily, build_system_prompt};
use crate::providers::{
    build_providers, ChatMessage, CompletionResult, ContentBlock, MessageContent, Provider,
    ToolDef,
};
use crate::skills::SkillRegistry;
use crate::tools::ToolRegistry;
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Turn {
    pub role: String,
    pub content: String,
    pub ts: String,
    pub provider: Option<String>,
    pub model: Option<String>,
}

pub struct Brain {
    cfg: Arc<Config>,
    providers: BTreeMap<String, Arc<dyn Provider>>,
    skills: Arc<SkillRegistry>,
    tools: Option<Arc<ToolRegistry>>,
    history: Mutex<HashMap<String, VecDeque<Turn>>>,
    /// Sessions that have already been hydrated from JSONL transcripts on disk.
    /// Lazily populated on first message into a given session_id, so we don't
    /// pay the disk cost up front for sessions the user never re-enters today.
    loaded_sessions: Mutex<HashSet<String>>,
    system_cache: Mutex<(String, std::time::Instant)>,
}

#[derive(Clone, Debug, Default)]
pub struct RespondOpts<'a> {
    pub source: &'a str,
    pub provider_override: Option<&'a str>,
    pub model_override: Option<&'a str>,
    pub extra_system: &'a str,
    /// If false, tools are disabled for this call. Currently every caller
    /// (UI, telegram, cron prompt jobs) sets `true`; the flag exists so a
    /// future caller can opt out without changing the brain's signature.
    pub allow_tools: bool,
    /// Override the global tool-loop iteration cap for this call. `None` =
    /// use ToolConfig.max_iterations. Per-job knob plumbed through from
    /// cron.toml's `[[job]] max_iterations`.
    pub max_iterations_override: Option<u32>,
    /// When the tool loop hits its iteration cap and this flag is true, the
    /// brain auto-retries once with a 50% larger cap (clamped to
    /// `MAX_ITERATIONS_HARD_CEILING`). Cron jobs set this; interactive
    /// Telegram/CLI sessions leave it false so the human can decide whether
    /// to keep going.
    pub auto_retry_on_cap: bool,
}

impl Brain {
    pub fn new(cfg: Arc<Config>) -> Result<Self> {
        let providers = build_providers(&cfg);
        if providers.is_empty() {
            return Err(LarryError::Config(
                "no enabled providers — check config/credentials".into(),
            ));
        }
        let skills = SkillRegistry::load(&cfg)?;
        let tools = if cfg.tools.enabled {
            Some(Arc::new(ToolRegistry::new(
                cfg.clone(),
                cfg.tools.clone(),
                skills.clone(),
            )))
        } else {
            None
        };
        Ok(Self {
            cfg,
            providers,
            skills,
            tools,
            history: Mutex::new(HashMap::new()),
            loaded_sessions: Mutex::new(HashSet::new()),
            system_cache: Mutex::new((
                String::new(),
                // Don't subtract: on Windows, Instant is QPC-based and can be
                // smaller than 120s (e.g., shortly after boot or under certain
                // launcher restart cycles), making the subtraction overflow
                // and abort the daemon. The cache hit path also checks
                // `cache.0.is_empty()` first, so the timestamp here only
                // matters once content is populated.
                std::time::Instant::now(),
            )),
        })
    }

    pub fn list_providers(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    pub fn provider_default_model(&self, name: &str) -> Option<String> {
        self.providers.get(name).map(|p| p.default_model().to_string())
    }

    pub fn skills(&self) -> &Arc<SkillRegistry> {
        &self.skills
    }

    pub fn tools_enabled(&self) -> bool {
        self.tools.is_some()
    }

    /// Surface the effective tool policy in a shape the UI / editing agents
    /// can read. Codex review #10. Lets a future agent see allowed write roots
    /// before proposing a tool that wants to write somewhere new — without
    /// having to spelunk config.toml + tools.rs to figure it out.
    pub fn tool_policy(&self) -> serde_json::Value {
        let t = &self.cfg.tools;
        // read_file allow-list: prefer the configured list; fall back to the
        // hardcoded defaults if not set. Mirrors tools.rs::resolve_read_path.
        let read_roots: Vec<String> = if t.read_file_allowed_paths.is_empty() {
            let mut roots = vec![
                self.cfg.workspace_root.display().to_string(),
                self.cfg.larry_home.display().to_string(),
            ];
            if let Some(home) = dirs::home_dir() {
                roots.push(home.join("openbrain").join("data").display().to_string());
            }
            roots
        } else {
            t.read_file_allowed_paths.iter().map(|p| p.display().to_string()).collect()
        };
        let write_roots: Vec<String> = if t.write_file_allowed_paths.is_empty() {
            // Defaults — match what tools.rs falls back to if the list is empty.
            vec![
                self.cfg.skills_dir.display().to_string(),
                self.cfg.sessions_dir.display().to_string(),
                self.cfg.larry_home.join("scratch").display().to_string(),
                self.cfg.workspace_root.join("memory").display().to_string(),
                self.cfg.workspace_root.join("_scratch").display().to_string(),
            ]
        } else {
            t.write_file_allowed_paths.iter().map(|p| p.display().to_string()).collect()
        };
        serde_json::json!({
            "enabled": t.enabled,
            "max_iterations": t.max_iterations,
            "shell": {
                "enabled": t.shell_enabled,
                "block_pattern_count": t.shell_block_patterns.len(),
            },
            "read_file": {
                "allowed_roots": read_roots,
            },
            "write_file": {
                "allowed_paths": write_roots,
                "note": "entries with a file extension are file-mode (exact match); entries without are dir-mode (descendants allowed)",
            },
            "http_get": {
                "blocked_hosts": t.http_get_blocked_hosts,
            },
            "delegate": {
                "claude_path": self.cfg.delegate_claude_path,
                "codex_path":  self.cfg.delegate_codex_path,
            },
        })
    }

    pub async fn respond(
        &self,
        user_text: &str,
        session_id: &str,
        opts: RespondOpts<'_>,
    ) -> Result<CompletionResult> {
        let chain = self.chain(opts.provider_override);
        if chain.is_empty() {
            return Err(LarryError::Permanent(
                "no provider in fallback chain matched configured providers".into(),
            ));
        }

        let mut messages = self.build_messages(session_id, user_text).await;
        let base_system = self.build_system(opts.extra_system).await;

        // Decide if tools are usable for the *first* call. The model only sees them
        // when the routed provider supports tools.
        let allow_tools = opts.allow_tools && self.tools.is_some();
        let tool_defs = if allow_tools {
            self.tools.as_ref().map(|t| t.tool_defs())
        } else {
            None
        };
        let tools_for_call: Option<&[ToolDef]> =
            tool_defs.as_deref().filter(|_| allow_tools);

        // Text-mode tool prompt: appended to the system prompt only for providers
        // that returned `supports_tools() == false`. This is the fallback path
        // for models that can't emit structured tool_calls (older Ollama models,
        // some OpenAI-compat endpoints). The brain parses `<tool_call>` XML out
        // of the response text further down.
        let text_mode_system = tool_defs
            .as_ref()
            .map(|defs| format!("{base_system}\n\n{}", text_mode_tools_prompt(defs)));

        // First call: try fallback chain. Native providers see tool_defs;
        // text-mode providers see the augmented system prompt instead.
        let mut last_err: Option<LarryError> = None;
        let mut result_opt: Option<CompletionResult> = None;
        let mut used_text_mode = false;
        for (idx, name) in chain.iter().enumerate() {
            let Some(prov) = self.providers.get(name) else { continue };
            let model = if Some(name.as_str()) == opts.provider_override {
                opts.model_override
            } else {
                None
            };
            let native_tools = prov.supports_tools();
            let tools_arg = if native_tools { tools_for_call } else { None };
            let sys = if !native_tools && allow_tools && text_mode_system.is_some() {
                text_mode_system.as_deref().unwrap_or(&base_system)
            } else {
                &base_system
            };
            match prov
                .complete(&messages, model, Some(sys), self.cfg.max_tokens, tools_arg)
                .await
            {
                Ok(r) => {
                    used_text_mode = !native_tools && allow_tools;
                    result_opt = Some(r);
                    break;
                }
                Err(e) => {
                    tracing::warn!(provider = %name, error = %e, "provider failed (try {}/{}) — fallback", idx + 1, chain.len());
                    last_err = Some(e);
                }
            }
        }
        let mut result = match result_opt {
            Some(r) => r,
            None => {
                return Err(last_err.unwrap_or_else(|| {
                    LarryError::Permanent("no providers attempted".into())
                }));
            }
        };

        // For text-mode providers, parse `<tool_call>...</tool_call>` blocks out
        // of the text response and convert them into ToolUse blocks. The brain's
        // tool loop below then runs them like any other tool_use.
        if used_text_mode && !has_tool_uses(&result) {
            let parsed = extract_text_tool_calls(&result.text);
            if !parsed.is_empty() {
                tracing::info!(count = parsed.len(), "text-mode: parsed inline tool calls");
                let cleaned_text = strip_tool_call_xml(&result.text);
                let mut new_blocks: Vec<ContentBlock> = Vec::new();
                if !cleaned_text.is_empty() {
                    new_blocks.push(ContentBlock::Text { text: cleaned_text.clone() });
                }
                new_blocks.extend(parsed);
                result.blocks = new_blocks;
                result.text = cleaned_text;
            }
        }

        // If the response didn't request tools, finalize.
        if !has_tool_uses(&result) {
            self.finalize_turn(session_id, user_text, &result, opts.source)
                .await;
            return Ok(result);
        }

        // Tool loop. Pin to the same provider as the first response (whether it
        // ran in native or text mode).
        let prov_name = result.provider.clone();
        let Some(prov) = self.providers.get(&prov_name).cloned() else {
            return Err(LarryError::Permanent(format!(
                "tool loop: provider {prov_name} no longer available"
            )));
        };
        let tools_reg = self
            .tools
            .as_ref()
            .ok_or_else(|| LarryError::Permanent("tool loop: tools disabled".into()))?
            .clone();
        let global_max = tools_reg.max_iterations();
        // Per-job override from cron.toml's `[[job]] max_iterations`. Clamped
        // to the hard ceiling so a typo can't run away with provider budget.
        let initial_max: u32 = opts
            .max_iterations_override
            .unwrap_or(global_max)
            .min(MAX_ITERATIONS_HARD_CEILING);
        let mut max_iter: u32 = initial_max;
        let mut retried = false;
        let mut iter: u32 = 0;
        let loop_native = prov.supports_tools();
        let loop_system: &str = if loop_native {
            &base_system
        } else {
            text_mode_system.as_deref().unwrap_or(&base_system)
        };
        let loop_tools = if loop_native { tool_defs.as_deref() } else { None };

        let hit_cap = loop {
            iter += 1;
            if iter > max_iter {
                // Out of iterations. Cron callers get one auto-retry with a
                // 50% larger cap (clamped to MAX_ITERATIONS_HARD_CEILING).
                // Interactive sessions fall through to the partial-response
                // path so the human can decide whether to push on.
                if opts.auto_retry_on_cap && !retried {
                    let bumped = ((max_iter as u64) * 3 / 2)
                        .min(MAX_ITERATIONS_HARD_CEILING as u64) as u32;
                    if bumped > max_iter {
                        tracing::warn!(
                            session = %session_id,
                            previous_max = max_iter,
                            new_max = bumped,
                            "tool loop hit cap on cron run; auto-retrying with higher cap",
                        );
                        max_iter = bumped;
                        retried = true;
                        // Note: don't reset `iter` — we keep going from where
                        // we were, so the conversation context is preserved.
                        continue;
                    }
                }
                break true;
            }

            // Append assistant message with the original block list (text + tool_use).
            messages.push(ChatMessage::assistant_blocks(result.blocks.clone()));

            // Execute every tool_use block in parallel. Tools are independent —
            // the model issued them as a batch — so there's no reason to await
            // them serially. Order is preserved when collecting results.
            let tool_calls: Vec<(String, String, serde_json::Value)> = result
                .blocks
                .iter()
                .filter_map(|b| {
                    if let ContentBlock::ToolUse { id, name, input } = b {
                        Some((id.clone(), name.clone(), input.clone()))
                    } else {
                        None
                    }
                })
                .collect();
            let outcomes = futures::future::join_all(tool_calls.into_iter().map(
                |(id, name, input)| {
                    let tr = tools_reg.clone();
                    async move {
                        let outcome = tr.execute(&name, &input).await;
                        (id, outcome)
                    }
                },
            ))
            .await;
            let tool_result_blocks: Vec<ContentBlock> = outcomes
                .into_iter()
                .map(|(id, outcome)| ContentBlock::ToolResult {
                    tool_use_id: id,
                    content: outcome.content,
                    is_error: outcome.is_error,
                })
                .collect();
            messages.push(ChatMessage::user_blocks(tool_result_blocks));

            // Re-call.
            result = prov
                .complete(
                    &messages,
                    None,
                    Some(loop_system),
                    self.cfg.max_tokens,
                    loop_tools,
                )
                .await?;

            // Text-mode: parse `<tool_call>` XML out of the text response so the
            // outer predicate sees ToolUse blocks like a native call would.
            if !loop_native && !has_tool_uses(&result) {
                let parsed = extract_text_tool_calls(&result.text);
                if !parsed.is_empty() {
                    let cleaned = strip_tool_call_xml(&result.text);
                    let mut new_blocks: Vec<ContentBlock> = Vec::new();
                    if !cleaned.is_empty() {
                        new_blocks.push(ContentBlock::Text { text: cleaned.clone() });
                    }
                    new_blocks.extend(parsed);
                    result.blocks = new_blocks;
                    result.text = cleaned;
                }
            }

            if !has_tool_uses(&result) {
                self.finalize_turn(session_id, user_text, &result, opts.source)
                    .await;
                return Ok(result);
            }

            tracing::info!(iter, "tool loop iteration {iter}/{max_iter}");
        };

        if hit_cap {
            let suffix = if retried { " (after retry)" } else { "" };
            let warn = format!(
                "\n\n[wormhole: tool loop hit max_iterations={max_iter}{suffix}; returning partial response]"
            );
            result.text.push_str(&warn);
        }
        self.finalize_turn(session_id, user_text, &result, opts.source)
            .await;
        Ok(result)
    }

    async fn build_system(&self, extra: &str) -> String {
        let ttl = std::time::Duration::from_secs(60);
        let now = std::time::Instant::now();
        let mut cache = self.system_cache.lock().await;
        if cache.0.is_empty() || now.duration_since(cache.1) > ttl || !extra.is_empty() {
            let base = build_system_prompt(&self.cfg, extra);
            let result = if let Some(skills_section) = self.skills.system_section() {
                format!("{base}\n\n{skills_section}")
            } else {
                base
            };
            if extra.is_empty() {
                cache.0 = result.clone();
                cache.1 = now;
            }
            result
        } else {
            cache.0.clone()
        }
    }

    fn chain(&self, override_name: Option<&str>) -> Vec<String> {
        let mut chain: Vec<String> = Vec::new();
        if let Some(n) = override_name {
            chain.push(n.to_string());
        }
        chain.push(self.cfg.default_provider.clone());
        for f in &self.cfg.fallback_chain {
            if !chain.iter().any(|c| c == f) {
                chain.push(f.clone());
            }
        }
        chain
            .into_iter()
            .filter(|n| self.providers.contains_key(n))
            .collect()
    }

    async fn build_messages(&self, session_id: &str, user_text: &str) -> Vec<ChatMessage> {
        // Lazy hydrate: on first sight of a session_id this process has seen,
        // pull the most recent N turns from the JSONL transcripts on disk so
        // conversation continuity survives restarts. After hydration the
        // in-memory VecDeque is the source of truth.
        let needs_hydrate = {
            let mut loaded = self.loaded_sessions.lock().await;
            if loaded.contains(session_id) {
                false
            } else {
                loaded.insert(session_id.to_string());
                true
            }
        };
        if needs_hydrate {
            let in_mem_empty = {
                let h = self.history.lock().await;
                h.get(session_id).map(|d| d.is_empty()).unwrap_or(true)
            };
            if in_mem_empty {
                let turns = load_session_history(
                    &self.cfg.sessions_dir,
                    session_id,
                    self.cfg.history_turns,
                );
                if !turns.is_empty() {
                    tracing::info!(
                        session = %session_id,
                        turns = turns.len(),
                        "hydrated session history from disk",
                    );
                    let mut h = self.history.lock().await;
                    let dq = h.entry(session_id.to_string()).or_default();
                    for t in turns {
                        dq.push_back(t);
                    }
                }
            }
        }

        let h = self.history.lock().await;
        let history = h.get(session_id).cloned().unwrap_or_default();
        drop(h);
        let keep = self.cfg.history_turns.saturating_mul(2);
        let history: Vec<&Turn> = history.iter().rev().take(keep).collect();
        let mut msgs: Vec<ChatMessage> = history
            .into_iter()
            .rev()
            .map(|t| ChatMessage {
                role: t.role.clone(),
                content: MessageContent::Text(t.content.clone()),
            })
            .collect();
        msgs.push(ChatMessage::user(user_text));
        msgs
    }

    async fn record_turn(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        provider: Option<String>,
        model: Option<String>,
    ) {
        let mut h = self.history.lock().await;
        let dq = h.entry(session_id.to_string()).or_default();
        dq.push_back(Turn {
            role: role.into(),
            content: content.into(),
            ts: Local::now().to_rfc3339(),
            provider,
            model,
        });
        while dq.len() > 200 {
            dq.pop_front();
        }
    }

    async fn finalize_turn(
        &self,
        session_id: &str,
        user_text: &str,
        result: &CompletionResult,
        source: &str,
    ) {
        self.record_turn(session_id, "user", user_text, None, None).await;
        self.record_turn(
            session_id,
            "assistant",
            &result.text,
            Some(result.provider.clone()),
            Some(result.model.clone()),
        )
        .await;
        if source != "ui" {
            if let Err(e) = append_daily(&self.cfg, "user", user_text, source) {
                tracing::warn!(error=%e, session=%session_id, "append_daily(user) failed");
            }
            if let Err(e) = append_daily(
                &self.cfg,
                &format!("assistant ({}/{})", result.provider, result.model),
                &result.text,
                source,
            ) {
                tracing::warn!(error=%e, session=%session_id, "append_daily(assistant) failed");
            }
        }
        if let Err(e) = self.write_transcript(session_id, user_text, result, source) {
            tracing::warn!(error=%e, session=%session_id, "write_transcript failed");
        }
        tracing::info!(
            session = %session_id,
            source = %source,
            provider = %result.provider,
            model = %result.model,
            in_tokens = result.input_tokens,
            out_tokens = result.output_tokens,
            elapsed_ms = result.elapsed_ms,
            "ok"
        );
    }

    fn write_transcript(
        &self,
        session_id: &str,
        user_text: &str,
        result: &CompletionResult,
        source: &str,
    ) -> Result<()> {
        let safe_id: String = session_id
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .take(80)
            .collect();
        let day = Local::now().date_naive().to_string();
        let path: PathBuf = self.cfg.sessions_dir.join(format!("{day}__{safe_id}.jsonl"));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let rec = serde_json::json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "ts": Local::now().to_rfc3339(),
            "session_id": session_id,
            "source": source,
            "user": user_text,
            "assistant": result.text,
            "provider": result.provider,
            "model": result.model,
            "input_tokens": result.input_tokens,
            "output_tokens": result.output_tokens,
            "elapsed_ms": result.elapsed_ms,
        });
        let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
        writeln!(f, "{}", rec)?;
        Ok(())
    }
}

fn has_tool_uses(r: &CompletionResult) -> bool {
    r.blocks.iter().any(|b| matches!(b, ContentBlock::ToolUse { .. }))
}

/// Read up to `turns` recent user+assistant pairs (so 2*turns Turn entries) from
/// the session's JSONL transcripts. Files are named `<date>__<session_id>.jsonl`
/// — we scan the sessions dir, find every file ending in `__<safe_id>.jsonl`,
/// sort by date descending, and read newest→oldest until we've collected enough
/// turns. Quietly returns empty on missing dir / read errors.
pub(crate) fn load_session_history(
    sessions_dir: &std::path::Path,
    session_id: &str,
    turns: usize,
) -> Vec<Turn> {
    let safe_id: String = session_id
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .take(80)
        .collect();
    let suffix = format!("__{safe_id}.jsonl");

    let read_dir = match fs::read_dir(sessions_dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };
    let mut matches: Vec<PathBuf> = read_dir
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let name = p.file_name()?.to_str()?.to_string();
            if name.ends_with(&suffix) {
                Some(p)
            } else {
                None
            }
        })
        .collect();
    // Filenames embed the date in the leading `YYYY-MM-DD__` so a string sort
    // == date sort. Newest last.
    matches.sort();

    // Walk newest → oldest, collecting turns until we have 2*turns entries.
    let target = turns.saturating_mul(2);
    let mut acc: Vec<Turn> = Vec::new();
    for path in matches.iter().rev() {
        let f = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let reader = BufReader::new(f);
        let mut file_turns: Vec<Turn> = Vec::new();
        for line in reader.lines().map_while(|r| r.ok()) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let v: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let user = v.get("user").and_then(|x| x.as_str()).unwrap_or("");
            let assistant = v.get("assistant").and_then(|x| x.as_str()).unwrap_or("");
            let ts = v
                .get("ts")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let provider = v.get("provider").and_then(|x| x.as_str()).map(String::from);
            let model = v.get("model").and_then(|x| x.as_str()).map(String::from);
            if !user.is_empty() {
                file_turns.push(Turn {
                    role: "user".into(),
                    content: user.to_string(),
                    ts: ts.clone(),
                    provider: None,
                    model: None,
                });
            }
            if !assistant.is_empty() {
                file_turns.push(Turn {
                    role: "assistant".into(),
                    content: assistant.to_string(),
                    ts,
                    provider,
                    model,
                });
            }
        }
        // file_turns are in chronological order (oldest first). We want the
        // most recent `target` turns, so prepend in reverse.
        for t in file_turns.into_iter().rev() {
            acc.push(t);
            if acc.len() >= target {
                break;
            }
        }
        if acc.len() >= target {
            break;
        }
    }
    // `acc` is newest → oldest; flip back to chronological for the brain.
    acc.reverse();
    acc
}

/// System-prompt addendum that teaches a tool-blind model the XML protocol the
/// brain parses. Only used when the routed provider returned
/// `supports_tools() == false` — i.e. text-mode tool calling.
///
/// The model is told to emit zero or more `<tool_call name="X"><input>{...}</input></tool_call>`
/// blocks anywhere in its reply. The brain strips them out, runs the tool, and
/// feeds the result back as a normal user-block response.
fn text_mode_tools_prompt(defs: &[ToolDef]) -> String {
    let mut s = String::new();
    s.push_str("# TOOLS (text-mode protocol)\n\n");
    s.push_str(
        "You can call tools by emitting one or more XML blocks of this exact form, \
         anywhere in your reply:\n\n\
         <tool_call name=\"<tool_name>\"><input>{...JSON...}</input></tool_call>\n\n\
         Rules:\n\
         - The body inside <input>...</input> MUST be a valid JSON object matching the tool's schema.\n\
         - You may emit multiple <tool_call> blocks in one reply; they will all run in parallel.\n\
         - When tool results come back as the next user message, decide whether to call more tools \
         or produce your final answer (with no <tool_call> blocks).\n\
         - Do not pretend to run tools. Do not invent results. If you don't need a tool, just answer.\n\n\
         Available tools:\n\n",
    );
    for d in defs {
        let schema = serde_json::to_string(&d.input_schema)
            .unwrap_or_else(|_| "{}".into());
        s.push_str(&format!("## {}\n{}\n  schema: {}\n\n", d.name, d.description, schema));
    }
    s
}

/// Extract every `<tool_call name="X"><input>...</input></tool_call>` block from
/// model output and return them as ToolUse blocks. Tolerant of whitespace and
/// missing `<input>` wrappers — many local models forget the inner tag.
pub(crate) fn extract_text_tool_calls(text: &str) -> Vec<ContentBlock> {
    let mut out: Vec<ContentBlock> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let Some(rel_start) = text[i..].find("<tool_call") else { break };
        let start = i + rel_start;
        let Some(rel_end) = text[start..].find("</tool_call>") else { break };
        let end = start + rel_end + "</tool_call>".len();
        let block = &text[start..end];

        let name = extract_xml_attr(block, "name").unwrap_or_default();
        if name.is_empty() {
            i = end;
            continue;
        }
        // Body between the opening tag's `>` and the closing `</tool_call>`.
        let body_start = match block.find('>') {
            Some(p) => p + 1,
            None => { i = end; continue; }
        };
        let body_end = block.len() - "</tool_call>".len();
        let body = &block[body_start..body_end];
        let json_text = if let (Some(a), Some(b)) = (body.find("<input>"), body.find("</input>")) {
            &body[a + "<input>".len()..b]
        } else {
            body
        };
        let json_text = json_text.trim();
        let input: serde_json::Value = if json_text.is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str(json_text)
                .unwrap_or_else(|_| serde_json::json!({ "_raw": json_text }))
        };
        out.push(ContentBlock::ToolUse {
            id: format!("call_{}", uuid::Uuid::new_v4().simple()),
            name,
            input,
        });
        i = end;
    }
    out
}

fn extract_xml_attr(tag: &str, attr: &str) -> Option<String> {
    // Look for `attr="value"` or `attr='value'`.
    let idx = tag.find(&format!("{attr}=\""))
        .or_else(|| tag.find(&format!("{attr}='")));
    let idx = idx?;
    let quote = tag.as_bytes().get(idx + attr.len() + 1).copied()?;
    let after = idx + attr.len() + 2;
    let close = tag[after..].find(quote as char)?;
    Some(tag[after..after + close].to_string())
}

#[cfg(test)]
mod text_mode_tests {
    use super::*;

    #[test]
    fn parse_single_tool_call() {
        let raw = r#"sure thing<tool_call name="shell"><input>{"command": "ls"}</input></tool_call>"#;
        let blocks = extract_text_tool_calls(raw);
        assert_eq!(blocks.len(), 1);
        if let ContentBlock::ToolUse { name, input, .. } = &blocks[0] {
            assert_eq!(name, "shell");
            assert_eq!(input.get("command").and_then(|v| v.as_str()), Some("ls"));
        } else {
            panic!("expected ToolUse, got {:?}", blocks[0]);
        }
    }

    #[test]
    fn parse_multiple_tool_calls() {
        let raw = r#"<tool_call name="a"><input>{}</input></tool_call> some text <tool_call name="b"><input>{"k":1}</input></tool_call>"#;
        let blocks = extract_text_tool_calls(raw);
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn parse_without_input_wrapper() {
        let raw = r#"<tool_call name="x">{"q": 7}</tool_call>"#;
        let blocks = extract_text_tool_calls(raw);
        assert_eq!(blocks.len(), 1);
        if let ContentBlock::ToolUse { name, input, .. } = &blocks[0] {
            assert_eq!(name, "x");
            assert_eq!(input.get("q").and_then(|v| v.as_i64()), Some(7));
        } else {
            panic!("expected ToolUse");
        }
    }

    #[test]
    fn parse_no_tool_calls() {
        assert!(extract_text_tool_calls("just plain text").is_empty());
    }

    #[test]
    fn strip_xml_round_trip() {
        let raw = r#"hi <tool_call name="z"><input>{}</input></tool_call> bye"#;
        assert_eq!(strip_tool_call_xml(raw), "hi  bye");
    }
}

/// Remove every `<tool_call ...>...</tool_call>` block from a string. Used to
/// avoid echoing the protocol back to the user when the model emits inline
/// tool calls.
pub(crate) fn strip_tool_call_xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while let Some(rel) = text[i..].find("<tool_call") {
        let start = i + rel;
        out.push_str(&text[i..start]);
        if let Some(rel_end) = text[start..].find("</tool_call>") {
            i = start + rel_end + "</tool_call>".len();
        } else {
            // No closing tag — bail out and keep the rest verbatim.
            i = start;
            out.push_str(&text[i..]);
            return out.trim().to_string();
        }
    }
    out.push_str(&text[i..]);
    out.trim().to_string()
}
