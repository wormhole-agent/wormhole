#!/usr/bin/env node
// Shipped with WormHole v0.1.0. See workspace/tools/TOOLS.md for usage.
//
/**
 * tools/telegram-send.js
 *
 * Send a Telegram message to a configured default chat (or any chat) from any script or cron task.
 *
 * Reads bot credentials from the DPAPI vault via tools/secrets/accessor.js.
 * which DPAPI-decrypts silently for processes running as the current Windows user. No interactive
 * `unlock` step required - this is safe for unattended cron jobs.
 *
 * Usage from CLI:
 *   node tools/telegram-send.js "Your message"
 *   node tools/telegram-send.js --chat <your-chat-id> "Your message"
 *   echo "long message" | node tools/telegram-send.js --stdin
 *   node tools/telegram-send.js --reply-to 1234 "Reply text"
 *   node tools/telegram-send.js --silent "Quiet message"            # no notification
 *
 * Usage from another script:
 *   const { send } = require('./tools/telegram-send');
 *   await send('Hello');
 *   await send('Hi', { chatId: '<your-chat-id>', silent: true });
 *
 * Exits non-zero on failure with a short message on stderr. Cron-safe:
 * never throws unhandled exceptions, never blocks longer than 10 seconds.
 */

const https = require('https');
const path = require('path');
const fs = require('fs');

// -------- credentials --------

function getCreds() {
  // Env wins (handy for tests, dev overrides). Otherwise vault.
  const fromEnv = {
    token: process.env.TELEGRAM_BOT_TOKEN,
    chatId: process.env.TELEGRAM_DEFAULT_CHAT_ID,
  };
  if (fromEnv.token && fromEnv.chatId) return fromEnv;

  const { getByLabel } = require(path.join(__dirname, 'secrets', 'accessor'));
  return {
    token: fromEnv.token || getByLabel('Bot Token', 'Telegram Bot'),
    chatId: fromEnv.chatId || getByLabel('Default Chat ID', 'Telegram Bot'),
  };
}

// -------- send --------

/**
 * @param {string} text
 * @param {object} [opts]
 * @param {string} [opts.chatId]       override default chat
 * @param {string|number} [opts.replyTo]  reply_to_message_id
 * @param {boolean} [opts.silent]      disable_notification = true
 * @param {string} [opts.parseMode]    'Markdown' | 'MarkdownV2' | 'HTML' (default: none)
 * @param {number} [opts.timeoutMs]    HTTP timeout (default 10000)
 * @returns {Promise<{ok:boolean, messageId?:number, error?:string}>}
 */
function send(text, opts = {}) {
  if (!text || typeof text !== 'string') {
    return Promise.resolve({ ok: false, error: 'no text' });
  }

  // Telegram caps single message at 4096 chars. Truncate with a marker.
  const MAX = 4096;
  let body = text;
  if (body.length > MAX) {
    body = body.slice(0, MAX - 30) + '\n\n[... truncated]';
  }

  let creds;
  try {
    creds = getCreds();
  } catch (e) {
    return Promise.resolve({ ok: false, error: `credentials: ${e.message}` });
  }
  if (!creds.token || !creds.chatId) {
    return Promise.resolve({ ok: false, error: 'missing token or chat id' });
  }

  const payload = {
    chat_id: opts.chatId || creds.chatId,
    text: body,
  };
  if (opts.replyTo) payload.reply_to_message_id = Number(opts.replyTo);
  if (opts.silent) payload.disable_notification = true;
  if (opts.parseMode) payload.parse_mode = opts.parseMode;

  const json = JSON.stringify(payload);
  const timeoutMs = opts.timeoutMs || 10000;

  return new Promise((resolve) => {
    const req = https.request({
      hostname: 'api.telegram.org',
      path: `/bot${creds.token}/sendMessage`,
      method: 'POST',
      timeout: timeoutMs,
      headers: {
        'Content-Type': 'application/json',
        'Content-Length': Buffer.byteLength(json),
      },
    }, (res) => {
      let chunks = '';
      res.on('data', (d) => { chunks += d.toString(); });
      res.on('end', () => {
        try {
          const parsed = JSON.parse(chunks);
          if (parsed.ok) {
            resolve({ ok: true, messageId: parsed.result.message_id });
          } else {
            resolve({ ok: false, error: parsed.description || `HTTP ${res.statusCode}` });
          }
        } catch (e) {
          resolve({ ok: false, error: `parse: ${e.message}` });
        }
      });
    });
    req.on('error', (e) => resolve({ ok: false, error: e.message }));
    req.on('timeout', () => { req.destroy(); resolve({ ok: false, error: 'timeout' }); });
    req.write(json);
    req.end();
  });
}

module.exports = { send };

// -------- CLI --------

if (require.main === module) {
  (async () => {
    const args = process.argv.slice(2);
    let chatId, replyTo, silent = false, parseMode, useStdin = false;
    const positional = [];

    for (let i = 0; i < args.length; i++) {
      const a = args[i];
      if (a === '--chat' || a === '--target') { chatId = args[++i]; }
      else if (a === '--reply-to') { replyTo = args[++i]; }
      else if (a === '--silent') { silent = true; }
      else if (a === '--markdown') { parseMode = 'MarkdownV2'; }
      else if (a === '--html') { parseMode = 'HTML'; }
      else if (a === '--stdin') { useStdin = true; }
      else if (a === '--help' || a === '-h') {
        console.log(fs.readFileSync(__filename, 'utf8').split('\n').slice(1, 24).join('\n').replace(/^\s*\*\s?/gm, ''));
        process.exit(0);
      }
      else { positional.push(a); }
    }

    let text = positional.join(' ');
    if (useStdin) {
      text = await new Promise((resolve) => {
        let buf = '';
        process.stdin.setEncoding('utf8');
        process.stdin.on('data', (d) => { buf += d; });
        process.stdin.on('end', () => resolve(buf.trim()));
      });
    }

    if (!text) {
      console.error('telegram-send: no message text. Use a positional arg or --stdin.');
      process.exit(2);
    }

    const result = await send(text, { chatId, replyTo, silent, parseMode });
    if (result.ok) {
      console.log(`sent: message_id=${result.messageId}`);
      process.exit(0);
    } else {
      console.error(`failed: ${result.error}`);
      process.exit(1);
    }
  })();
}
