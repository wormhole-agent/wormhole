// Shipped with WormHole v0.1.0. See workspace/tools/TOOLS.md for usage.
//
// registry.js - parse secrets.md into a structured registry
// Never exports raw values in logs; only the accessor returns plaintext.

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const vault = require('./vault');

const SECRETS_PATH = path.join(__dirname, '..', '..', 'secrets.md');

// Patterns we recognise as "things that look like secrets" on any line.
// Used both for scanning external files AND for validating registry values.
const SECRET_PATTERNS = [
  { name: 'stripe_live',      re: /\bsk_live_[A-Za-z0-9]{24,}\b/g,                    severity: 'critical' },
  { name: 'stripe_test',      re: /\bsk_test_[A-Za-z0-9]{24,}\b/g,                    severity: 'high' },
  { name: 'stripe_restricted',re: /\brk_(?:live|test)_[A-Za-z0-9]{24,}\b/g,           severity: 'critical' },
  { name: 'openai',           re: /\bsk-(?:proj-)?[A-Za-z0-9_-]{20,}\b/g,              severity: 'critical' },
  { name: 'anthropic',        re: /\bsk-ant-[A-Za-z0-9_-]{20,}\b/g,                    severity: 'critical' },
  { name: 'google_api',       re: /\bAIza[A-Za-z0-9_-]{35}\b/g,                        severity: 'high' },
  { name: 'aws_access',       re: /\b(?:AKIA|ASIA)[A-Z0-9]{16}\b/g,                    severity: 'critical' },
  { name: 'github_pat',       re: /\bghp_[A-Za-z0-9]{36}\b/g,                          severity: 'critical' },
  { name: 'github_oauth',     re: /\bgho_[A-Za-z0-9]{36}\b/g,                          severity: 'critical' },
  { name: 'github_server',    re: /\bghs_[A-Za-z0-9]{36}\b/g,                          severity: 'critical' },
  { name: 'github_user',      re: /\bghu_[A-Za-z0-9]{36}\b/g,                          severity: 'critical' },
  { name: 'resend',           re: /\bre_[A-Za-z0-9]{8}_[A-Za-z0-9]{20,}\b/g,           severity: 'high' },
  { name: 'deepseek',         re: /\bsk-[a-f0-9]{32}\b/g,                              severity: 'medium' },
  { name: 'telegram_bot',     re: /\b\d{9,11}:[A-Za-z0-9_-]{30,}\b/g,                  severity: 'high' },
  { name: 'slack',            re: /\bxox[baprs]-[A-Za-z0-9-]{10,}\b/g,                 severity: 'high' },
  { name: 'jwt',              re: /\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b/g, severity: 'medium' },
  { name: 'generic_pem',      re: /-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----/g, severity: 'critical' },
];

function fingerprint(value) {
  return crypto.createHash('sha256').update(value).digest('hex').slice(0, 16);
}

function mask(value) {
  if (!value) return '';
  if (value.length <= 12) return value.slice(0, 3) + '...';
  return value.slice(0, 7) + '...' + value.slice(-4);
}

// Parse secrets.md - extracts `**Key Name**: value` pairs under H3 section headers.
function parseSecretsMd(content) {
  const lines = content.split(/\r?\n/);
  const entries = [];
  let section = 'root';
  for (const line of lines) {
    const hmatch = line.match(/^###\s+(.+?)\s*$/);
    if (hmatch) { section = hmatch[1].trim(); continue; }
    // - **Label**: value    OR    * **Label**: value    OR    **Label**: value
    const kmatch = line.match(/^\s*[-*]?\s*\*\*([^*]+?)\*\*\s*:\s*(.+?)\s*$/);
    if (kmatch) {
      const label = kmatch[1].trim();
      const value = kmatch[2].trim().replace(/^`+|`+$/g, '');
      if (value && value.length > 6 && !/^(pending|tbd|none|n\/a|\(.+\))$/i.test(value)) {
        entries.push({ section, label, value });
      }
    }
  }
  return entries;
}

function loadRegistry() {
  // Prefer encrypted vault when present; fall back to plaintext secrets.md.
  const { content } = vault.readSecrets();
  const entries = parseSecretsMd(content);
  return entries.map(e => ({
    id: (e.section + '.' + e.label).toLowerCase().replace(/[^a-z0-9]+/g, '.').replace(/^\.|\.$/g, ''),
    section: e.section,
    label: e.label,
    value: e.value,
    masked: mask(e.value),
    fingerprint: fingerprint(e.value),
  }));
}

// Only entries whose value matches a known high-value secret pattern.
function loadHighValueEntries() {
  const all = loadRegistry();
  return all.filter(e => {
    for (const p of SECRET_PATTERNS) {
      p.re.lastIndex = 0;
      if (p.re.test(e.value)) return true;
    }
    return false;
  }).map(e => {
    let matchedPattern = null;
    for (const p of SECRET_PATTERNS) {
      p.re.lastIndex = 0;
      if (p.re.test(e.value)) { matchedPattern = p.name; break; }
    }
    return { ...e, pattern: matchedPattern };
  });
}

module.exports = {
  SECRETS_PATH,
  SECRET_PATTERNS,
  fingerprint,
  mask,
  parseSecretsMd,
  loadRegistry,
  loadHighValueEntries,
};
