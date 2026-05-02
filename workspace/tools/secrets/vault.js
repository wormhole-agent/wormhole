// Shipped with WormHole v0.1.0. See workspace/tools/TOOLS.md for usage.
//
// vault.js - DPAPI-backed encrypt/decrypt for secrets.md
//
// Windows DPAPI (user scope) binds the ciphertext to the current Windows
// user account. The encrypted blob is useless on another machine or under
// another user, with no key file of our own to manage.
//
// We shell out to PowerShell's System.Security.Cryptography.ProtectedData
// because it's built-in. No npm dependency.

const fs = require('fs');
const path = require('path');
const os = require('os');
const crypto = require('crypto');
const { spawnSync } = require('child_process');

const WORKSPACE = path.join(__dirname, '..', '..');
const PLAINTEXT = path.join(WORKSPACE, 'secrets.md');
const CIPHERTEXT = path.join(WORKSPACE, 'secrets.md.enc');

function runPowerShell(script) {
  const res = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], {
    encoding: 'buffer',
    maxBuffer: 20 * 1024 * 1024,
  });
  if (res.status !== 0) {
    const err = res.stderr ? res.stderr.toString('utf8') : 'unknown powershell error';
    throw new Error(`powershell failed (${res.status}): ${err.trim()}`);
  }
  return res.stdout;
}

// --- Encrypt a utf8 string -> base64 ciphertext on disk.
function encryptToFile(plaintext, filePath = CIPHERTEXT) {
  const b64in = Buffer.from(plaintext, 'utf8').toString('base64');
  const script = `
    Add-Type -AssemblyName System.Security;
    $bytes = [Convert]::FromBase64String('${b64in}');
    $enc = [System.Security.Cryptography.ProtectedData]::Protect($bytes, $null, 'CurrentUser');
    [Convert]::ToBase64String($enc);
  `;
  const out = runPowerShell(script).toString('utf8').trim();
  // Wrap with a small header so we can detect format changes later.
  const payload = {
    v: 1,
    scope: 'CurrentUser',
    user: os.userInfo().username,
    host: os.hostname(),
    createdAt: new Date().toISOString(),
    sha256_plain: crypto.createHash('sha256').update(plaintext, 'utf8').digest('hex'),
    ciphertextB64: out,
  };
  fs.writeFileSync(filePath, JSON.stringify(payload, null, 2));
}

// --- Decrypt the ciphertext file -> utf8 string (in memory only).
function decryptFromFile(filePath = CIPHERTEXT) {
  const raw = fs.readFileSync(filePath, 'utf8');
  const payload = JSON.parse(raw);
  if (payload.v !== 1) throw new Error(`unsupported vault version: ${payload.v}`);
  const script = `
    Add-Type -AssemblyName System.Security;
    $enc = [Convert]::FromBase64String('${payload.ciphertextB64}');
    try {
      $dec = [System.Security.Cryptography.ProtectedData]::Unprotect($enc, $null, 'CurrentUser');
      [Convert]::ToBase64String($dec);
    } catch {
      Write-Error $_.Exception.Message;
      exit 1;
    }
  `;
  const b64out = runPowerShell(script).toString('utf8').trim();
  const plaintext = Buffer.from(b64out, 'base64').toString('utf8');
  const fp = crypto.createHash('sha256').update(plaintext, 'utf8').digest('hex');
  if (payload.sha256_plain && payload.sha256_plain !== fp) {
    throw new Error('decryption integrity check failed (sha256 mismatch)');
  }
  return plaintext;
}

function hasVault() { return fs.existsSync(CIPHERTEXT); }
function hasPlaintext() { return fs.existsSync(PLAINTEXT); }

// Read secrets.md content, preferring encrypted vault if present.
// Returns { source: 'vault'|'plaintext', content: string }
function readSecrets() {
  if (hasVault()) {
    return { source: 'vault', content: decryptFromFile() };
  }
  if (hasPlaintext()) {
    return { source: 'plaintext', content: fs.readFileSync(PLAINTEXT, 'utf8') };
  }
  throw new Error('no secrets source found (neither secrets.md nor secrets.md.enc)');
}

// lock(): encrypt plaintext, then securely remove plaintext file.
function lock() {
  if (!hasPlaintext()) {
    if (hasVault()) return { status: 'already-locked', vault: CIPHERTEXT };
    throw new Error('nothing to lock: secrets.md not found');
  }
  const content = fs.readFileSync(PLAINTEXT, 'utf8');
  encryptToFile(content, CIPHERTEXT);
  // Overwrite with zeros before unlink to reduce disk-recovery risk.
  try {
    const stat = fs.statSync(PLAINTEXT);
    fs.writeFileSync(PLAINTEXT, Buffer.alloc(Math.min(stat.size, 64 * 1024), 0));
  } catch (_) { /* best effort */ }
  fs.unlinkSync(PLAINTEXT);
  return { status: 'locked', vault: CIPHERTEXT };
}

// unlock(): decrypt vault back to plaintext file for editing. Dangerous - caller accepts this.
function unlock() {
  if (!hasVault()) throw new Error('no vault to unlock');
  const content = decryptFromFile();
  fs.writeFileSync(PLAINTEXT, content, { mode: 0o600 });
  return { status: 'unlocked', plaintext: PLAINTEXT };
}

module.exports = {
  PLAINTEXT,
  CIPHERTEXT,
  hasVault,
  hasPlaintext,
  readSecrets,
  encryptToFile,
  decryptFromFile,
  lock,
  unlock,
};
