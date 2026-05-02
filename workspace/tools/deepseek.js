#!/usr/bin/env node
// Shipped with WormHole v0.1.0. See workspace/tools/TOOLS.md for usage.
//
/**
 * deepseek.js - DeepSeek API wrapper (chat + reasoner)
 *
 * Why this exists:
 *   DeepSeek is our mandatory "red-team / debias" step in chained research.
 *   Per Mo Gawdat's playbook: after Gemini/Claude give you an answer, hand it
 *   to DeepSeek and ask "what's wrong with this / what did they miss?"
 *   DeepSeek tends to push back on Western framing and surface blind spots.
 *
 * Usage:
 *   node tools/deepseek.js "your prompt"
 *   node tools/deepseek.js --model pro "hard problem"
 *   node tools/deepseek.js --model pro-thinking "red-team this thoroughly"
 *   node tools/deepseek.js --critique < draft.md
 *   echo "draft text" | node tools/deepseek.js --critique
 *   node tools/deepseek.js --json "prompt"
 *
 * Models (V4 era, released 2026-04-24):
 *   flash         - deepseek-v4-flash, non-thinking (default, $0.14/$0.28 per 1M)
 *   flash-thinking- deepseek-v4-flash with reasoning enabled
 *   pro           - deepseek-v4-pro, non-thinking (1.6T MoE, $1.74/$3.48 per 1M)
 *   pro-thinking  - deepseek-v4-pro with reasoning enabled (the heavy critique mode)
 *   chat          - legacy alias for v4-flash non-thinking
 *   reasoner      - legacy alias for v4-flash thinking
 *
 * When to use which:
 *   --critique on a draft  -> pro-thinking (highest-quality red-team)
 *   routine debias pass    -> flash-thinking (cheap, fast, still pushes back)
 *   bulk classification    -> flash (cheapest small model on the market)
 *   nothing               -> default flash is almost always right
 *
 * Auth:
 *   Set DEEPSEEK_API_KEY in env, or stash in workspace/secrets.md
 *   (key lookup falls back to a `DEEPSEEK_API_KEY=...` line in secrets.md)
 */

const https = require('https');
const fs = require('fs');
const path = require('path');

// --- CLI args ---
const args = process.argv.slice(2);
let model = null;        // resolved below: defaults to 'flash', or 'pro-thinking' for --critique
let critique = false;
let jsonOutput = false;
let readStdin = false;
const promptParts = [];

let fileInput = null;
let modelExplicit = false;
for (let i = 0; i < args.length; i++) {
  const a = args[i];
  if (a === '--model' && args[i + 1]) { model = args[++i]; modelExplicit = true; }
  else if (a === '--critique') { critique = true; }
  else if (a === '--json') { jsonOutput = true; }
  else if (a === '--stdin') { readStdin = true; }
  else if ((a === '--file' || a === '-f') && args[i + 1]) { fileInput = args[++i]; }
  else { promptParts.push(a); }
}

// Smart default: --critique upgrades to V4-Pro thinking unless model was set explicitly.
// Otherwise default to V4-Flash non-thinking (the cheap, fast everyday option).
if (!modelExplicit) {
  model = critique ? 'pro-thinking' : 'flash';
}

// --- Key lookup: env first, then secrets.md fallback ---
function getApiKey() {
  if (process.env.DEEPSEEK_API_KEY) return process.env.DEEPSEEK_API_KEY;
  try {
    const secretsPath = path.join(__dirname, '..', 'secrets.md');
    const text = fs.readFileSync(secretsPath, 'utf8');
    // Match formats: DEEPSEEK_API_KEY=sk-... or **DeepSeek API Key**: sk-...
    const m = text.match(/DEEPSEEK[_ ]?API[_ ]?KEY[^\S\r\n]*[:=][^\S\r\n]*`?([A-Za-z0-9_\-]+)`?/i)
          || text.match(/DeepSeek[^\n]*?\b(sk-[A-Za-z0-9_\-]{20,})/i);
    if (m) return m[1];
  } catch {}
  return null;
}

const apiKey = getApiKey();
if (!apiKey) {
  console.error('ERROR: DeepSeek API key not found.');
  console.error('Set DEEPSEEK_API_KEY env var, or add a line to secrets.md:');
  console.error('  DEEPSEEK_API_KEY=sk-xxxxx');
  console.error('Get one at: https://platform.deepseek.com/');
  process.exit(2);
}

// Resolve model alias -> { name, thinking }.
// V4 is a single model with a thinking-mode toggle, not two separate models.
const MODEL_MAP = {
  // V4 explicit
  flash:           { name: 'deepseek-v4-flash', thinking: false },
  'flash-thinking':{ name: 'deepseek-v4-flash', thinking: true  },
  pro:             { name: 'deepseek-v4-pro',   thinking: false },
  'pro-thinking':  { name: 'deepseek-v4-pro',   thinking: true  },
  // Legacy aliases (DeepSeek docs: chat/reasoner == v4-flash non-thinking/thinking)
  chat:            { name: 'deepseek-chat',     thinking: false },
  v3:              { name: 'deepseek-chat',     thinking: false },
  reasoner:        { name: 'deepseek-reasoner', thinking: true  },
  r1:              { name: 'deepseek-reasoner', thinking: true  },
};
const resolved = MODEL_MAP[model] || { name: model, thinking: false };
const resolvedModel = resolved.name;
const thinkingMode = resolved.thinking;

// Read stdin if piped or --critique without arg
async function readAllStdin() {
  return new Promise((resolve) => {
    let data = '';
    process.stdin.setEncoding('utf8');
    process.stdin.on('data', (c) => (data += c));
    process.stdin.on('end', () => resolve(data));
  });
}

async function main() {
  let userPrompt = promptParts.join(' ').trim();

  // Source for piped/file content
  let sideInput = '';
  if (fileInput) {
    try { sideInput = fs.readFileSync(fileInput, 'utf8').trim(); }
    catch (e) { console.error(`Cannot read --file ${fileInput}: ${e.message}`); process.exit(1); }
  } else if ((critique || readStdin || !userPrompt) && !process.stdin.isTTY) {
    sideInput = (await readAllStdin()).trim();
  }

  if (sideInput) {
    userPrompt = critique
      ? `Red-team this. What's wrong, missing, biased, or too Western/American in framing? Point out blind spots and counter-arguments I should address:\n\n---\n${sideInput}\n---`
      : (userPrompt ? `${userPrompt}\n\n${sideInput}` : sideInput);
  }

  if (!userPrompt) {
    console.error('Usage: node tools/deepseek.js [--model chat|reasoner] [--critique] "prompt"');
    console.error('       echo "draft" | node tools/deepseek.js --critique');
    process.exit(1);
  }

  // V4 thinking mode is enabled via the `thinking: { type: 'enabled' }` field
  // when targeting the v4-* model names. Legacy reasoner model name implies
  // thinking already and does not need the flag.
  const payload = {
    model: resolvedModel,
    messages: [{ role: 'user', content: userPrompt }],
    stream: false,
  };
  if (thinkingMode && resolvedModel.startsWith('deepseek-v4-')) {
    payload.thinking = { type: 'enabled' };
  }
  const body = JSON.stringify(payload);

  const result = await new Promise((resolve, reject) => {
    const req = https.request(
      'https://api.deepseek.com/v1/chat/completions',
      {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${apiKey}`,
          'Content-Length': Buffer.byteLength(body),
        },
        timeout: 120000,
      },
      (res) => {
        let data = '';
        res.on('data', (c) => (data += c));
        res.on('end', () => {
          try {
            const j = JSON.parse(data);
            if (res.statusCode >= 400) return reject(new Error(`HTTP ${res.statusCode}: ${data.slice(0, 500)}`));
            resolve(j);
          } catch (e) {
            reject(new Error(`Bad JSON from DeepSeek (HTTP ${res.statusCode}): ${data.slice(0, 300)}`));
          }
        });
      }
    );
    req.on('error', reject);
    req.on('timeout', () => { req.destroy(new Error('timeout after 120s')); });
    req.write(body);
    req.end();
  });

  const msg = result.choices?.[0]?.message;
  const text = msg?.content || '';
  const reasoning = msg?.reasoning_content || null; // R1 only

  if (jsonOutput) {
    console.log(JSON.stringify({
      model: resolvedModel,
      text,
      reasoning,
      usage: result.usage,
    }, null, 2));
  } else {
    if (reasoning) {
      console.log('--- reasoning ---');
      console.log(reasoning);
      console.log('--- answer ---');
    }
    console.log(text);
  }
}

main().catch((e) => {
  console.error('DeepSeek error:', e.message);
  process.exit(3);
});
