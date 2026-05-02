//! Ollama provider with OpenAI-compatible tool calling.
//!
//! Ollama's `/api/chat` accepts the same `tools` schema as OpenAI's chat
//! completions, and returns tool calls under `message.tool_calls`. Two notable
//! differences from the OpenAI wire format:
//!   - Tool calls don't include an `id` — we synthesize one so the brain's
//!     tool loop can track which result belongs to which call.
//!   - `arguments` is returned as a JSON object, not a string. We accept
//!     either to be defensive.

use super::{ChatMessage, CompletionResult, ContentBlock, MessageContent, Provider, ToolDef};
use crate::config::ProviderCfg;
use crate::error::{LarryError, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::time::{Duration, Instant};

pub struct OllamaProvider {
    cfg: ProviderCfg,
    http: reqwest::Client,
    base: String,
}

#[derive(Deserialize, Default)]
struct Resp {
    #[serde(default)]
    message: RespMessage,
    #[serde(default)]
    done_reason: Option<String>,
    #[serde(default)]
    prompt_eval_count: u64,
    #[serde(default)]
    eval_count: u64,
}

#[derive(Deserialize, Default)]
struct RespMessage {
    #[serde(default)]
    content: String,
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
    /// Ollama hands this back as an object; some compatibility layers send a
    /// string. Accept either via untyped `Value`.
    #[serde(default)]
    arguments: Value,
}

impl OllamaProvider {
    pub fn new(cfg: ProviderCfg, http: reqwest::Client) -> Result<Self> {
        let base = cfg
            .base_url
            .clone()
            .unwrap_or_else(|| "http://127.0.0.1:11434".into());
        Ok(Self { cfg, http, base })
    }
}

/// Same translation as the OpenAI provider: assistant blocks compress into a
/// single message with `tool_calls`; user blocks fan out into one
/// `role: "tool"` per ToolResult.
fn message_to_ollama(m: &ChatMessage) -> Vec<Value> {
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
                            tool_calls.push(json!({
                                "id": id,
                                "type": "function",
                                "function": { "name": name, "arguments": input }
                            }));
                        }
                        ContentBlock::ToolResult { .. } => {}
                    }
                }
                let mut msg = Map::new();
                msg.insert("role".into(), Value::String("assistant".into()));
                msg.insert(
                    "content".into(),
                    Value::String(text_parts.join("\n")),
                );
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
impl Provider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }
    fn default_model(&self) -> &str {
        &self.cfg.default_model
    }
    fn supports_tools(&self) -> bool {
        // tools_style = "text" disables native function-calling, mirroring
        // openai_compat — falls back to system-prompt injection for models that
        // weren't trained to emit structured `tool_calls`.
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
        let url = format!("{}/api/chat", self.base.trim_end_matches('/'));

        let mut payload_messages: Vec<Value> = Vec::with_capacity(messages.len() + 1);
        if let Some(sys) = system {
            payload_messages.push(json!({ "role": "system", "content": sys }));
        }
        for m in messages {
            payload_messages.extend(message_to_ollama(m));
        }

        let mut body = Map::new();
        body.insert("model".into(), Value::String(model.clone()));
        body.insert("messages".into(), Value::Array(payload_messages));
        body.insert("stream".into(), Value::Bool(false));
        body.insert(
            "options".into(),
            json!({ "num_predict": max_tokens }),
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
            .json(&body)
            .send()
            .await
            .map_err(|e| LarryError::Transient(format!("ollama http: {e}")))?;

        let status = resp.status();
        let raw = resp
            .text()
            .await
            .map_err(|e| LarryError::Transient(format!("ollama body: {e}")))?;

        if !status.is_success() {
            if status.is_server_error() {
                return Err(LarryError::Transient(format!("ollama {}: {}", status, raw)));
            }
            return Err(LarryError::Permanent(format!("ollama {}: {}", status, raw)));
        }

        let parsed: Resp = serde_json::from_str(&raw)
            .map_err(|e| LarryError::Permanent(format!("ollama decode: {e}: {raw}")))?;

        let text = parsed.message.content;
        let tool_calls = parsed.message.tool_calls.unwrap_or_default();

        let mut blocks: Vec<ContentBlock> = Vec::new();
        if !text.is_empty() {
            blocks.push(ContentBlock::Text { text: text.clone() });
        }
        for tc in tool_calls {
            let id = tc
                .id
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("call_{}", uuid::Uuid::new_v4().simple()));
            let input = match tc.function.arguments {
                Value::Null => json!({}),
                Value::String(s) if s.trim().is_empty() => json!({}),
                Value::String(s) => serde_json::from_str(&s)
                    .unwrap_or_else(|_| json!({ "_raw_arguments": s })),
                other => other,
            };
            blocks.push(ContentBlock::ToolUse {
                id,
                name: tc.function.name,
                input,
            });
        }

        // If the model both emitted text and called a tool, Ollama's
        // done_reason is usually "stop"; preserve it but mark "tool_use" so the
        // brain's loop predicate stays consistent with Anthropic semantics if
        // anyone inspects stop_reason.
        let stop_reason = if blocks.iter().any(|b| matches!(b, ContentBlock::ToolUse { .. })) {
            Some("tool_use".to_string())
        } else {
            parsed.done_reason
        };

        Ok(CompletionResult {
            text,
            blocks,
            stop_reason,
            provider: "ollama".into(),
            model,
            input_tokens: parsed.prompt_eval_count,
            output_tokens: parsed.eval_count,
            elapsed_ms: t0.elapsed().as_millis() as u64,
        })
    }
}
