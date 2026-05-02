//! Telegram bot: long-poll getUpdates, route to brain, sendMessage replies.

use crate::brain::{Brain, RespondOpts};
use crate::config::Config;
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

const MAX_TG_MSG_LEN: usize = 4000;
const POLL_TIMEOUT_S: u64 = 60;

#[derive(Debug, Deserialize)]
struct UpdatesResp {
    ok: bool,
    #[serde(default)]
    result: Vec<Update>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Update {
    update_id: i64,
    #[serde(default)]
    message: Option<Message>,
    #[serde(default)]
    edited_message: Option<Message>,
}

#[derive(Debug, Deserialize)]
struct Message {
    message_id: i64,
    #[serde(default)]
    chat: Chat,
    #[serde(default)]
    from: Option<User>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct Chat {
    #[serde(default)]
    id: i64,
}

#[derive(Debug, Deserialize)]
struct User {
    #[serde(default)]
    username: Option<String>,
}

#[derive(Debug, Serialize)]
struct SendReq<'a> {
    chat_id: i64,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_to_message_id: Option<i64>,
    disable_web_page_preview: bool,
}

#[derive(Default, Clone)]
struct ChatOverride {
    provider: Option<String>,
    model: Option<String>,
}

pub struct TelegramBot {
    cfg: Arc<Config>,
    brain: Arc<Brain>,
    http: reqwest::Client,
    api_base: String,
    overrides: Mutex<HashMap<i64, ChatOverride>>,
}

impl TelegramBot {
    pub fn new(cfg: Arc<Config>, brain: Arc<Brain>) -> Result<Arc<Self>> {
        let token = cfg
            .telegram_token
            .clone()
            .ok_or_else(|| crate::error::LarryError::Config("no telegram bot token".into()))?;
        let http = reqwest::Client::builder()
            .user_agent(concat!("larry/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Arc::new(Self {
            cfg,
            brain,
            http,
            api_base: format!("https://api.telegram.org/bot{}", token),
            overrides: Mutex::new(HashMap::new()),
        }))
    }

    /// Fire a single sendChatAction:typing. Fire-and-forget; logs errors at warn.
    async fn send_typing(http: &reqwest::Client, api_base: &str, chat_id: i64) {
        let url = format!("{}/sendChatAction", api_base);
        let body = serde_json::json!({ "chat_id": chat_id, "action": "typing" });
        match http
            .post(&url)
            .timeout(Duration::from_secs(10))
            .json(&body)
            .send()
            .await
        {
            Ok(r) if !r.status().is_success() => {
                let s = r.status();
                let txt = r.text().await.unwrap_or_default();
                let preview: String = txt.chars().take(200).collect();
                tracing::warn!(status=%s, body=%preview, "tg typing failed");
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(error=%e, "tg typing error"),
        }
    }

    pub async fn send(&self, chat_id: i64, text: &str, reply_to: Option<i64>) {
        let mut reply_to = reply_to;
        for chunk in chunks(text, MAX_TG_MSG_LEN) {
            let body = SendReq {
                chat_id,
                text: chunk,
                reply_to_message_id: reply_to.take(),
                disable_web_page_preview: true,
            };
            let url = format!("{}/sendMessage", self.api_base);
            match self
                .http
                .post(&url)
                .timeout(Duration::from_secs(20))
                .json(&body)
                .send()
                .await
            {
                Ok(r) => {
                    if !r.status().is_success() {
                        let s = r.status();
                        let txt = r.text().await.unwrap_or_default();
                        let preview: String = txt.chars().take(200).collect();
                        tracing::warn!(status=%s, body=%preview, "tg send failed");
                    }
                }
                Err(e) => tracing::warn!(error=%e, "tg send error"),
            }
        }
    }

    pub async fn run(self: Arc<Self>) -> Result<()> {
        tracing::info!(
            allowed = ?self.cfg.telegram_allowed_chats,
            "telegram bot started"
        );

        // Drain any backlog so we don't reply to messages from before startup.
        let mut offset: Option<i64> = None;
        match self
            .http
            .get(format!("{}/getUpdates", self.api_base))
            .query(&[("timeout", "0"), ("offset", "-1")])
            .timeout(Duration::from_secs(15))
            .send()
            .await
        {
            Ok(r) => {
                if let Ok(text) = r.text().await {
                    if let Ok(parsed) = serde_json::from_str::<UpdatesResp>(&text) {
                        if let Some(last) = parsed.result.last() {
                            offset = Some(last.update_id + 1);
                            tracing::info!(offset = ?offset, drained = parsed.result.len(), "drained backlog");
                        }
                    }
                }
            }
            Err(e) => tracing::warn!(error=%e, "drain getUpdates failed"),
        }

        loop {
            let mut req = self
                .http
                .get(format!("{}/getUpdates", self.api_base))
                .timeout(Duration::from_secs(POLL_TIMEOUT_S + 10));
            if let Some(o) = offset {
                req = req.query(&[("timeout", POLL_TIMEOUT_S.to_string()), ("offset", o.to_string())]);
            } else {
                req = req.query(&[("timeout", POLL_TIMEOUT_S.to_string())]);
            }

            let send_res = req.send().await;
            let resp = match send_res {
                Ok(r) => r,
                Err(e) if e.is_timeout() => continue,
                Err(e) => {
                    tracing::warn!(error=%e, "getUpdates http error");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };

            let status = resp.status();
            let body = match resp.text().await {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!(error=%e, "getUpdates body read error");
                    continue;
                }
            };
            if !status.is_success() {
                let preview: String = body.chars().take(300).collect();
                tracing::warn!(%status, %preview, "getUpdates non-200");
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            let parsed: UpdatesResp = match serde_json::from_str(&body) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(error=%e, "getUpdates decode failed");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };
            if !parsed.ok {
                tracing::warn!(desc = ?parsed.description, "getUpdates ok=false");
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            for upd in parsed.result {
                offset = Some(upd.update_id + 1);
                if let Err(e) = self.handle_update(upd).await {
                    tracing::warn!(error=%e, "update handler error");
                }
            }
        }
    }

    async fn handle_update(self: &Arc<Self>, upd: Update) -> Result<()> {
        let msg = match upd.message.or(upd.edited_message) {
            Some(m) => m,
            None => return Ok(()),
        };
        let text = match msg.text {
            Some(t) if !t.is_empty() => t,
            _ => return Ok(()),
        };
        let chat_id = msg.chat.id;
        if chat_id == 0 {
            return Ok(());
        }
        let user_label = msg
            .from
            .as_ref()
            .and_then(|u| u.username.clone())
            .unwrap_or_else(|| chat_id.to_string());

        // Allowlist
        if !self.cfg.telegram_allowed_chats.is_empty()
            && !self.cfg.telegram_allowed_chats.contains(&chat_id)
        {
            tracing::info!(chat_id, "ignored (not in allowed_chat_ids)");
            return Ok(());
        }

        tracing::info!(chat_id, user=%user_label, len = text.len(), "rx");

        // Commands first
        if let Some(stripped) = text.strip_prefix('/') {
            let mut parts = stripped.splitn(2, ' ');
            let cmd = parts.next().unwrap_or("").to_ascii_lowercase();
            let arg = parts.next().unwrap_or("").trim().to_string();
            if self.handle_command(chat_id, &cmd, &arg, msg.message_id).await? {
                return Ok(());
            }
        }

        // Otherwise: brain
        let override_pair = {
            let map = self.overrides.lock().await;
            map.get(&chat_id).cloned().unwrap_or_default()
        };
        let session_id = format!("tg:{chat_id}");
        let source = format!("telegram:{user_label}");

        // Spawn a background task that keeps the "typing" indicator alive while
        // the brain works. Telegram's sendChatAction lasts about 5 seconds, so
        // we re-send every 4. A oneshot channel signals cancellation.
        let (typing_stop_tx, typing_stop_rx) = tokio::sync::oneshot::channel::<()>();
        let typing_handle = {
            let http = self.http.clone();
            let api_base = self.api_base.clone();
            tokio::spawn(async move {
                Self::send_typing(&http, &api_base, chat_id).await;
                let mut rx = typing_stop_rx;
                loop {
                    tokio::select! {
                        _ = &mut rx => break,
                        _ = tokio::time::sleep(Duration::from_secs(4)) => {
                            Self::send_typing(&http, &api_base, chat_id).await;
                        }
                    }
                }
            })
        };

        let brain_result = self
            .brain
            .respond(
                &text,
                &session_id,
                RespondOpts {
                    source: &source,
                    provider_override: override_pair.provider.as_deref(),
                    model_override: override_pair.model.as_deref(),
                    extra_system: "",
                    allow_tools: true,
                },
            )
            .await;

        let _ = typing_stop_tx.send(());
        let _ = typing_handle.await;

        match brain_result {
            Ok(result) => {
                self.send(chat_id, &result.text, Some(msg.message_id)).await;
            }
            Err(e) => {
                tracing::error!(error=%e, "brain failed");
                self.send(chat_id, &format!("[larry error] {e}"), Some(msg.message_id))
                    .await;
            }
        }
        Ok(())
    }

    async fn handle_command(
        self: &Arc<Self>,
        chat_id: i64,
        cmd: &str,
        arg: &str,
        reply_to: i64,
    ) -> Result<bool> {
        match cmd {
            "start" | "help" => {
                self.send(chat_id, HELP_TEXT, Some(reply_to)).await;
                Ok(true)
            }
            "ping" => {
                self.send(chat_id, "pong", Some(reply_to)).await;
                Ok(true)
            }
            "providers" => {
                let names = self.brain.list_providers();
                self.send(chat_id, &format!("providers: {}", names.join(", ")), Some(reply_to))
                    .await;
                Ok(true)
            }
            "provider" => {
                if arg.is_empty() {
                    let map = self.overrides.lock().await;
                    let cur = map
                        .get(&chat_id)
                        .and_then(|o| o.provider.clone())
                        .unwrap_or_else(|| self.cfg.default_provider.clone());
                    self.send(chat_id, &format!("current provider: {cur}"), Some(reply_to))
                        .await;
                    return Ok(true);
                }
                if arg == "default" {
                    self.overrides.lock().await.remove(&chat_id);
                    self.send(chat_id, "cleared override; using default chain", Some(reply_to))
                        .await;
                    return Ok(true);
                }
                let avail = self.brain.list_providers();
                if !avail.iter().any(|p| p == arg) {
                    self.send(
                        chat_id,
                        &format!(
                            "unknown provider {arg}; available: {}",
                            avail.join(", ")
                        ),
                        Some(reply_to),
                    )
                    .await;
                    return Ok(true);
                }
                let mut map = self.overrides.lock().await;
                map.insert(
                    chat_id,
                    ChatOverride {
                        provider: Some(arg.into()),
                        model: None,
                    },
                );
                self.send(chat_id, &format!("provider override -> {arg}"), Some(reply_to))
                    .await;
                Ok(true)
            }
            "model" => {
                let mut sp = arg.splitn(2, ' ');
                let prov = sp.next().unwrap_or("").trim();
                let model = sp.next().unwrap_or("").trim();
                if prov.is_empty() || model.is_empty() {
                    self.send(chat_id, "usage: /model <provider> <model_id>", Some(reply_to))
                        .await;
                    return Ok(true);
                }
                let avail = self.brain.list_providers();
                if !avail.iter().any(|p| p == prov) {
                    self.send(
                        chat_id,
                        &format!(
                            "unknown provider {prov}; available: {}",
                            avail.join(", ")
                        ),
                        Some(reply_to),
                    )
                    .await;
                    return Ok(true);
                }
                let mut map = self.overrides.lock().await;
                map.insert(
                    chat_id,
                    ChatOverride {
                        provider: Some(prov.into()),
                        model: Some(model.into()),
                    },
                );
                self.send(chat_id, &format!("override -> {prov}/{model}"), Some(reply_to))
                    .await;
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}

fn chunks(s: &str, n: usize) -> Vec<&str> {
    let bytes = s.as_bytes();
    if bytes.len() <= n {
        return vec![s];
    }
    let mut out = Vec::new();
    let mut start = 0;
    while start < bytes.len() {
        let mut end = (start + n).min(bytes.len());
        // step back to a char boundary
        while end < bytes.len() && !s.is_char_boundary(end) {
            end -= 1;
        }
        out.push(&s[start..end]);
        start = end;
    }
    out
}

const HELP_TEXT: &str = "Larry commands:\n\
/help — this message\n\
/ping — quick liveness check\n\
/providers — list active providers\n\
/provider <name|default> — override provider for this chat\n\
/model <provider> <model_id> — override provider+model\n\
Anything else is sent to the brain (default model).";
