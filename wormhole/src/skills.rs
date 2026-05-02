//! Skills: markdown files in ~/wormhole/skills/ that the assistant can
//! `list_skills` and `load_skill` on demand. Format:
//!
//! ```text
//! ---
//! name: find_or_build_capability
//! description: Decide whether to use an existing skill, search for one, or build one.
//! when_to_use: When a task needs a capability you don't currently have.
//! ---
//!
//! # body in markdown
//! 1. Check skills...
//! ```

use crate::config::Config;
use crate::error::Result;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub when_to_use: String,
    pub body: String,
}

pub struct SkillRegistry {
    skills: BTreeMap<String, Skill>,
    /// In-memory usage counter per skill name. Bumped whenever `load_skill`
    /// is invoked (or any other code path that exercises the skill body).
    /// Not persisted across restarts — the curator skill snapshots this on a
    /// schedule when long-term ranking is needed.
    usage: RwLock<HashMap<String, u64>>,
}

impl SkillRegistry {
    pub fn load(cfg: &Config) -> Result<Arc<Self>> {
        let dir = &cfg.skills_dir;
        let mut skills = BTreeMap::new();
        if !dir.exists() {
            tracing::info!(skills_dir = %dir.display(), "no skills dir, skipping");
            return Ok(Arc::new(Self { skills, usage: RwLock::new(HashMap::new()) }));
        }
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let fname = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if fname == "README" {
                continue;
            }
            match parse_skill_file(&p) {
                Ok(s) => {
                    tracing::info!(skill = %s.name, file = %p.display(), "loaded skill");
                    skills.insert(s.name.clone(), s);
                }
                Err(e) => {
                    tracing::warn!(file = %p.display(), error = %e, "failed to parse skill");
                }
            }
        }
        Ok(Arc::new(Self { skills, usage: RwLock::new(HashMap::new()) }))
    }

    pub fn names(&self) -> Vec<String> {
        self.skills.keys().cloned().collect()
    }

    pub fn body(&self, name: &str) -> Option<String> {
        self.skills.get(name).map(|s| s.body.clone())
    }

    /// Bump the usage counter for `name`. No-op for unknown skills — we still
    /// record the count so a typo'd `load_skill` shows up in the curator's
    /// "missing skills" list later.
    pub fn bump_use(&self, name: &str) {
        if let Ok(mut map) = self.usage.write() {
            *map.entry(name.to_string()).or_insert(0) += 1;
        }
    }

    /// Snapshot of (name, uses) sorted by uses desc, then name asc. Includes
    /// every loaded skill (zero count if never invoked) so the report can show
    /// the long tail.
    pub fn usage_ranked(&self) -> Vec<(String, u64)> {
        let map = self.usage.read().map(|g| g.clone()).unwrap_or_default();
        let mut out: Vec<(String, u64)> = self
            .skills
            .keys()
            .map(|n| (n.clone(), *map.get(n).unwrap_or(&0)))
            .collect();
        // Surface skills that were called but aren't loaded — typos / removed
        // files. Useful signal for the curator.
        for (n, c) in map.iter() {
            if !self.skills.contains_key(n) {
                out.push((n.clone(), *c));
            }
        }
        out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        out
    }

    /// Human-readable most-used / least-used summary. Used by the curator skill
    /// and the `/api/skills/usage` UI endpoint. Format is meant to fit in a
    /// Telegram message or a section of REPORT.md.
    #[allow(dead_code)]
    pub fn usage_report(&self) -> String {
        let ranked = self.usage_ranked();
        if ranked.is_empty() {
            return "no skills loaded".into();
        }
        let mut s = String::new();
        s.push_str("# Skill usage report\n\n");
        let total: u64 = ranked.iter().map(|(_, c)| *c).sum();
        s.push_str(&format!("total invocations: {total}\n\n"));
        s.push_str("## Most used\n");
        let top: Vec<_> = ranked.iter().take(10).collect();
        for (n, c) in &top {
            s.push_str(&format!("- {n}: {c}\n"));
        }
        s.push_str("\n## Least used (loaded but never invoked or rare)\n");
        let mut tail: Vec<_> = ranked.iter().rev().take(10).collect();
        tail.reverse();
        for (n, c) in &tail {
            s.push_str(&format!("- {n}: {c}\n"));
        }
        s
    }

    /// Single-line summary per skill — fits inside the system prompt.
    pub fn list_for_prompt(&self) -> String {
        let mut out = String::new();
        for s in self.skills.values() {
            out.push_str(&format!("- **{}** — {}\n  when_to_use: {}\n", s.name, s.description, s.when_to_use));
        }
        out
    }

    /// Section to inject into the system prompt.
    pub fn system_section(&self) -> Option<String> {
        if self.skills.is_empty() {
            return None;
        }
        let listing = self.list_for_prompt();
        Some(format!(
            "# AVAILABLE SKILLS\n\
             Skills are reusable playbooks. To run one, call the `load_skill` tool with the skill's name and follow its body.\n\n\
             {listing}\n\
             To find missing capabilities (no skill exists for the task), follow `find_or_build_capability` if it's loaded."
        ))
    }
}

fn parse_skill_file(path: &Path) -> Result<Skill> {
    let text = fs::read_to_string(path)?;
    let mut name = String::new();
    let mut description = String::new();
    let mut when_to_use = String::new();

    let trimmed = text.trim_start_matches('\u{feff}'); // strip BOM if any
    let lines: Vec<&str> = trimmed.lines().collect();

    let mut i = 0;
    if lines.first().map(|s| s.trim()) == Some("---") {
        i += 1;
        while i < lines.len() && lines[i].trim() != "---" {
            let line = lines[i];
            if let Some((k, v)) = line.split_once(':') {
                let key = k.trim().to_ascii_lowercase();
                let value = v.trim().trim_matches('"').to_string();
                match key.as_str() {
                    "name" => name = value,
                    "description" => description = value,
                    "when_to_use" | "when-to-use" | "trigger" => when_to_use = value,
                    _ => {}
                }
            }
            i += 1;
        }
        if i < lines.len() && lines[i].trim() == "---" {
            i += 1;
        }
    }
    let body = lines[i..].join("\n").trim().to_string();

    if name.is_empty() {
        // fall back to filename stem
        name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed")
            .to_string();
    }
    if description.is_empty() {
        description = "(no description provided)".into();
    }
    if when_to_use.is_empty() {
        when_to_use = "(no trigger described)".into();
    }

    Ok(Skill {
        name,
        description,
        when_to_use,
        body,
    })
}
