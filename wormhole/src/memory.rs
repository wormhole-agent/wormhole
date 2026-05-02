//! Memory: read AGENTS / MEMORY / USER / SOUL + today's daily file as context;
//! append turns back to today's daily file.

use crate::config::Config;
use crate::error::Result;
use chrono::{Datelike, Local, NaiveDate};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

fn read_capped(path: PathBuf, max_chars: Option<usize>) -> String {
    match fs::read_to_string(&path) {
        Ok(text) => {
            if let Some(max) = max_chars {
                if text.chars().count() > max {
                    let start = text.chars().count().saturating_sub(max);
                    return text.chars().skip(start).collect();
                }
            }
            text
        }
        Err(_) => String::new(),
    }
}

pub fn daily_path(cfg: &Config, date: Option<NaiveDate>) -> PathBuf {
    let d = date.unwrap_or_else(|| Local::now().date_naive());
    cfg.workspace_root
        .join(&cfg.daily_dir_name)
        .join(format!("{:04}-{:02}-{:02}.md", d.year(), d.month(), d.day()))
}

pub fn build_system_prompt(cfg: &Config, extra: &str) -> String {
    let mut sections: Vec<String> = Vec::new();
    let base = &cfg.workspace_root;

    let soul = read_capped(base.join(&cfg.soul_md_name), None);
    if !soul.trim().is_empty() {
        sections.push(format!("# SOUL\n{}", soul.trim()));
    }
    let user = read_capped(base.join(&cfg.user_md_name), None);
    if !user.trim().is_empty() {
        sections.push(format!("# USER\n{}", user.trim()));
    }
    let agents = read_capped(base.join(&cfg.agents_md_name), None);
    if !agents.trim().is_empty() {
        sections.push(format!("# AGENTS\n{}", agents.trim()));
    }
    let memory = read_capped(base.join(&cfg.memory_md_name), None);
    if !memory.trim().is_empty() {
        sections.push(format!("# MEMORY\n{}", memory.trim()));
    }
    let daily = read_capped(daily_path(cfg, None), Some(cfg.daily_max_chars));
    if !daily.trim().is_empty() {
        let today = Local::now().date_naive();
        sections.push(format!("# TODAY ({})\n{}", today, daily.trim()));
    }
    if !extra.trim().is_empty() {
        sections.push(extra.trim().to_string());
    }
    let now = Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    sections.push(format!("# Current local time: {}", now));
    sections.join("\n\n")
}

pub fn append_daily(cfg: &Config, role: &str, text: &str, source: &str) -> Result<()> {
    let path = daily_path(cfg, None);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let ts = Local::now().format("%H:%M:%S").to_string();
    let src = if source.is_empty() {
        String::new()
    } else {
        format!(" [{}]", source)
    };
    let block = format!("\n## {ts} {role}{src}\n{}\n", text.trim());
    let mut f = fs::OpenOptions::new().create(true).append(true).open(&path)?;
    f.write_all(block.as_bytes())?;
    Ok(())
}
