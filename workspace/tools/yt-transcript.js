#!/usr/bin/env node
// Shipped with WormHole v0.1.0. See workspace/tools/TOOLS.md for usage.
//
/**
 * yt-transcript.js - Fetch a YouTube video transcript (captions) as plain text.
 *
 * Usage:
 *   node tools/yt-transcript.js <url-or-id>
 *   node tools/yt-transcript.js --json <url-or-id>
 *   node tools/yt-transcript.js --save <url-or-id>  # writes to logs/yt-transcripts/<id>.txt
 *
 * Requires: Python + yt-dlp (already installed in workspace env).
 */

const { execFileSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');

function parseArgs(argv) {
  const args = { json: false, save: false, url: null };
  for (const a of argv) {
    if (a === '--json') args.json = true;
    else if (a === '--save') args.save = true;
    else if (!args.url) args.url = a;
  }
  return args;
}

function videoIdFromUrl(urlOrId) {
  // Accept bare IDs (11 chars, no slashes) as-is
  if (/^[A-Za-z0-9_-]{11}$/.test(urlOrId)) return urlOrId;
  const m =
    urlOrId.match(/[?&]v=([A-Za-z0-9_-]{11})/) ||
    urlOrId.match(/youtu\.be\/([A-Za-z0-9_-]{11})/) ||
    urlOrId.match(/\/shorts\/([A-Za-z0-9_-]{11})/) ||
    urlOrId.match(/\/embed\/([A-Za-z0-9_-]{11})/);
  return m ? m[1] : null;
}

function vttToText(vtt) {
  const lines = vtt.split(/\r?\n/);
  const out = [];
  let lastCue = null;
  for (const raw of lines) {
    const line = raw.trim();
    if (!line) continue;
    if (line.startsWith('WEBVTT') || line.startsWith('Kind:') || line.startsWith('Language:')) continue;
    if (/-->/.test(line)) continue; // timing line
    if (/^\d+$/.test(line)) continue; // cue index
    // Strip inline timing tags like <00:00:00.000>
    const cleaned = line.replace(/<\d\d:\d\d:\d\d\.\d{3}>/g, '').replace(/<[^>]+>/g, '').trim();
    if (!cleaned) continue;
    if (cleaned === lastCue) continue; // dedupe rolling-window captions
    out.push(cleaned);
    lastCue = cleaned;
  }
  return out.join(' ').replace(/\s+/g, ' ').trim();
}

function fetchTranscript(videoId) {
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'yt-sub-'));
  try {
    // Try manual subs first, then auto, in English
    const url = `https://www.youtube.com/watch?v=${videoId}`;
    const outTemplate = path.join(tmpDir, '%(id)s.%(ext)s');
    try {
      execFileSync('python', [
        '-m', 'yt_dlp',
        '--skip-download',
        '--write-subs',
        '--write-auto-subs',
        '--sub-langs', 'en.*,en',
        '--sub-format', 'vtt',
        '--convert-subs', 'vtt',
        '-o', outTemplate,
        url,
      ], { stdio: ['ignore', 'pipe', 'pipe'] });
    } catch (e) {
      // non-zero exit still sometimes writes subs; fall through
    }

    const files = fs.readdirSync(tmpDir).filter((f) => f.endsWith('.vtt'));
    if (files.length === 0) {
      throw new Error('No captions available for this video (no .vtt produced)');
    }
    // Prefer non-auto if both exist
    files.sort((a, b) => {
      const aAuto = /\.auto\./i.test(a) || /\.a\./i.test(a);
      const bAuto = /\.auto\./i.test(b) || /\.a\./i.test(b);
      if (aAuto === bAuto) return 0;
      return aAuto ? 1 : -1;
    });
    const vtt = fs.readFileSync(path.join(tmpDir, files[0]), 'utf8');
    return { text: vttToText(vtt), sourceFile: files[0] };
  } finally {
    try {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    } catch (_) { /* ignore */ }
  }
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  if (!args.url) {
    console.error('Usage: node tools/yt-transcript.js [--json] [--save] <url-or-id>');
    process.exit(1);
  }
  const id = videoIdFromUrl(args.url);
  if (!id) {
    console.error('Could not parse video id from:', args.url);
    process.exit(1);
  }
  const { text, sourceFile } = fetchTranscript(id);
  const payload = { videoId: id, sourceFile, charCount: text.length, text };
  if (args.save) {
    const dir = path.join(__dirname, '..', 'logs', 'yt-transcripts');
    fs.mkdirSync(dir, { recursive: true });
    const file = path.join(dir, `${id}.txt`);
    fs.writeFileSync(file, text, 'utf8');
    payload.savedTo = file;
  }
  if (args.json) {
    console.log(JSON.stringify(payload, null, 2));
  } else {
    console.log(text);
    if (payload.savedTo) console.error(`\n[saved to ${payload.savedTo}]`);
  }
}

main();
