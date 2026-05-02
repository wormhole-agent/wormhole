use super::{ChatMessage, CompletionResult, ContentBlock, MessageContent, Provider, ToolDef};
use crate::config::{PromptCachingCfg, ProviderCfg};
use crate::error::{LarryError, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub struct AnthropicProvider {
    cfg: ProviderCfg,
    http: reqwest::Client,
    api_key: String,
    base: String,
    prompt_caching: PromptCachingCfg,
}

#[derive(Serialize)]
struct Req<'a> {
    model: &'a str,
    max_tokens: u32,
    messages: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Value>>,
}

#[derive(Deserialize)]
struct Resp {
    content: Vec<RespBlock>,
    #[serde(default)]
    usage: Usage,
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RespBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: Value },
    #[serde(other)]
    Other,
}

#[derive(Deserialize, Default)]
#[allow(dead_code)]
struct Usage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
}

impl AnthropicProvider {
    pub fn new(
        cfg: ProviderCfg,
        http: reqwest::Client,
        prompt_caching: PromptCachingCfg,
    ) -> Result<Self> {
        let api_key = cfg
            .api_key
            .clone()
            .ok_or_else(|| LarryError::Permanent("anthropic: no api key".into()))?;
        let base = cfg
            .base_url
            .clone()
            .unwrap_or_else(|| "https://api.anthropic.com".into());
        // Warn early on unrecognised TTL values so the user notices a typo
        // instead of silently shipping uncached requests.
        if prompt_caching.enabled
            && prompt_caching.cache_ttl != "ephemeral"
            && prompt_caching.cache_ttl != "1h"
        {
            tracing::warn!(
                ttl = %prompt_caching.cache_ttl,
                "anthropic: unknown cache_ttl, falling back to ephemeral"
            );
        }
        Ok(Self {
            cfg,
            http,
            api_key,
            base,
            prompt_caching,
        })
    }
}

fn message_to_value(msg: &ChatMessage) -> Value {
    match &msg.content {
        MessageContent::Text(t) => json!({ "role": msg.role, "content": t }),
        MessageContent::Blocks(blocks) => {
            let arr: Vec<Value> = blocks.iter().map(block_to_value).collect();
            json!({ "role": msg.role, "content": arr })
        }
    }
}

fn block_to_value(b: &ContentBlock) -> Value {
    match b {
        ContentBlock::Text { text } => json!({ "type": "text", "text": text }),
        ContentBlock::ToolUse { id, name, input } => {
            json!({ "type": "tool_use", "id": id, "name": name, "input": input })
        }
        ContentBlock::ToolResult { tool_use_id, content, is_error } => {
            let mut v = json!({
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": content,
            });
            if *is_error {
                v["is_error"] = json!(true);
            }
            v
        }
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }
    fn default_model(&self) -> &str {
        &self.cfg.default_model
    }
    fn supports_tools(&self) -> bool { true }

    async fn complete(
        &self,
        messages: &[ChatMessage],
        model: Option<&str>,
        system: Option<&str>,
        max_tokens: u32,
        tools: Option<&[ToolDef]>,
    ) -> Result<CompletionResult> {
        let url = format!("{}/v1/messages", self.base.trim_end_matches('/'));
        let model = model.unwrap_or(&self.cfg.default_model).to_string();

        let msgs: Vec<Value> = messages.iter().map(message_to_value).collect();

        let tools_payload: Option<Vec<Value>> = tools.map(|ts| {
            ts.iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "input_schema": t.input_schema,
                    })
                })
                .collect()
        });

        // Mark the system prompt as a cache breakpoint so identical-prefix calls
        // hit the cache (90% discount on those tokens). Anthropic silently
        // ignores cache_control if the block is too short to cache, so it's
        // safe to always set when caching is enabled.
        //
        // TODO: when cache_ttl == "1h", add the prompt-caching-2024-07-31 beta
        // header and emit `cache_control: { type: "ephemeral", ttl: "1h" }`.
        // Today only the 5 min ("ephemeral") path is wired through; "1h" falls
        // back to ephemeral with a warning at startup.
        let system_payload: Option<Vec<Value>> = system.map(|s| {
            if !self.prompt_caching.enabled {
                return vec![json!({ "type": "text", "text": s })];
            }
            vec![json!({
                "type": "text",
                "text": s,
                "cache_control": { "type": "ephemeral" }
            })]
        });

        let body = Req {
            model: &model,
            max_tokens,
            messages: msgs,
            system: system_payload,
            tools: tools_payload,
        };

        let t0 = Instant::now();
        let resp = self
            .http
            .post(&url)
            .timeout(Duration::from_secs(self.cfg.timeout_s))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LarryError::Transient(format!("anthropic http: {e}")))?;

        let status = resp.status();
        let raw = resp
            .text()
            .await
            .map_err(|e| LarryError::Transient(format!("anthropic body: {e}")))?;

        if !status.is_success() {
            if status.is_server_error() || status.as_u16() == 429 {
                return Err(LarryError::Transient(format!(
                    "anthropic {}: {}",
                    status,
                    raw.chars().take(300).collect::<String>()
                )));
            }
            return Err(LarryError::Permanent(format!(
                "anthropic {}: {}",
                status,
                raw.chars().take(300).collect::<String>()
            )));
        }

        let parsed: Resp = serde_json::from_str(&raw)
            .map_err(|e| LarryError::Permanent(format!("anthropic decode: {e}: {raw}")))?;

        let mut blocks: Vec<ContentBlock> = Vec::with_capacity(parsed.content.len());
        let mut text = String::new();
        for b in parsed.content {
            match b {
                RespBlock::Text { text: t } => {
                    text.push_str(&t);
                    blocks.push(ContentBlock::Text { text: t });
                }
                RespBlock::ToolUse { id, name, input } => {
                    blocks.push(ContentBlock::ToolUse { id, name, input });
                }
                RespBlock::Other => {}
            }
        }

        Ok(CompletionResult {
            text,
            blocks,
            stop_reason: parsed.stop_reason,
            provider: "anthropic".into(),
            model,
            input_tokens: parsed.usage.input_tokens,
            output_tokens: parsed.usage.output_tokens,
            elapsed_ms: t0.elapsed().as_millis() as u64,
        })
    }
}
