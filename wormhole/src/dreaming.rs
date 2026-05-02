//! Nightly dreaming pass.
//!
//! Reads the last N days of daily memory, asks the brain to distill events,
//! HIGH-confidence insights, and cross-module patterns, then:
//!   * writes a markdown diary to `<workspace>/memory/.dreams/<date>.md`
//!   * appends HIGH-confidence insights (deduplicated) to
//!     `<workspace>/MEMORY-proposed.md` for the weekly human-review step.
//!
//! Does NOT modify `MEMORY.md` — promotion is the user's call (handled by the
//! existing Monday memory-review cron's nag).
//!
//! Scheduling: this module exposes `run_nightly()`. The cadence is owned by
//! `cron.toml` via the `kind = "dream"` entry — see `cron.rs::run_one`.

use crate::brain::{Brain, RespondOpts};
use crate::config::Config;
use crate::error::Result;
use chrono::{Datelike, Duration, Local, NaiveDate, Weekday};
use serde::Deserialize;
use std::fs;
use std::io::Write;
use std::sync::Arc;

/// Result of a single dreaming run. Carries both the log-line summary (used
/// by `cmd_dream` and the cron run-log) and an optional Telegram message —
/// only set on Monday runs that produced ≥ 1 HIGH-confidence proposal, so the
/// dreaming cron only pings the user when there's actually something to review.
#[derive(Debug, Clone, Default)]
pub struct DreamRunResult {
    pub log_line: String,
    pub telegram_msg: Option<String>,
    pub promoted: usize,
}

impl DreamRunResult {
    pub fn skipped(reason: impl Into<String>) -> Self {
        Self {
            log_line: reason.into(),
            telegram_msg: None,
            promoted: 0,
        }
    }
}

/// Cap on the size of the corpus we hand to the brain. Iterating today →
/// backwards means the *oldest* day gets truncated first when a busy week
/// piles up.
const MAX_INPUT_CHARS: usize = 20_000;

#[derive(Debug, Default, Deserialize)]
struct DreamPayload {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    insights: Vec<Insight>,
    #[serde(default)]
    patterns: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Insight {
    #[serde(default)]
    text: String,
    #[serde(default)]
    confidence: String,
}

#[derive(Debug, Default, Deserialize)]
struct LightPayload {
    #[serde(default)]
    events: Vec<ScoredEvent>,
}

#[derive(Debug, Default, Deserialize)]
struct ScoredEvent {
    #[serde(default)]
    text: String,
    #[serde(default)]
    score: f64,
}

#[derive(Debug, Default, Deserialize)]
struct RemPayload {
    #[serde(default)]
    patterns: Vec<String>,
    #[serde(default)]
    cross_module_notes: Vec<String>,
}

pub struct DreamingScheduler {
    cfg: Arc<Config>,
    brain: Arc<Brain>,
}

impl DreamingScheduler {
    pub fn new(cfg: Arc<Config>, brain: Arc<Brain>) -> Self {
        Self { cfg, brain }
    }

    /// Runs the nightly pass. Returns a structured summary so the cron job
    /// can decide whether to ping Telegram.
    pub async fn run_nightly(&self) -> Result<DreamRunResult> {
        if !self.cfg.dreaming.enabled {
            tracing::info!("dreaming: disabled in config — skipping");
            return Ok(DreamRunResult::skipped("dreaming disabled in config"));
        }

        let today = Local::now().date_naive();
        let window: i64 = self.cfg.dreaming.window_days.max(1) as i64;
        tracing::info!(date = %today, window_days = window, "dreaming: starting 3-phase nightly pass");

        // Phase 1: light sleep — yesterday-only event extraction with scores.
        let light = match self.run_light_phase(today).await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("dreaming: light phase failed: {e} — continuing with empty");
                LightPayload::default()
            }
        };

        // Phase 2: REM — 7-day pattern synthesis.
        let (rem, rem_dates) = match self.run_rem_phase(today).await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!("dreaming: REM phase failed: {e} — continuing with empty");
                (RemPayload::default(), Vec::new())
            }
        };

        if rem_dates.is_empty() && light.events.is_empty() {
            tracing::warn!("dreaming: no daily memory in last {window} days — skipping");
            return Ok(DreamRunResult::skipped("no daily memory in window"));
        }

        // Phase 3: deep — Anthropic decides what is HIGH-confidence enough to
        // promote into MEMORY-proposed.md.
        let (payload, r) = self.run_deep_phase(today, &light, &rem).await?;

        let dates_used: Vec<NaiveDate> = if !rem_dates.is_empty() {
            rem_dates
        } else {
            vec![today - Duration::days(1)]
        };

        self.write_dream_diary(today, &dates_used, &payload, &r, &light, &rem)?;
        let promoted = self.append_proposed(today, &payload)?;

        let threshold = self.cfg.dreaming.promotion_threshold * 0.5;
        let above = light.events.iter().filter(|e| e.score >= threshold).count();
        let log_line = format!(
            "dream {today}: {} day(s), light={}/{} above {:.2}, rem={}p+{}n, {} insights, {} patterns, {} promoted ({}/{}, {}ms)",
            dates_used.len(),
            above,
            light.events.len(),
            threshold,
            rem.patterns.len(),
            rem.cross_module_notes.len(),
            payload.insights.len(),
            payload.patterns.len(),
            promoted,
            r.provider,
            r.model,
            r.elapsed_ms,
        );
        tracing::info!("dreaming: complete — {log_line}");

        // Monday review ping. Only fires when there's actually new
        // material to review; the cron's `deliver = "telegram"` setting on top
        // of this will then forward the message via the existing deliver_fn.
        let telegram_msg = if today.weekday() == Weekday::Mon && promoted >= 1 {
            Some(format!(
                "[memory-review] {promoted} HIGH-confidence insight(s) proposed overnight. \
                 Review at MEMORY-proposed.md (memory/.dreams/{today}.md for the full diary)."
            ))
        } else {
            None
        };

        Ok(DreamRunResult {
            log_line,
            telegram_msg,
            promoted,
        })
    }

    async fn run_light_phase(&self, today: NaiveDate) -> Result<LightPayload> {
        let yesterday = today - Duration::days(1);
        let (corpus, _dates) = self.gather_corpus(yesterday, 1);
        if corpus.trim().is_empty() {
            return Ok(LightPayload::default());
        }

        let prompt = format!(
"You are doing the light sleep pass. Read yesterday's daily memory log and extract notable events. For each event, give it a memorability score from 0.0 (trivial/routine) to 1.0 (major decision, key outcome, important lesson). Reply with ONLY JSON: {{\"events\": [{{\"text\": \"...\", \"score\": 0.8}}]}}

=== YESTERDAY ({yesterday}) ===
{corpus}
=== END ==="
        );

        let session = format!("dream-light:{today}");
        let r = self
            .brain
            .respond(
                &prompt,
                &session,
                RespondOpts {
                    source: "dreaming-light",
                    provider_override: Some(self.cfg.dreaming.light_model.as_str()),
                    model_override: self.cfg.dreaming.light_model_id.as_deref(),
                    extra_system: "",
                    allow_tools: false,
                },
            )
            .await?;

        Ok(parse_light_response(&r.text))
    }

    async fn run_rem_phase(&self, today: NaiveDate) -> Result<(RemPayload, Vec<NaiveDate>)> {
        let window: i64 = self.cfg.dreaming.window_days.max(1) as i64;
        let (corpus, dates) = self.gather_corpus(today, window);
        if corpus.trim().is_empty() {
            return Ok((RemPayload::default(), dates));
        }

        let prompt = format!(
"You are doing the REM pass. Read the past {n} days of memory and identify: 1) recurring patterns across multiple days or modules, 2) cross-module connections or themes. Reply with ONLY JSON: {{\"patterns\": [\"...\"], \"cross_module_notes\": [\"...\"]}}

=== LAST {n} DAYS (newest first) ===
{corpus}
=== END ===",
            n = dates.len(),
        );

        let session = format!("dream-rem:{today}");
        let r = self
            .brain
            .respond(
                &prompt,
                &session,
                RespondOpts {
                    source: "dreaming-rem",
                    provider_override: Some(self.cfg.dreaming.rem_model.as_str()),
                    model_override: self.cfg.dreaming.rem_model_id.as_deref(),
                    extra_system: "",
                    allow_tools: false,
                },
            )
            .await?;

        Ok((parse_rem_response(&r.text), dates))
    }

    async fn run_deep_phase(
        &self,
        today: NaiveDate,
        light: &LightPayload,
        rem: &RemPayload,
    ) -> Result<(DreamPayload, crate::providers::CompletionResult)> {
        let threshold = self.cfg.dreaming.promotion_threshold * 0.5;

        let mut light_block = String::new();
        for ev in &light.events {
            if ev.score < threshold {
                continue;
            }
            let t = ev.text.trim();
            if t.is_empty() {
                continue;
            }
            light_block.push_str(&format!("- ({:.2}) {t}\n", ev.score));
        }
        if light_block.is_empty() {
            light_block.push_str("(none)\n");
        }

        let mut rem_block = String::new();
        for p in &rem.patterns {
            let t = p.trim();
            if t.is_empty() {
                continue;
            }
            rem_block.push_str(&format!("- {t}\n"));
        }
        for n in &rem.cross_module_notes {
            let t = n.trim();
            if t.is_empty() {
                continue;
            }
            rem_block.push_str(&format!("- (cross-module) {t}\n"));
        }
        if rem_block.is_empty() {
            rem_block.push_str("(none)\n");
        }

        let prompt = format!(
"You are doing the deep sleep pass. Below are scored events from yesterday and patterns from 7 days. Your job: decide what is truly worth writing into long-term memory. Output HIGH-confidence items only (things that are settled facts, decisions made, lessons proven true - not speculation). Reply with ONLY JSON: {{\"summary\": \"...\", \"insights\": [{{\"text\": \"...\", \"confidence\": \"HIGH\"}}], \"patterns\": [\"...\"]}}

Scored events (score >= {threshold:.2} shown):
{light_block}
Patterns:
{rem_block}"
        );

        let session = format!("dream-deep:{today}");
        let r = self
            .brain
            .respond(
                &prompt,
                &session,
                RespondOpts {
                    source: "dreaming-deep",
                    provider_override: Some(self.cfg.dreaming.deep_model.as_str()),
                    model_override: self.cfg.dreaming.deep_model_id.as_deref(),
                    extra_system: "",
                    allow_tools: false,
                },
            )
            .await?;

        let payload = parse_dream_response(&r.text);
        Ok((payload, r))
    }

    fn gather_corpus(&self, today: NaiveDate, days: i64) -> (String, Vec<NaiveDate>) {
        let dir = self
            .cfg
            .workspace_root
            .join(&self.cfg.daily_dir_name);
        let mut sections: Vec<String> = Vec::new();
        let mut dates: Vec<NaiveDate> = Vec::new();
        let mut total: usize = 0;
        for delta in 0..days {
            let d = today - Duration::days(delta);
            let name = format!("{:04}-{:02}-{:02}.md", d.year(), d.month(), d.day());
            let path = dir.join(&name);
            let Ok(text) = fs::read_to_string(&path) else { continue };
            let trimmed = text.trim();
            if trimmed.is_empty() {
                continue;
            }
            let block = format!("\n## {d}\n{trimmed}\n");
            let block_chars = block.chars().count();
            if total + block_chars > MAX_INPUT_CHARS {
                let remaining = MAX_INPUT_CHARS.saturating_sub(total);
                if remaining > 400 {
                    let truncated: String = block.chars().take(remaining).collect();
                    sections.push(format!("{truncated}\n[... truncated ...]"));
                    dates.push(d);
                }
                break;
            }
            total += block_chars;
            sections.push(block);
            dates.push(d);
        }
        (sections.join(""), dates)
    }

    fn write_dream_diary(
        &self,
        date: NaiveDate,
        dates_used: &[NaiveDate],
        payload: &DreamPayload,
        raw: &crate::providers::CompletionResult,
        light: &LightPayload,
        rem: &RemPayload,
    ) -> Result<()> {
        let dir = self
            .cfg
            .workspace_root
            .join(&self.cfg.daily_dir_name)
            .join(".dreams");
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!(
            "{:04}-{:02}-{:02}.md",
            date.year(),
            date.month(),
            date.day()
        ));

        let span = match (dates_used.last(), dates_used.first()) {
            (Some(oldest), Some(newest)) if oldest != newest => format!("{oldest} → {newest}"),
            (Some(only), _) => only.to_string(),
            _ => "(empty)".into(),
        };

        let mut s = String::new();
        s.push_str(&format!("# Dream — {date}\n\n"));
        s.push_str(&format!(
            "_{} day(s) read, range: {span}, deep model: {}/{}, {}ms_\n\n",
            dates_used.len(),
            raw.provider,
            raw.model,
            raw.elapsed_ms
        ));

        let threshold = self.cfg.dreaming.promotion_threshold * 0.5;
        let above = light.events.iter().filter(|e| e.score >= threshold).count();
        s.push_str("## Phases\n");
        s.push_str(&format!(
            "- Light ({}): {} events scored, {} above threshold {:.2}\n",
            self.cfg.dreaming.light_model,
            light.events.len(),
            above,
            threshold,
        ));
        s.push_str(&format!(
            "- REM ({}): {} patterns, {} cross-module notes\n",
            self.cfg.dreaming.rem_model,
            rem.patterns.len(),
            rem.cross_module_notes.len(),
        ));
        s.push_str(&format!(
            "- Deep ({}): produced {} insights\n\n",
            self.cfg.dreaming.deep_model,
            payload.insights.len(),
        ));

        if !payload.summary.trim().is_empty() {
            s.push_str("## Summary\n");
            s.push_str(payload.summary.trim());
            s.push_str("\n\n");
        }
        if !payload.insights.is_empty() {
            s.push_str("## Insights\n");
            for it in &payload.insights {
                let conf = it.confidence.trim().to_uppercase();
                let tag = if conf.is_empty() {
                    String::new()
                } else {
                    format!("[{conf}] ")
                };
                let text = it.text.trim();
                if text.is_empty() {
                    continue;
                }
                s.push_str(&format!("- {tag}{text}\n"));
            }
            s.push('\n');
        }
        if !payload.patterns.is_empty() {
            s.push_str("## Patterns\n");
            for p in &payload.patterns {
                let t = p.trim();
                if t.is_empty() {
                    continue;
                }
                s.push_str(&format!("- {t}\n"));
            }
            s.push('\n');
        }

        // If the parser couldn't recover any structured content, dump the raw
        // model reply so we don't lose the work.
        if payload.summary.trim().is_empty()
            && payload.insights.is_empty()
            && payload.patterns.is_empty()
        {
            s.push_str("## Raw response (parser fallback)\n\n");
            s.push_str(raw.text.trim());
            s.push('\n');
        }

        fs::write(&path, s)?;
        Ok(())
    }

    fn append_proposed(&self, date: NaiveDate, payload: &DreamPayload) -> Result<usize> {
        let path = self.cfg.workspace_root.join("MEMORY-proposed.md");
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let existing_norm = normalize_for_dedup(&existing);

        let high: Vec<&Insight> = payload
            .insights
            .iter()
            .filter(|i| i.confidence.trim().eq_ignore_ascii_case("high"))
            .filter(|i| !i.text.trim().is_empty())
            .collect();
        if high.is_empty() {
            return Ok(0);
        }

        let mut to_add: Vec<String> = Vec::new();
        let mut added_norm = String::new();
        for ins in high {
            let key = normalize_for_dedup(ins.text.trim());
            // First 80 normalized chars catches near-duplicates without
            // making the check fragile to small wording changes.
            let probe: String = key.chars().take(80).collect();
            if probe.is_empty() {
                continue;
            }
            if existing_norm.contains(&probe) || added_norm.contains(&probe) {
                continue;
            }
            added_norm.push(' ');
            added_norm.push_str(&probe);
            to_add.push(ins.text.trim().to_string());
        }
        if to_add.is_empty() {
            return Ok(0);
        }

        let mut block = String::new();
        if existing.trim().is_empty() {
            block.push_str("# MEMORY proposed updates\n\n");
            block.push_str("Items here are HIGH-confidence dream insights awaiting human review before promotion to MEMORY.md.\n\n");
        }
        block.push_str(&format!("## Dream {date}\n\n"));
        for a in &to_add {
            block.push_str(&format!("- [HIGH] {a}\n"));
        }
        block.push('\n');

        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        f.write_all(block.as_bytes())?;
        Ok(to_add.len())
    }
}

fn normalize_for_dedup(s: &str) -> String {
    let lower: String = s.chars().flat_map(|c| c.to_lowercase()).collect();
    lower
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn parse_dream_response(text: &str) -> DreamPayload {
    for candidate in extract_json_candidates(text) {
        if let Ok(p) = serde_json::from_str::<DreamPayload>(&candidate) {
            return p;
        }
    }
    DreamPayload::default()
}

fn parse_light_response(text: &str) -> LightPayload {
    for candidate in extract_json_candidates(text) {
        if let Ok(p) = serde_json::from_str::<LightPayload>(&candidate) {
            return p;
        }
    }
    LightPayload::default()
}

fn parse_rem_response(text: &str) -> RemPayload {
    for candidate in extract_json_candidates(text) {
        if let Ok(p) = serde_json::from_str::<RemPayload>(&candidate) {
            return p;
        }
    }
    RemPayload::default()
}

fn extract_json_candidates(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let trimmed = text.trim();

    // Try fenced ```json ... ``` first.
    if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        let after = after.strip_prefix("json").unwrap_or(after);
        let after = after.trim_start_matches(['\n', '\r', ' ']);
        if let Some(end) = after.find("```") {
            out.push(after[..end].trim().to_string());
        }
    }

    out.push(trimmed.to_string());

    // Substring between first '{' and last '}'.
    if let (Some(s), Some(e)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if e > s {
            out.push(trimmed[s..=e].to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_clean_json() {
        let raw = r#"{"summary":"x","insights":[{"text":"a","confidence":"HIGH"}],"patterns":["p1"]}"#;
        let p = parse_dream_response(raw);
        assert_eq!(p.summary, "x");
        assert_eq!(p.insights.len(), 1);
        assert_eq!(p.insights[0].confidence, "HIGH");
        assert_eq!(p.patterns, vec!["p1".to_string()]);
    }

    #[test]
    fn parse_fenced_json_with_prose() {
        let raw = "Here you go:\n```json\n{\"summary\":\"y\",\"insights\":[],\"patterns\":[]}\n```\nDone.";
        let p = parse_dream_response(raw);
        assert_eq!(p.summary, "y");
    }

    #[test]
    fn parse_json_with_trailing_text() {
        let raw = "{\"summary\":\"z\",\"insights\":[],\"patterns\":[]} thanks!";
        let p = parse_dream_response(raw);
        assert_eq!(p.summary, "z");
    }

    #[test]
    fn parse_garbage_returns_empty() {
        let p = parse_dream_response("model went off the rails");
        assert!(p.summary.is_empty());
        assert!(p.insights.is_empty());
    }

    #[test]
    fn dedup_normalize() {
        let a = "Brookside Party Warehouse paid $850 on 2026-04-29.";
        let b = "brookside  party WAREHOUSE  paid 850 on 2026 04 29!";
        assert_eq!(normalize_for_dedup(a), normalize_for_dedup(b));
    }
}
