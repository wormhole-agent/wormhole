#!/usr/bin/env node
// Shipped with WormHole v0.1.0. See workspace/tools/TOOLS.md for usage.
//
/**
 * Triage Router - Small-model pre-filter before Sonnet
 * Uses qwen2.5:2b locally to classify tasks.
 * Routes simple/cheap tasks to Gemma4:26b, complex to Sonnet.
 *
 * Usage (as library):
 *   const { route } = require('./triage-router');
 *   const { model, reason } = await route(prompt);
 *   // model: 'gemma4:26b' | 'sonnet' | 'opus'
 *
 * Usage (CLI):
 *   echo "summarize this text" | node tools/triage-router.js
 *   node tools/triage-router.js "is this a sales lead?"
 */

const { execSync } = require('child_process');

// Complexity signals that warrant Sonnet or above
const SONNET_SIGNALS = [
  'write', 'draft', 'compose', 'generate', 'create', 'build',
  'analyze', 'research', 'compare', 'evaluate', 'plan', 'strategy',
  'code', 'script', 'implement', 'refactor', 'debug',
  'explain', 'summarize long', 'deep dive',
];

const OPUS_SIGNALS = [
  'strategy', 'decision', 'tradeoff', 'architecture', 'design system',
  'should i', 'what should we', 'big picture',
];

/**
 * Fast heuristic classification (no LLM call).
 * Returns 'gemma4:26b' | 'sonnet' | 'opus'
 */
function heuristicRoute(prompt) {
  const lower = prompt.toLowerCase();

  for (const sig of OPUS_SIGNALS) {
    if (lower.includes(sig)) return { model: 'ollama/gemma4:26b', reason: `heuristic:opus-signal:${sig}`, escalated: true };
  }

  for (const sig of SONNET_SIGNALS) {
    if (lower.includes(sig)) return { model: 'anthropic/claude-sonnet-4-6', reason: `heuristic:sonnet-signal:${sig}`, escalated: true };
  }

  // Short prompts under 50 words: local model handles it
  const wordCount = prompt.split(/\s+/).length;
  if (wordCount < 50) return { model: 'ollama/gemma4:26b', reason: `heuristic:short-prompt:${wordCount}w`, escalated: false };

  return { model: 'anthropic/claude-sonnet-4-6', reason: 'heuristic:default-sonnet', escalated: true };
}

/**
 * LLM-assisted classification using qwen2.5:2b.
 * Only called when heuristic is ambiguous.
 */
function llmRoute(prompt) {
  const classifyPrompt = `Classify this task. Reply with exactly one word: LOCAL, SONNET, or OPUS.

LOCAL = simple lookup, yes/no, short extraction, formatting, math
SONNET = writing, coding, analysis, multi-step reasoning
OPUS = strategy, architecture decisions, tradeoffs with no clear answer

Task: ${prompt.slice(0, 300)}

Reply:`;

  try {
    const result = execSync(
      `ollama run qwen2.5:1.5b "${classifyPrompt.replace(/"/g, '\\"')}"`,
      { timeout: 10000, encoding: 'utf8' }
    ).trim().toUpperCase();

    if (result.includes('OPUS')) return { model: 'ollama/gemma4:26b', reason: 'llm-triage:OPUS->gemma4', escalated: true };
    if (result.includes('SONNET')) return { model: 'anthropic/claude-sonnet-4-6', reason: 'llm-triage:SONNET', escalated: true };
    return { model: 'ollama/gemma4:26b', reason: 'llm-triage:LOCAL', escalated: false };
  } catch {
    // qwen2.5:2b not available or timed out - fall back to heuristic
    return heuristicRoute(prompt);
  }
}

/**
 * Main router. Heuristic first, LLM fallback for ambiguous cases.
 */
function route(prompt) {
  const heuristic = heuristicRoute(prompt);
  // If heuristic is confident (has a specific signal match), use it
  if (heuristic.reason !== 'heuristic:default-sonnet') return heuristic;
  // Ambiguous - ask the 2B model
  return llmRoute(prompt);
}

module.exports = { route, heuristicRoute, llmRoute };

// CLI
if (require.main === module) {
  let prompt = process.argv[2];
  if (!prompt && !process.stdin.isTTY) {
    prompt = require('fs').readFileSync('/dev/stdin', 'utf8').trim();
  }
  if (!prompt) {
    console.error('Usage: node tools/triage-router.js "<prompt>"');
    process.exit(1);
  }

  const result = route(prompt);
  console.log(JSON.stringify(result, null, 2));
}
