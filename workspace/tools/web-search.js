// Shipped with WormHole v0.1.0. See workspace/tools/TOOLS.md for usage.
//
#!/usr/bin/env node
/**
 * web-search.js - Multi-source web search tool
 *
 * Tries multiple free search methods in order:
 * 1. Brave Search API (if BRAVE_SEARCH_API_KEY is set)
 * 2. Google Custom Search (if GOOGLE_CSE_KEY + GOOGLE_CSE_ID set)
 * 3. DuckDuckGo HTML scrape (no API key needed, rate-limited)
 * 4. SearXNG local instance (if running)
 *
 * Usage:
 *   node web-search.js "query"
 *   node web-search.js --count 5 "query"
 *   node web-search.js --provider brave "query"
 *   node web-search.js --json "query"
 */

const https = require('https');
const http = require('http');

const args = process.argv.slice(2);
let count = 5;
let forceProvider = null;
let jsonOutput = false;
const queryParts = [];

for (let i = 0; i < args.length; i++) {
  if (args[i] === '--count' && args[i + 1]) { count = parseInt(args[++i]); }
  else if (args[i] === '--provider' && args[i + 1]) { forceProvider = args[++i]; }
  else if (args[i] === '--json') { jsonOutput = true; }
  else { queryParts.push(args[i]); }
}

const query = queryParts.join(' ');
if (!query) {
  console.error('Usage: node web-search.js [--count N] [--provider brave|ddg|searxng] "query"');
  process.exit(1);
}

// --- Brave Search API ---
async function searchBrave(q, n) {
  const apiKey = process.env.BRAVE_SEARCH_API_KEY;
  if (!apiKey) throw new Error('No BRAVE_SEARCH_API_KEY');

  const params = new URLSearchParams({ q, count: n });
  return new Promise((resolve, reject) => {
    const req = https.get(
      `https://api.search.brave.com/res/v1/web/search?${params}`,
      { headers: { 'X-Subscription-Token': apiKey, 'Accept': 'application/json' }, timeout: 10000 },
      (res) => {
        let data = '';
        res.on('data', c => data += c);
        res.on('end', () => {
          try {
            const j = JSON.parse(data);
            if (j.web && j.web.results) {
              resolve(j.web.results.slice(0, n).map(r => ({
                title: r.title, url: r.url, snippet: r.description
              })));
            } else { reject(new Error('Unexpected Brave response')); }
          } catch (e) { reject(e); }
        });
      }
    );
    req.on('error', reject);
    req.on('timeout', () => { req.destroy(); reject(new Error('Brave timeout')); });
  });
}

// --- DuckDuckGo HTML scrape ---
async function searchDDG(q, n) {
  const params = new URLSearchParams({ q, t: 'h_', ia: 'web' });
  return new Promise((resolve, reject) => {
    const req = https.get(
      `https://html.duckduckgo.com/html/?${params}`,
      { headers: { 'User-Agent': 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36' }, timeout: 10000 },
      (res) => {
        let data = '';
        res.on('data', c => data += c);
        res.on('end', () => {
          const results = [];
          // Parse result blocks from HTML
          const blocks = data.split(/class="result__body"/g).slice(1, n + 1);
          for (const block of blocks) {
            const titleMatch = block.match(/class="result__a"[^>]*>([^<]+)</);
            const urlMatch = block.match(/class="result__url"[^>]*href="([^"]+)"/);
            const snippetMatch = block.match(/class="result__snippet"[^>]*>([^<]+)/);
            const hrefMatch = block.match(/class="result__a"[^>]*href="([^"]+)"/);
            if (titleMatch) {
              let url = '';
              if (hrefMatch) {
                // DDG wraps URLs in redirect
                const decoded = decodeURIComponent(hrefMatch[1]);
                const uddg = decoded.match(/uddg=([^&]+)/);
                url = uddg ? decodeURIComponent(uddg[1]) : decoded;
              }
              results.push({
                title: titleMatch[1].trim(),
                url: url,
                snippet: snippetMatch ? snippetMatch[1].trim() : ''
              });
            }
          }
          if (results.length === 0 && data.includes('bot')) {
            reject(new Error('DDG bot detection - rate limited'));
          } else {
            resolve(results);
          }
        });
      }
    );
    req.on('error', reject);
    req.on('timeout', () => { req.destroy(); reject(new Error('DDG timeout')); });
  });
}

// --- SearXNG local instance ---
async function searchSearXNG(q, n) {
  const host = process.env.SEARXNG_HOST || 'localhost';
  const port = process.env.SEARXNG_PORT || 8888;
  const params = new URLSearchParams({ q, format: 'json', categories: 'general' });
  return new Promise((resolve, reject) => {
    const req = http.get(
      `http://${host}:${port}/search?${params}`,
      { timeout: 8000 },
      (res) => {
        let data = '';
        res.on('data', c => data += c);
        res.on('end', () => {
          try {
            const j = JSON.parse(data);
            resolve((j.results || []).slice(0, n).map(r => ({
              title: r.title, url: r.url, snippet: r.content || ''
            })));
          } catch (e) { reject(e); }
        });
      }
    );
    req.on('error', reject);
    req.on('timeout', () => { req.destroy(); reject(new Error('SearXNG timeout')); });
  });
}

// --- Main ---
async function main() {
  const providers = forceProvider ? [forceProvider] : ['brave', 'ddg', 'searxng'];
  let results = null;
  let usedProvider = null;

  for (const p of providers) {
    try {
      switch (p) {
        case 'brave': results = await searchBrave(query, count); break;
        case 'ddg': results = await searchDDG(query, count); break;
        case 'searxng': results = await searchSearXNG(query, count); break;
      }
      if (results && results.length > 0) {
        usedProvider = p;
        break;
      }
    } catch (e) {
      process.stderr.write(`[${p}] ${e.message}\n`);
    }
  }

  if (!results || results.length === 0) {
    console.error('All search providers failed.');
    process.exit(1);
  }

  if (jsonOutput) {
    console.log(JSON.stringify({ provider: usedProvider, results }, null, 2));
  } else {
    console.log(`[${usedProvider}] ${results.length} results for: ${query}\n`);
    results.forEach((r, i) => {
      console.log(`${i + 1}. ${r.title}`);
      console.log(`   ${r.url}`);
      if (r.snippet) console.log(`   ${r.snippet}`);
      console.log();
    });
  }
}

main();
