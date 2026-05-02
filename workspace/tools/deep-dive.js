// Shipped with WormHole v0.1.0. See workspace/tools/TOOLS.md for usage.
//
#!/usr/bin/env node
/**
 * deep-dive.js
 *
 * Dumps a module's entire file tree (or an arbitrary directory) into a single
 * large-context LLM turn for strategic review. This is the "load 370K tokens
 * at once" pattern: stop pre-filtering, let a long-context model see all
 * connections.
 *
 * Usage:
 *   node tools/deep-dive.js <module-name> "Your question"
 *   node tools/deep-dive.js --path ./modules/web-biz "Audit this"
 *   node tools/deep-dive.js web-biz --model sonnet "Summarize active leads"
 *   node tools/deep-dive.js web-biz --provider gemini "What am I missing?"
 *   node tools/deep-dive.js web-biz --dry-run "prompt"    # preview, no API call
 *
 * Providers:
 *   --provider opus      Anthropic Claude Opus (200K context, expensive)
 *   --provider sonnet    Anthropic Claude Sonnet (200K, cheaper, default)
 *   --provider gemini    Google Gemini 2.5 Pro (2M context, cheapest for big dumps)
 *   --provider deepseek  DeepSeek V4-Pro (1M context, ~7x cheaper than Sonnet, MIT license)
 *
 * Secrets handled via tools/secrets/accessor.js. Never inline keys.
 */

const fs = require("fs");
const path = require("path");

// ---------- arg parsing ----------
const args = process.argv.slice(2);
if (args.length === 0 || args.includes("--help") || args.includes("-h")) {
  console.log(`
Usage:
  node tools/deep-dive.js <module> "prompt" [options]
  node tools/deep-dive.js --path <dir> "prompt" [options]

Options:
  --provider <opus|sonnet|gemini>   default: sonnet
  --model <alias>                   override model string
  --max-file-kb <n>                 skip files larger than this (default 200)
  --include-memory                  also include MEMORY.md + recent daily notes
  --include-globs <csv>             extra globs to include (default: **/*.md,**/*.js,**/*.ts,**/*.json,**/*.html,**/*.css,**/*.txt,**/*.py)
  --exclude-globs <csv>             extra exclusions
  --out <path>                      save full prompt + response to file
  --dry-run                         print token estimate, don't call API
  --json                            machine-readable output
`);
  process.exit(0);
}

const opts = {
  module: null,
  dir: null,
  prompt: null,
  provider: "sonnet",
  model: null,
  maxFileKb: 200,
  includeMemory: false,
  includeGlobs: ["**/*.md", "**/*.js", "**/*.ts", "**/*.tsx", "**/*.json", "**/*.html", "**/*.css", "**/*.txt", "**/*.py"],
  excludeGlobs: ["node_modules/**", ".git/**", "dist/**", "build/**", "*.min.*", "*.lock", "package-lock.json"],
  out: null,
  dryRun: false,
  json: false,
};

for (let i = 0; i < args.length; i++) {
  const a = args[i];
  if (a === "--path") opts.dir = args[++i];
  else if (a === "--provider") opts.provider = args[++i];
  else if (a === "--model") opts.model = args[++i];
  else if (a === "--max-file-kb") opts.maxFileKb = parseInt(args[++i], 10);
  else if (a === "--include-memory") opts.includeMemory = true;
  else if (a === "--include-globs") opts.includeGlobs = args[++i].split(",");
  else if (a === "--exclude-globs") opts.excludeGlobs.push(...args[++i].split(","));
  else if (a === "--out") opts.out = args[++i];
  else if (a === "--dry-run") opts.dryRun = true;
  else if (a === "--json") opts.json = true;
  else if (!a.startsWith("--")) {
    if (!opts.module && !opts.dir) opts.module = a;
    else if (!opts.prompt) opts.prompt = a;
  }
}

if (!opts.prompt) {
  console.error("ERROR: prompt is required. Quote it as the last positional arg.");
  process.exit(1);
}

// ---------- resolve target directory ----------
const workspace = path.resolve(__dirname, "..");
let targetDir = opts.dir ? path.resolve(opts.dir) : null;
if (!targetDir && opts.module) {
  targetDir = path.join(workspace, "modules", opts.module);
}
if (!targetDir || !fs.existsSync(targetDir)) {
  console.error(`ERROR: directory not found: ${targetDir || "(none)"}`);
  console.error(`Hint: check modules/INDEX.md for module folder names.`);
  process.exit(1);
}

// ---------- glob walk ----------
function matchesAny(relPath, patterns) {
  return patterns.some((p) => globMatch(relPath, p));
}
function globMatch(str, pattern) {
  // Minimal glob: supports **, *, exact. `**/*.md` also matches top-level files.
  const norm = str.replace(/\\/g, "/");
  const patterns = [pattern];
  // Also try stripping a leading `**/` so top-level files match `**/*.md`.
  if (pattern.startsWith("**/")) patterns.push(pattern.slice(3));
  for (const p of patterns) {
    const re = new RegExp(
      "^" +
        p
          .replace(/[.+^${}()|[\]\\]/g, "\\$&")
          .replace(/\*\*/g, "::DOUBLESTAR::")
          .replace(/\*/g, "[^/]*")
          .replace(/::DOUBLESTAR::/g, ".*") +
        "$"
    );
    if (re.test(norm)) return true;
  }
  return false;
}

function walk(dir, collected = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    const rel = path.relative(targetDir, full).replace(/\\/g, "/");
    if (matchesAny(rel, opts.excludeGlobs)) continue;
    if (entry.isDirectory()) walk(full, collected);
    else if (entry.isFile()) {
      if (matchesAny(rel, opts.includeGlobs)) collected.push(full);
    }
  }
  return collected;
}

const files = walk(targetDir);

// ---------- build context bundle ----------
const chunks = [];
let totalBytes = 0;
let skippedLarge = 0;
const maxBytes = opts.maxFileKb * 1024;

for (const file of files) {
  const stat = fs.statSync(file);
  if (stat.size > maxBytes) {
    skippedLarge++;
    continue;
  }
  const rel = path.relative(workspace, file).replace(/\\/g, "/");
  const content = fs.readFileSync(file, "utf8");
  chunks.push(`\n\n<<<FILE: ${rel}>>>\n${content}\n<<<END FILE>>>\n`);
  totalBytes += content.length;
}

// Optional memory inclusion
if (opts.includeMemory) {
  const memoryFile = path.join(workspace, "MEMORY.md");
  if (fs.existsSync(memoryFile)) {
    const c = fs.readFileSync(memoryFile, "utf8");
    chunks.unshift(`\n\n<<<FILE: MEMORY.md>>>\n${c}\n<<<END FILE>>>\n`);
    totalBytes += c.length;
  }
  const memDir = path.join(workspace, "memory");
  if (fs.existsSync(memDir)) {
    const recent = fs
      .readdirSync(memDir)
      .filter((f) => /^\d{4}-\d{2}-\d{2}\.md$/.test(f))
      .sort()
      .slice(-7);
    for (const f of recent) {
      const c = fs.readFileSync(path.join(memDir, f), "utf8");
      chunks.push(`\n\n<<<FILE: memory/${f}>>>\n${c}\n<<<END FILE>>>\n`);
      totalBytes += c.length;
    }
  }
}

const bundle = chunks.join("");
const estTokens = Math.round(totalBytes / 4); // rough: 1 token ~ 4 chars

const systemPrompt = `You are performing a DEEP-DIVE analysis. The user has loaded an entire
project/module into your context instead of using retrieval. Your job: see
connections, contradictions, gaps, and opportunities that a normal
file-by-file read would miss. Be direct and specific. Cite file paths when
referencing things. Think strategically.`;

const userPrompt = `# Target: ${path.relative(workspace, targetDir).replace(/\\/g, "/")}
# Files loaded: ${files.length - skippedLarge} (${(totalBytes / 1024).toFixed(1)} KB, ~${estTokens.toLocaleString()} tokens)
# Files skipped (>${opts.maxFileKb}KB): ${skippedLarge}

## Question
${opts.prompt}

## Full context bundle
${bundle}`;

// ---------- dry run ----------
if (opts.dryRun) {
  const report = {
    targetDir: path.relative(workspace, targetDir),
    filesLoaded: files.length - skippedLarge,
    filesSkippedLarge: skippedLarge,
    totalBytes,
    estTokens,
    provider: opts.provider,
    prompt: opts.prompt,
  };
  if (opts.json) console.log(JSON.stringify(report, null, 2));
  else {
    console.log(`\n=== DRY RUN ===`);
    console.log(`Target:       ${report.targetDir}`);
    console.log(`Files loaded: ${report.filesLoaded}`);
    console.log(`Skipped >${opts.maxFileKb}KB: ${report.filesSkippedLarge}`);
    console.log(`Total size:   ${(totalBytes / 1024).toFixed(1)} KB`);
    console.log(`Est tokens:   ~${estTokens.toLocaleString()}`);
    console.log(`Provider:     ${opts.provider}`);
    console.log(`\nPrompt preview:\n${opts.prompt}\n`);
  }
  if (opts.out) {
    fs.mkdirSync(path.dirname(path.resolve(opts.out)), { recursive: true });
    fs.writeFileSync(opts.out, userPrompt);
    console.error(`Prompt written to: ${opts.out}`);
  }
  process.exit(0);
}

// ---------- call the model ----------
(async () => {
  let accessor;
  try {
    accessor = require("./secrets/accessor.js");
  } catch (e) {
    console.error(`ERROR: secrets accessor not available: ${e.message}`);
    console.error(`Run with --dry-run to preview without API call.`);
    process.exit(1);
  }

  let response;
  try {
    if (opts.provider === "gemini") {
      response = await callGemini(accessor, systemPrompt, userPrompt, opts);
    } else if (opts.provider === "deepseek") {
      response = await callDeepSeek(accessor, systemPrompt, userPrompt, opts);
    } else {
      response = await callAnthropic(accessor, systemPrompt, userPrompt, opts);
    }
  } catch (e) {
    console.error(`ERROR calling ${opts.provider}: ${e.message}`);
    process.exit(1);
  }

  if (opts.out) {
    fs.mkdirSync(path.dirname(path.resolve(opts.out)), { recursive: true });
    fs.writeFileSync(
      opts.out,
      `# Deep dive: ${path.relative(workspace, targetDir)}\n\n## Prompt\n${opts.prompt}\n\n## Response\n${response}\n`
    );
    console.error(`Response saved to: ${opts.out}`);
  }

  if (opts.json) {
    console.log(
      JSON.stringify(
        {
          targetDir: path.relative(workspace, targetDir),
          filesLoaded: files.length - skippedLarge,
          estTokens,
          provider: opts.provider,
          response,
        },
        null,
        2
      )
    );
  } else {
    console.log(response);
  }
})();

// ---------- provider callers ----------
async function callAnthropic(accessor, sys, user, opts) {
  let key = null;
  try { key = accessor.getByLabel("anthropic_api_key"); } catch (_) {}
  if (!key) key = process.env.ANTHROPIC_API_KEY || null;
  if (!key) throw new Error("anthropic_api_key not found in vault or ANTHROPIC_API_KEY env");
  const modelMap = {
    opus: "claude-opus-4-7",
    sonnet: "claude-sonnet-4-6",
  };
  const model = opts.model || modelMap[opts.provider] || "claude-sonnet-4-6";

  const res = await fetch("https://api.anthropic.com/v1/messages", {
    method: "POST",
    headers: {
      "x-api-key": key,
      "anthropic-version": "2023-06-01",
      "content-type": "application/json",
    },
    body: JSON.stringify({
      model,
      max_tokens: opts.maxOutputTokens || 16384,
      system: sys,
      messages: [{ role: "user", content: user }],
    }),
  });
  if (!res.ok) throw new Error(`${res.status}: ${await res.text()}`);
  const data = await res.json();
  return data.content?.[0]?.text || JSON.stringify(data);
}

async function callDeepSeek(accessor, sys, user, opts) {
  let key;
  try {
    key = accessor.get("deepseek.deepseek.api.key");
  } catch (_) {
    key = accessor.getByLabel("DEEPSEEK_API_KEY");
  }
  if (!key) key = process.env.DEEPSEEK_API_KEY || null;
  if (!key) throw new Error("DEEPSEEK_API_KEY not found in vault or env");
  // Default to V4-Pro for deep-dives: 1M context handles big module dumps comfortably,
  // pricing is ~7x cheaper than Sonnet for comparable reasoning quality.
  const model = opts.model || "deepseek-v4-pro";
  const res = await fetch("https://api.deepseek.com/v1/chat/completions", {
    method: "POST",
    headers: {
      "Authorization": `Bearer ${key}`,
      "content-type": "application/json",
    },
    body: JSON.stringify({
      model,
      messages: [
        { role: "system", content: sys },
        { role: "user", content: user },
      ],
      max_tokens: opts.maxOutputTokens || 8192,
    }),
  });
  if (!res.ok) throw new Error(`${res.status}: ${await res.text()}`);
  const data = await res.json();
  return data.choices?.[0]?.message?.content || JSON.stringify(data);
}

async function callGemini(accessor, sys, user, opts) {
  let key = null;
  try { key = accessor.getByLabel("gemini_api_key"); } catch (_) {}
  if (!key) key = process.env.GEMINI_API_KEY || process.env.GOOGLE_API_KEY || null;
  if (!key) throw new Error("gemini_api_key not found in vault or GEMINI_API_KEY/GOOGLE_API_KEY env");
  const model = opts.model || "gemini-2.5-pro";
  const url = `https://generativelanguage.googleapis.com/v1beta/models/${model}:generateContent?key=${key}`;

  const res = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      systemInstruction: { parts: [{ text: sys }] },
      contents: [{ role: "user", parts: [{ text: user }] }],
      generationConfig: { maxOutputTokens: 8192 },
    }),
  });
  if (!res.ok) throw new Error(`${res.status}: ${await res.text()}`);
  const data = await res.json();
  return (
    data.candidates?.[0]?.content?.parts?.map((p) => p.text).join("\n") ||
    JSON.stringify(data)
  );
}
