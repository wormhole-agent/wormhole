//! OpenAI-compatible chat-completions provider. Used for OpenAI + DeepSeek.
//!
//! Supports OpenAI-style function calling (`tools` request param +
//! `tool_calls` response field). Translates between Anthropic-style
//! `ContentBlock` round-trips and OpenAI's `role: "tool"` messages so the
//! brain's tool loop works against any of the three providers.

use super::{ChatMessage, CompletionResult, ContentBlock, MessageContent, Provider, ToolDef};
use crate::config::ProviderCfg;
use crate::error::{LarryError, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::time::{Duration, Instant};

pub struct OpenAICompatProvider {
    cfg: ProviderCfg,
    http: reqwest::Client,
    name: String,
    base: String,
    api_key: String,
}

#[derive(Deserialize)]
struct Resp {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Usage,
}

#[derive(Deserialize)]
struct Choice {
    message: RespMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
struct RespMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<RespToolCall>>,
}

#[derive(Deserialize)]
struct RespToolCall {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: RespFunction,
}

#[derive(Deserialize, Default)]
struct RespFunction {
    #[serde(default)]
    name: String,
    #[serde(default)]
    arguments: String,
}

#[derive(Deserialize, Default)]
struct Usage {
    #[serde(default)]
    prompt_tokens: u64,
    #[serde(default)]
    completion_tokens: u64,
}

impl OpenAICompatProvider {
    pub fn new(
        cfg: ProviderCfg,
        http: reqwest::Client,
        name: String,
        default_base: String,
    ) -> Result<Self> {
        let api_key = cfg
            .api_key
            .clone()
            .ok_or_else(|| LarryError::Permanent(format!("{name}: no api key")))?;
        let base = cfg.base_url.clone().unwrap_or(default_base);
        Ok(Self {
            cfg,
            http,
            name,
            base,
            api_key,
        })
    }

    fn token_param_for(&self, model: &str) -> &'static str {
        if self.name != "openai" {
            return "max_tokens";
        }
        let m = model.to_ascii_lowercase();
        if m.starts_with("gpt-5")
            || m.starts_with("o1")
            || m.starts_with("o3")
            || m.starts_with("o4")
        {
            "max_completion_tokens"
        } else {
            "max_tokens"
        }
    }
}

/// Convert one ChatMessage into one or more OpenAI-format messages.
///
/// Tool round-trips don't map 1:1: a single user message holding multiple
/// `ToolResult` blocks must be expanded into one `role: "tool"` message per
/// result. An assistant message with `ToolUse` blocks becomes a single message
/// whose `tool_calls` array carries each call.
fn message_to_openai(m: &ChatMessage) -> Vec<Value> {
    match &m.content {
        MessageContent::Text(t) => vec![json!({ "role": m.role, "content": t })],
        MessageContent::Blocks(blocks) => {
            let mut out: Vec<Value> = Vec::new();
            if m.role == "assistant" {
                let mut text_parts: Vec<String> = Vec::new();
                let mut tool_calls: Vec<Value> = Vec::new();
                for b in blocks {
                    match b {
                        ContentBlock::Text { text } => text_parts.push(text.clone()),
                        ContentBlock::ToolUse { id, name, input } => {
                            let args_str = serde_json::to_string(input)
                                .unwrap_or_else(|_| "{}".into());
                            tool_calls.push(json!({
                                "id": id,
                                "type": "function",
                                "function": { "name": name, "arguments": args_str }
                            }));
                        }
                        ContentBlock::ToolResult { .. } => {}
                    }
                }
                let mut msg = Map::new();
                msg.insert("role".into(), Value::String("assistant".into()));
                if text_parts.is_empty() {
                    msg.insert("content".into(), Value::Null);
                } else {
                    msg.insert("content".into(), Value::String(text_parts.join("\n")));
                }
                if !tool_calls.is_empty() {
                    msg.insert("tool_calls".into(), Value::Array(tool_calls));
                }
                out.push(Value::Object(msg));
            } else {
                let mut text_parts: Vec<String> = Vec::new();
                for b in blocks {
                    match b {
                        ContentBlock::Text { text } => text_parts.push(text.clone()),
                        ContentBlock::ToolResult { tool_use_id, content, .. } => {
                            out.push(json!({
                                "role": "tool",
                                "tool_call_id": tool_use_id,
                                "content": content,
                            }));
                        }
                        ContentBlock::ToolUse { .. } => {}
                    }
                }
                if !text_parts.is_empty() {
                    out.push(json!({ "role": m.role, "content": text_parts.join("\n") }));
                }
            }
            out
        }
    }
}

#[async_trait]
impl Provider for OpenAICompatProvider {
    fn name(&self) -> &str {
        &self.name
    }
    fn default_model(&self) -> &str {
        &self.cfg.default_model
    }
    fn supports_tools(&self) -> bool {
        // tools_style = "text" disables native function-calling on this provider
        // so brain.rs routes tools through system-prompt injection instead. That
        // covers OpenAI-compatible endpoints fronted by older / fine-tuned models
        // that haven't been trained to emit `tool_calls`.
        !self.cfg.tools_style.eq_ignore_ascii_case("text")
    }

    async fn complete(
        &self,
        messages: &[ChatMessage],
        model: Option<&str>,
        system: Option<&str>,
        max_tokens: u32,
        tools: Option<&[ToolDef]>,
    ) -> Result<CompletionResult> {
        let model = model.unwrap_or(&self.cfg.default_model).to_string();
        let url = format!("{}/chat/completions", self.base.trim_end_matches('/'));

        let mut payload_messages: Vec<Value> = Vec::with_capacity(messages.len() + 1);
        if let Some(sys) = system {
            payload_messages.push(json!({ "role": "system", "content": sys }));
        }
        for m in messages {
            payload_messages.extend(message_to_openai(m));
        }

        let mut body = Map::new();
        body.insert("model".into(), Value::String(model.clone()));
        body.insert("messages".into(), Value::Array(payload_messages));
        body.insert(
            self.token_param_for(&model).into(),
            Value::Number(max_tokens.into()),
        );

        if let Some(ts) = tools {
            if !ts.is_empty() {
                let tools_payload: Vec<Value> = ts
                    .iter()
                    .map(|t| {
                        json!({
                            "type": "function",
                            "function": {
                                "name": t.name,
                                "description": t.description,
                                "parameters": t.input_schema,
                            }
                        })
                    })
                    .collect();
                body.insert("tools".into(), Value::Array(tools_payload));
            }
        }

        let t0 = Instant::now();
        let resp = self
            .http
            .post(&url)
            .timeout(Duration::from_secs(self.cfg.timeout_s))
            .bearer_auth(&self.api_key)
            .header("content-type", "application/json")
            .body(serde_json::to_vec(&body)?)
            .send()
            .await
            .map_err(|e| LarryError::Transient(format!("{} http: {e}", self.name)))?;

        let status = resp.status();
        let raw = resp
            .text()
            .await
            .map_err(|e| LarryError::Transient(format!("{} body: {e}", self.name)))?;

        if !status.is_success() {
            if status.is_server_error() || status.as_u16() == 429 {
                return Err(LarryError::Transient(format!(
                    "{} {}: {}",
                    self.name,
                    status,
                    raw.chars().take(300).collect::<String>()
                )));
            }
            return Err(LarryError::Permanent(format!(
                "{} {}: {}",
                self.name,
                status,
                raw.chars().take(300).collect::<String>()
            )));
        }

        let parsed: Resp = serde_json::from_str(&raw)
            .map_err(|e| LarryError::Permanent(format!("{} decode: {e}: {raw}", self.name)))?;

        let choice = parsed.choices.into_iter().next();
        let stop_reason = choice.as_ref().and_then(|c| c.finish_reason.clone());
        let (text_out, tool_calls) = match choice {
            Some(c) => (
                c.message.content.unwrap_or_default(),
                c.message.tool_calls.unwrap_or_default(),
            ),
            None => (String::new(), Vec::new()),
        };

        let mut blocks: Vec<ContentBlock> = Vec::new();
        if !text_out.is_empty() {
            blocks.push(ContentBlock::Text { text: text_out.clone() });
        }
        for tc in tool_calls {
            let id = tc
                .id
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("call_{}", uuid::Uuid::new_v4().simple()));
            let input: Value = if tc.function.arguments.trim().is_empty() {
                json!({})
            } else {
                serde_json::from_str(&tc.function.arguments).unwrap_or_else(|_| {
                    // Some models hand back already-parsed JSON or malformed
                    // strings; surface the raw text so the tool can decide.
                    json!({ "_raw_arguments": tc.function.arguments })
                })
            };
            blocks.push(ContentBlock::ToolUse {
                id,
                name: tc.function.name,
                input,
            });
        }

        Ok(CompletionResult {
            text: text_out,
            blocks,
            stop_reason,
            provider: self.name.clone(),
            model,
            input_tokens: parsed.usage.prompt_tokens,
            output_tokens: parsed.usage.completion_tokens,
            elapsed_ms: t0.elapsed().as_millis() as u64,
        })
    }
}
