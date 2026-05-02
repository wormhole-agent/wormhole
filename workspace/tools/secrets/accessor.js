// Shipped with WormHole v0.1.0. See workspace/tools/TOOLS.md for usage.
//
// accessor.js - the canonical way to read a secret.
// Logs every access to logs/secrets-guard/access.log with masked value only.
//
// Usage:
//   const { get, getByLabel } = require('./tools/secrets/accessor');
//   const key = get('providers.stripe.secret.key');
//   const key = getByLabel('Stripe Secret Key');       // convenience
//   const key = getByLabel('Stripe Secret Key', 'Providers');

const fs = require('fs');
const path = require('path');
const os = require('os');
const { loadRegistry, mask } = require('./registry');

const LOG_DIR = path.join(__dirname, '..', '..', 'logs', 'secrets-guard');
const LOG_FILE = path.join(LOG_DIR, 'access.log');

function ensureLogDir() {
  if (!fs.existsSync(LOG_DIR)) fs.mkdirSync(LOG_DIR, { recursive: true });
}

function logAccess(entry, event, meta = {}) {
  ensureLogDir();
  const line = JSON.stringify({
    ts: new Date().toISOString(),
    event,
    id: entry ? entry.id : null,
    label: entry ? entry.label : null,
    section: entry ? entry.section : null,
    masked: entry ? entry.masked : null,
    caller: meta.caller || callerPath(),
    pid: process.pid,
    user: os.userInfo().username,
    ...meta,
  }) + '\n';
  try { fs.appendFileSync(LOG_FILE, line); } catch (_) { /* best-effort */ }
}

function callerPath() {
  const stack = new Error().stack.split('\n').slice(3);
  for (const s of stack) {
    const m = s.match(/\((.+?):\d+:\d+\)/) || s.match(/at (.+?):\d+:\d+/);
    if (m && !m[1].includes('node:')) return m[1];
  }
  return 'unknown';
}

function get(id) {
  const reg = loadRegistry();
  const entry = reg.find(e => e.id === id);
  if (!entry) {
    logAccess(null, 'miss', { requested: id });
    throw new Error(`secret not found: ${id}`);
  }
  logAccess(entry, 'get');
  return entry.value;
}

function getByLabel(label, section = null) {
  const reg = loadRegistry();
  const matches = reg.filter(e => e.label.toLowerCase() === label.toLowerCase() && (section === null || e.section.toLowerCase() === section.toLowerCase()));
  if (matches.length === 0) {
    logAccess(null, 'miss', { requestedLabel: label, section });
    throw new Error(`secret not found by label: ${label}${section ? ' in ' + section : ''}`);
  }
  if (matches.length > 1 && !section) {
    logAccess(null, 'ambiguous', { requestedLabel: label, candidates: matches.map(m => m.id) });
    throw new Error(`ambiguous label "${label}" - found in ${matches.length} sections. Specify section or use get(id).`);
  }
  logAccess(matches[0], 'get');
  return matches[0].value;
}

// list() returns masked registry - safe to log / print.
function list() {
  return loadRegistry().map(e => ({ id: e.id, section: e.section, label: e.label, masked: e.masked, fingerprint: e.fingerprint }));
}

module.exports = { get, getByLabel, list };
