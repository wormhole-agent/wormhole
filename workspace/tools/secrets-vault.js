// Shipped with WormHole v0.1.0. See workspace/tools/TOOLS.md for usage.
//
#!/usr/bin/env node
/**
 * secrets-vault - manage the encrypted secrets.md vault.
 *
 *   node tools/secrets-vault.js status
 *   node tools/secrets-vault.js lock        # encrypt secrets.md -> secrets.md.enc, remove plaintext
 *   node tools/secrets-vault.js unlock      # decrypt secrets.md.enc -> secrets.md (for editing)
 *   node tools/secrets-vault.js verify      # round-trip decrypt and spot-check a few entries
 *   node tools/secrets-vault.js edit        # unlock, spawn editor, relock on exit
 */

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');
const vault = require('./secrets/vault');
const { loadRegistry } = require('./secrets/registry');

const cmd = process.argv[2];

function fmtState() {
  return {
    vault: vault.hasVault() ? vault.CIPHERTEXT : null,
    plaintext: vault.hasPlaintext() ? vault.PLAINTEXT : null,
  };
}

function status() {
  const s = fmtState();
  console.log(JSON.stringify(s, null, 2));
  if (s.vault && s.plaintext) {
    console.log('\nWARNING: both vault and plaintext present. Run `lock` to remove plaintext, or `unlock` + accept overwrite to replace.');
  } else if (s.vault) {
    console.log('\nState: LOCKED (vault present, no plaintext).');
  } else if (s.plaintext) {
    console.log('\nState: UNLOCKED (plaintext present, no vault). Consider `lock`.');
  } else {
    console.log('\nState: NO SECRETS found at either path.');
  }
}

function lock() {
  const r = vault.lock();
  console.log(`locked: ${r.vault}`);
}

function unlock() {
  if (vault.hasPlaintext()) {
    console.error('refusing: plaintext secrets.md already exists. Move it aside or lock first.');
    process.exit(2);
  }
  const r = vault.unlock();
  console.log(`unlocked: ${r.plaintext}`);
  console.log('remember to `lock` again when done editing.');
}

function verify() {
  try {
    const reg = loadRegistry();
    const withValues = reg.length;
    const masked = reg.slice(0, 5).map(r => `  ${r.id} -> ${r.masked}`).join('\n');
    console.log(`verify: OK, ${withValues} entries parsed`);
    console.log('sample (masked):');
    console.log(masked);
  } catch (e) {
    console.error('verify: FAILED', e.message);
    process.exit(1);
  }
}

function edit() {
  if (!vault.hasVault() && !vault.hasPlaintext()) {
    console.error('no secrets source to edit.');
    process.exit(1);
  }
  const unlocked = !vault.hasPlaintext();
  if (unlocked) vault.unlock();
  const editor = process.env.EDITOR || 'notepad.exe';
  console.log(`editing ${vault.PLAINTEXT} with ${editor} (close editor to relock)`);
  const r = spawnSync(editor, [vault.PLAINTEXT], { stdio: 'inherit', shell: true });
  if (r.status !== 0) console.warn(`editor exited with ${r.status}`);
  vault.lock();
  console.log(`relocked -> ${vault.CIPHERTEXT}`);
}

switch (cmd) {
  case 'status':  status();  break;
  case 'lock':    lock();    break;
  case 'unlock':  unlock();  break;
  case 'verify':  verify();  break;
  case 'edit':    edit();    break;
  default:
    console.log('usage: node tools/secrets-vault.js <status|lock|unlock|verify|edit>');
    process.exit(cmd ? 2 : 0);
}
