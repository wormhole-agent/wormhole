#!/usr/bin/env node
// Shipped with WormHole v0.1.0. See workspace/tools/TOOLS.md for usage.
//
/**
 * secrets-guard - scan the workspace for exposed secrets.
 *
 * Modes:
 *   node tools/secrets-guard.js              # human report
 *   node tools/secrets-guard.js --json       # JSON report
 *   node tools/secrets-guard.js --fingerprint-only  # only flag known-registered secrets
 *   node tools/secrets-guard.js --scan-git   # also scan git history
 *   node tools/secrets-guard.js --check-payload <file|-> # exit 1 if payload contains registered secret
 *   node tools/secrets-guard.js --write-report         # write SECURITY.md
 *
 * Self-improving: every run appends to logs/secrets-guard/history.jsonl and
 * learns new patterns. When a leak is detected, its (fingerprint, context)
 * goes into logs/secrets-guard/learned-patterns.json so future scans catch
 * variations faster.
 */

const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');
const { loadHighValueEntries, SECRET_PATTERNS, fingerprint, mask } = require('./secrets/registry');

const WORKSPACE = path.join(__dirname, '..');
const LOG_DIR = path.join(WORKSPACE, 'logs', 'secrets-guard');
const HISTORY = path.join(LOG_DIR, 'history.jsonl');
const LEARNED = path.join(LOG_DIR, 'learned-patterns.json');
const REPORT = path.join(WORKSPACE, 'SECURITY.md');

const SKIP_DIRS = new Set(['node_modules', '.git', '.next', 'dist', 'build', '__pycache__']);
const SKIP_DIR_PREFIXES = ['.trash-cleanup-', 'temp-'];
const SKIP_FILE_EXTS = new Set(['.png', '.jpg', '.jpeg', '.gif', '.webp', '.ico', '.pdf', '.zip', '.tar', '.gz', '.exe', '.dll', '.woff', '.woff2', '.mp4', '.mov']);
const MAX_FILE_BYTES = 5 * 1024 * 1024;

const args = new Set(process.argv.slice(2));
const getArg = (name) => {
  const i = process.argv.indexOf(name);
  return i >= 0 ? process.argv[i + 1] : null;
};

function ensureLogDir() { if (!fs.existsSync(LOG_DIR)) fs.mkdirSync(LOG_DIR, { recursive: true }); }

function loadLearned() {
  try { return JSON.parse(fs.readFileSync(LEARNED, 'utf8')); } catch (_) { return { fingerprints: {}, patterns: [] }; }
}
function saveLearned(l) { ensureLogDir(); fs.writeFileSync(LEARNED, JSON.stringify(l, null, 2)); }

function* walk(dir) {
  let entries;
  try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch (_) { return; }
  for (const ent of entries) {
    const full = path.join(dir, ent.name);
    if (ent.isDirectory()) {
      if (SKIP_DIRS.has(ent.name)) continue;
      if (SKIP_DIR_PREFIXES.some(p => ent.name.startsWith(p))) continue;
      yield* walk(full);
    } else if (ent.isFile()) {
      const ext = path.extname(ent.name).toLowerCase();
      if (SKIP_FILE_EXTS.has(ext)) continue;
      yield full;
    }
  }
}

function readSafe(p) {
  try {
    const stat = fs.statSync(p);
    if (stat.size > MAX_FILE_BYTES) return null;
    return fs.readFileSync(p, 'utf8');
  } catch (_) { return null; }
}

// Filter obvious placeholder/example values from pattern matches.
const PLACEHOLDER_RE = /(xxx|your|example|placeholder|dummy|fake|redacted|\.\.\.|<.+?>)/i;

function isPlaceholder(val) {
  if (PLACEHOLDER_RE.test(val)) return true;
  // low-entropy check: mostly repeated chars or too short of a unique set
  const unique = new Set(val).size;
  return unique < 6;
}

function scanContent(content, { registered, learned }) {
  const hits = [];
  // Registered (fingerprint) matches - highest confidence.
  for (const entry of registered) {
    if (content.includes(entry.value)) {
      hits.push({ kind: 'registered', id: entry.id, label: entry.label, section: entry.section, masked: entry.masked, fingerprint: entry.fingerprint, severity: 'critical' });
    }
  }
  // Pattern matches - may or may not be registered.
  for (const p of SECRET_PATTERNS) {
    p.re.lastIndex = 0;
    const seen = new Set();
    let m;
    while ((m = p.re.exec(content)) !== null) {
      const val = m[0];
      if (seen.has(val)) continue;
      seen.add(val);
      if (isPlaceholder(val)) continue;
      const fp = fingerprint(val);
      const isRegistered = registered.some(e => e.fingerprint === fp);
      hits.push({ kind: 'pattern', pattern: p.name, masked: mask(val), fingerprint: fp, severity: p.severity, registered: isRegistered });
    }
  }
  // Learned fingerprints - historical leaks that may still linger.
  for (const fp of Object.keys(learned.fingerprints || {})) {
    const meta = learned.fingerprints[fp];
    if (meta.value && content.includes(meta.value)) {
      hits.push({ kind: 'learned', fingerprint: fp, masked: mask(meta.value), severity: meta.severity || 'high', note: meta.note || 'previously-leaked' });
    }
  }
  return hits;
}

function scanWorkspace() {
  const registered = loadHighValueEntries();
  const learned = loadLearned();
  const findings = [];
  const secretsAbs = path.join(WORKSPACE, 'secrets.md');
  const vaultAbs = path.join(WORKSPACE, 'secrets.md.enc');

  for (const file of walk(WORKSPACE)) {
    if (file === secretsAbs) continue; // canonical plaintext store, skip
    if (file === vaultAbs) continue;   // encrypted vault, opaque
    const rel = path.relative(WORKSPACE, file);
    // Skip the guard's own logs (would be self-referential noise)
    if (rel.startsWith('logs\\secrets-guard') || rel.startsWith('logs/secrets-guard')) continue;
    const content = readSafe(file);
    if (content == null) continue;
    const hits = scanContent(content, { registered, learned });
    if (hits.length) findings.push({ file: rel, hits });
  }
  return { findings, registered, learned };
}

function scanGitHistory() {
  const registered = loadHighValueEntries();
  const learned = loadLearned();
  const findings = [];
  let log;
  try { log = execSync('git log --all --format=%H', { cwd: WORKSPACE, encoding: 'utf8' }); }
  catch (_) { return { findings: [], note: 'no git repo or git not available' }; }
  const commits = log.trim().split('\n').filter(Boolean);
  for (const c of commits) {
    let diff;
    try { diff = execSync(`git show --no-color ${c}`, { cwd: WORKSPACE, encoding: 'utf8', maxBuffer: 20 * 1024 * 1024 }); }
    catch (_) { continue; }
    const hits = scanContent(diff, { registered, learned });
    if (hits.length) findings.push({ commit: c, hits });
  }
  return { findings };
}

function learnFromFindings(findings, learned) {
  // Any pattern hit that looks like a real secret (registered=true, or matches critical pattern) gets fingerprinted.
  for (const f of findings) {
    for (const h of f.hits) {
      if (h.kind === 'registered' && h.fingerprint) {
        learned.fingerprints[h.fingerprint] = learned.fingerprints[h.fingerprint] || { firstSeen: new Date().toISOString(), label: h.label, severity: h.severity, note: 'registered secret seen outside secrets.md' };
        learned.fingerprints[h.fingerprint].lastSeen = new Date().toISOString();
      }
    }
  }
  return learned;
}

function summarize(findings) {
  const bySeverity = { critical: 0, high: 0, medium: 0, low: 0 };
  const byFile = {};
  for (const f of findings) {
    for (const h of f.hits) {
      bySeverity[h.severity] = (bySeverity[h.severity] || 0) + 1;
    }
    byFile[f.file || f.commit] = f.hits.length;
  }
  return { total: findings.reduce((n, f) => n + f.hits.length, 0), bySeverity, byFile };
}

function writeReport(ws, gh) {
  const sum = summarize(ws.findings);
  const ghSum = gh ? summarize(gh.findings) : null;
  const now = new Date().toISOString();
  const sev = ['critical', 'high', 'medium', 'low'];
  let md = `# SECURITY.md\n\n_Auto-generated by tools/secrets-guard.js on ${now}_\n\n`;
  md += `## Summary\n- Workspace findings: ${sum.total} (critical:${sum.bySeverity.critical||0} high:${sum.bySeverity.high||0} medium:${sum.bySeverity.medium||0})\n`;
  if (ghSum) md += `- Git history findings: ${ghSum.total} across ${Object.keys(ghSum.byFile).length} commits\n`;
  md += `- Registered secrets tracked: ${ws.registered.length}\n\n`;

  if (sum.total === 0) {
    md += `\n**Clean workspace scan.** No registered secrets or matching patterns found outside secrets.md.\n\n`;
  } else {
    md += `## Workspace findings\n\n`;
    for (const f of ws.findings) {
      md += `### ${f.file}\n`;
      for (const h of f.hits) {
        md += `- **${h.severity.toUpperCase()}** ${h.kind} `;
        if (h.kind === 'registered') md += `\`${h.label}\` (${h.section}) fp=\`${h.fingerprint}\``;
        else if (h.kind === 'pattern') md += `${h.pattern} masked=\`${h.masked}\` fp=\`${h.fingerprint}\`${h.registered ? ' (registered)' : ''}`;
        else md += `learned fp=\`${h.fingerprint}\` masked=\`${h.masked}\``;
        md += `\n`;
      }
      md += `\n`;
    }
  }

  if (gh && gh.findings.length) {
    md += `## Git history findings (${gh.findings.length} commits)\n\n`;
    for (const f of gh.findings.slice(0, 50)) {
      md += `- \`${f.commit.slice(0, 10)}\`: ${f.hits.length} hit(s) - ${f.hits.map(h => h.kind + ':' + (h.label || h.pattern || h.fingerprint)).join(', ')}\n`;
    }
    if (gh.findings.length > 50) md += `- ... ${gh.findings.length - 50} more\n`;
  }

  md += `\n## Registered secrets (masked)\n\n`;
  for (const r of ws.registered) md += `- \`${r.id}\` (${r.section} / ${r.label}): ${r.masked} fp=\`${r.fingerprint}\`\n`;

  md += `\n## How to fix\n- Any workspace hit: move the value into secrets.md, replace inline with \`secrets.get('<id>')\` via tools/secrets/accessor.js.\n- Any git-history hit with a **current** fingerprint means the live key is in history - rotate then purge with git-filter-repo.\n- For burned (rotated) keys, history is low-risk but note in incident log.\n`;
  fs.writeFileSync(REPORT, md);
}

function appendHistory(record) {
  ensureLogDir();
  fs.appendFileSync(HISTORY, JSON.stringify({ ts: new Date().toISOString(), ...record }) + '\n');
}

// --- check-payload: used as a pre-cron / pre-commit hook ---
function checkPayload(source) {
  const registered = loadHighValueEntries();
  const learned = loadLearned();
  let content;
  if (source === '-' || !source) {
    content = fs.readFileSync(0, 'utf8'); // stdin
  } else {
    content = fs.readFileSync(source, 'utf8');
  }
  const hits = scanContent(content, { registered, learned });
  if (hits.length) {
    console.error('SECRETS-GUARD: payload contains sensitive material.');
    for (const h of hits) {
      console.error(` - ${h.severity} ${h.kind} ${h.label || h.pattern || ''} ${h.masked || ''}`);
    }
    process.exit(2);
  }
  console.error('SECRETS-GUARD: payload clean.');
  process.exit(0);
}

// --- main ---
function main() {
  const json = args.has('--json');
  const fingerprintOnly = args.has('--fingerprint-only');
  const doGit = args.has('--scan-git');
  const writeReportFlag = args.has('--write-report');
  const checkArg = process.argv.indexOf('--check-payload');

  if (checkArg >= 0) {
    return checkPayload(process.argv[checkArg + 1] || '-');
  }

  const ws = scanWorkspace();
  if (fingerprintOnly) ws.findings = ws.findings.map(f => ({ ...f, hits: f.hits.filter(h => h.kind !== 'pattern' || h.registered) })).filter(f => f.hits.length);
  const gh = doGit ? scanGitHistory() : null;

  // Learn.
  const updated = learnFromFindings(ws.findings, ws.learned);
  if (gh) learnFromFindings(gh.findings, updated);
  saveLearned(updated);

  const sum = summarize(ws.findings);
  appendHistory({ mode: doGit ? 'ws+git' : 'ws', summary: sum, gitCommitsWithHits: gh ? gh.findings.length : null });

  if (writeReportFlag) writeReport(ws, gh);

  if (json) {
    process.stdout.write(JSON.stringify({ workspace: ws.findings, git: gh ? gh.findings : null, summary: sum, registeredCount: ws.registered.length }, null, 2) + '\n');
    return;
  }

  console.log(`secrets-guard: ${sum.total} finding(s), ${ws.registered.length} registered secrets.`);
  console.log(` critical:${sum.bySeverity.critical||0} high:${sum.bySeverity.high||0} medium:${sum.bySeverity.medium||0}`);
  if (sum.total === 0) { console.log(' clean.'); return; }
  for (const f of ws.findings) {
    console.log(`\n ${f.file}`);
    for (const h of f.hits) {
      console.log(`   - ${h.severity} ${h.kind} ${h.label || h.pattern || ''} ${h.masked || ''}`);
    }
  }
  if (gh) {
    console.log(`\n git history: ${gh.findings.length} commit(s) with hits`);
    for (const f of gh.findings.slice(0, 10)) console.log(`   - ${f.commit.slice(0,10)}: ${f.hits.length}`);
  }
}

main();
