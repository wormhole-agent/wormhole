//! Subagent delegation: spawn Claude Code or Codex via subprocess.

use crate::config::Config;
use crate::error::Result;
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

#[derive(Debug, Clone)]
pub struct SubagentResult {
    pub agent: String,
    pub cmd: Vec<String>,
    pub cwd: String,
    pub returncode: i32,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

/// What kind of subprocess to spawn. Each variant maps to a fixed argv layout —
/// the shared `dispatch()` builds the actual command and routes through the
/// single timeout-aware `run()` engine. Codex review #subagent20.
pub enum Kind<'a> {
    Claude { prompt: &'a str, permission_mode: &'a str },
    Codex { prompt: &'a str },
    Shell { command: &'a str, shell: &'a str },
}

pub async fn run_claude(
    cfg: &Config,
    prompt: &str,
    cwd: Option<&str>,
    permission_mode: &str,
    timeout_s: u64,
) -> Result<SubagentResult> {
    dispatch(cfg, Kind::Claude { prompt, permission_mode }, cwd, timeout_s).await
}

pub async fn run_codex(
    cfg: &Config,
    prompt: &str,
    cwd: Option<&str>,
    timeout_s: u64,
) -> Result<SubagentResult> {
    dispatch(cfg, Kind::Codex { prompt }, cwd, timeout_s).await
}

pub async fn run_shell(
    cfg: &Config,
    command: &str,
    cwd: Option<&str>,
    shell: &str,
    timeout_s: u64,
) -> Result<SubagentResult> {
    dispatch(cfg, Kind::Shell { command, shell }, cwd, timeout_s).await
}

async fn dispatch(
    cfg: &Config,
    kind: Kind<'_>,
    cwd: Option<&str>,
    timeout_s: u64,
) -> Result<SubagentResult> {
    let cwd = cwd
        .map(String::from)
        .unwrap_or_else(|| cfg.workspace_root.display().to_string());
    let (agent, cmd) = match kind {
        Kind::Claude { prompt, permission_mode } => (
            "claude".to_string(),
            vec![
                cfg.delegate_claude_path.clone(),
                "--print".into(),
                "--permission-mode".into(),
                permission_mode.into(),
                prompt.into(),
            ],
        ),
        Kind::Codex { prompt } => (
            "codex".to_string(),
            vec![cfg.delegate_codex_path.clone(), "exec".into(), prompt.into()],
        ),
        Kind::Shell { command, shell } => {
            let argv = match shell {
                "bash" => vec!["bash".into(), "-lc".into(), command.into()],
                "pwsh" => vec![
                    "pwsh".into(),
                    "-NoProfile".into(),
                    "-Command".into(),
                    command.into(),
                ],
                "cmd" => vec!["cmd".into(), "/c".into(), command.into()],
                other => {
                    return Err(crate::error::LarryError::Permanent(format!(
                        "unknown shell: {other}"
                    )))
                }
            };
            (format!("shell:{shell}"), argv)
        }
    };
    run(cmd, &cwd, &agent, timeout_s).await
}

async fn run(cmd: Vec<String>, cwd: &str, agent: &str, timeout_s: u64) -> Result<SubagentResult> {
    tracing::info!(agent = %agent, cwd = %cwd, "subagent run");
    let mut iter = cmd.iter();
    let prog = iter
        .next()
        .ok_or_else(|| crate::error::LarryError::Permanent("empty cmd".into()))?;
    let args: Vec<&String> = iter.collect();

    let mut command = Command::new(prog);
    command
        .args(args.iter().map(|s| s.as_str()))
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());

    // Keep the child mutable so we can kill() it on timeout. Using
    // `wait_with_output()` consumed the child and a tokio::timeout merely
    // stopped *waiting* — the child kept running. Codex finding #110.
    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Ok(SubagentResult {
                agent: agent.into(),
                cmd: cmd.clone(),
                cwd: cwd.into(),
                returncode: -1,
                stdout: String::new(),
                stderr: format!("spawn failed: {e}"),
                timed_out: false,
            });
        }
    };

    // Take the piped stdout/stderr handles up front so we can read them after
    // either a clean wait OR a kill.
    let stdout_h = child.stdout.take();
    let stderr_h = child.stderr.take();

    let timeout_dur = Duration::from_secs(timeout_s);
    let wait_result = timeout(timeout_dur, child.wait()).await;

    let (returncode, timed_out) = match wait_result {
        Ok(Ok(status)) => (status.code().unwrap_or(-1), false),
        Ok(Err(e)) => {
            // Wait itself errored. Try kill in case it's still running.
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Ok(SubagentResult {
                agent: agent.into(),
                cmd,
                cwd: cwd.into(),
                returncode: -1,
                stdout: String::new(),
                stderr: format!("wait error: {e}"),
                timed_out: false,
            });
        }
        Err(_) => {
            // Timeout: actually kill the child, then reap.
            tracing::warn!(agent=%agent, timeout_s, "subprocess timed out — killing child");
            if let Err(e) = child.start_kill() {
                tracing::warn!(error=%e, "child kill failed (already exited?)");
            }
            // Bounded wait for kill to settle.
            let _ = timeout(Duration::from_secs(5), child.wait()).await;
            (-1, true)
        }
    };

    // Drain whatever stdout/stderr was captured, even after a kill.
    use tokio::io::AsyncReadExt;
    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();
    if let Some(mut s) = stdout_h {
        let _ = timeout(Duration::from_secs(2), s.read_to_end(&mut stdout_buf)).await;
    }
    if let Some(mut s) = stderr_h {
        let _ = timeout(Duration::from_secs(2), s.read_to_end(&mut stderr_buf)).await;
    }

    if timed_out {
        return Ok(SubagentResult {
            agent: agent.into(),
            cmd,
            cwd: cwd.into(),
            returncode: -1,
            stdout: String::from_utf8_lossy(&stdout_buf).to_string(),
            stderr: format!(
                "timed out after {}s — child killed\n{}",
                timeout_s,
                String::from_utf8_lossy(&stderr_buf)
            ),
            timed_out: true,
        });
    }
    Ok(SubagentResult {
        agent: agent.into(),
        cmd,
        cwd: cwd.into(),
        returncode,
        stdout: String::from_utf8_lossy(&stdout_buf).to_string(),
        stderr: String::from_utf8_lossy(&stderr_buf).to_string(),
        timed_out: false,
    })
}

pub fn summarise(res: &SubagentResult) -> String {
    let mut head = format!(
        "{} rc={}{}\n",
        res.agent,
        res.returncode,
        if res.timed_out { " (TIMED OUT)" } else { "" }
    );
    if !res.stdout.trim().is_empty() {
        let s = res.stdout.trim();
        let tail: String = if s.chars().count() > 1500 {
            let start = s.chars().count() - 1500;
            s.chars().skip(start).collect()
        } else {
            s.to_string()
        };
        head.push_str("stdout:\n");
        head.push_str(&tail);
        head.push('\n');
    }
    if !res.stderr.trim().is_empty() {
        let s = res.stderr.trim();
        let tail: String = if s.chars().count() > 800 {
            let start = s.chars().count() - 800;
            s.chars().skip(start).collect()
        } else {
            s.to_string()
        };
        head.push_str("stderr:\n");
        head.push_str(&tail);
    }
    head.trim().to_string()
}
