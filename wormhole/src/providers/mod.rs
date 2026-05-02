//! Provider abstraction. Every provider exposes the same async `complete` shape.
//!
//! v0.2: tool calling. Messages and responses can include `ContentBlock` arrays
//! to support Anthropic-style `tool_use` / `tool_result` round-trips. Plain-text
//! responses (no tools) still work the same as v0.1.

use crate::config::{Config, ProviderCfg};
use crate::error::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

pub mod anthropic;
pub mod ollama;
pub mod openai_compat;

/// One block in a multimodal message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: Value },
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
    },
}

/// A single chat message. `content` is either a plain string (most common) or
/// an explicit list of blocks (used when tool_use / tool_result is involved).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: MessageContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

impl ChatMessage {
    pub fn user(text: impl Into<String>) -> Self {
        Self { role: "user".into(), content: MessageContent::Text(text.into()) }
    }
    pub fn assistant(text: impl Into<String>) -> Self {
        Self { role: "assistant".into(), content: MessageContent::Text(text.into()) }
    }
    pub fn assistant_blocks(blocks: Vec<ContentBlock>) -> Self {
        Self { role: "assistant".into(), content: MessageContent::Blocks(blocks) }
    }
    pub fn user_blocks(blocks: Vec<ContentBlock>) -> Self {
        Self { role: "user".into(), content: MessageContent::Blocks(blocks) }
    }
}

/// Tool definition shipped to the provider.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone)]
pub struct CompletionResult {
    /// Concatenated text from all Text blocks (kept for backward compat).
    pub text: String,
    /// Raw blocks from the response. Empty if the provider doesn't structure responses.
    pub blocks: Vec<ContentBlock>,
    /// e.g. "end_turn" | "tool_use" | "max_tokens" | "stop_sequence" — provider-specific.
    pub stop_reason: Option<String>,
    pub provider: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub elapsed_ms: u64,
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn default_model(&self) -> &str;
    /// Whether this provider supports tool calling. v0.2: only anthropic.
    fn supports_tools(&self) -> bool { false }

    async fn complete(
        &self,
        messages: &[ChatMessage],
        model: Option<&str>,
        system: Option<&str>,
        max_tokens: u32,
        tools: Option<&[ToolDef]>,
    ) -> Result<CompletionResult>;
}

pub fn build_providers(cfg: &Config) -> BTreeMap<String, Arc<dyn Provider>> {
    let mut out: BTreeMap<String, Arc<dyn Provider>> = BTreeMap::new();
    let http = reqwest::Client::builder()
        .user_agent(concat!("larry/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("build reqwest client");

    for (name, pcfg) in &cfg.providers {
        if !pcfg.enabled {
            continue;
        }
        match try_build(name, pcfg, &http, cfg) {
            Ok(prov) => {
                out.insert(name.clone(), prov);
            }
            Err(e) => {
                tracing::warn!(provider = %name, error = %e, "skipping provider");
            }
        }
    }
    out
}

fn try_build(
    name: &str,
    pcfg: &ProviderCfg,
    http: &reqwest::Client,
    cfg: &Config,
) -> Result<Arc<dyn Provider>> {
    match name {
        "anthropic" => Ok(Arc::new(anthropic::AnthropicProvider::new(
            pcfg.clone(),
            http.clone(),
            cfg.prompt_caching.clone(),
        )?)),
        "openai" => Ok(Arc::new(openai_compat::OpenAICompatProvider::new(
            pcfg.clone(),
            http.clone(),
            "openai".into(),
            "https://api.openai.com/v1".into(),
        )?)),
        "deepseek" => {
            let base = pcfg
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.deepseek.com".into());
            Ok(Arc::new(openai_compat::OpenAICompatProvider::new(
                pcfg.clone(),
                http.clone(),
                "deepseek".into(),
                base,
            )?))
        }
        "ollama" => Ok(Arc::new(ollama::OllamaProvider::new(
            pcfg.clone(),
            http.clone(),
        )?)),
        other => Err(crate::error::LarryError::Permanent(format!(
            "unknown provider: {other}"
        ))),
    }
}
