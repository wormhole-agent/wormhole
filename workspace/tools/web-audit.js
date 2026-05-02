// Shipped with WormHole v0.1.0. See workspace/tools/TOOLS.md for usage.
//
#!/usr/bin/env node
/**
 * web-audit.js - Audit a static site or repo for SEO + ADA + AI-friendly compliance.
 *
 * Usage:
 *   node tools/web-audit.js <site-dir>           # audit one site
 *   node tools/web-audit.js --all                # audit every known site
 *   node tools/web-audit.js <site-dir> --json    # machine-readable output
 *   node tools/web-audit.js <site-dir> --fix-meta # add missing robots.txt/sitemap.xml/llms.txt scaffolds (interactive-safe)
 *
 * Standard: docs/web-standards.md
 *
 * The audit is heuristic, not a full WCAG conformance test. It catches the easy
 * misses: missing schema, no llms.txt, no alt text, no h1, no canonical, etc.
 * Use Lighthouse / axe / schema.org validator for the deep checks.
 */

const fs = require('fs');
const path = require('path');

const KNOWN_SITES = [
  // Add paths to your sites here. Each entry is a relative directory path.
  // Example: 'my-site',
];

// Dirs to skip from per-file audit (auto-generated content, drafts, archives, staging).
const SKIP_DIRS = new Set([
  '.git', 'node_modules', '_archive', 'tmp', 'logs',
  // Some auto-generated submission pages live under predictable directory names
  // — fixing them is a template change, not per-file. Audit the top-level shell only.
  'discussions', 'reviews', 'releases', 'data',
  // Build/staging/scratch dirs
  'v2', 'staging', 'scratch', 'logo-concepts', 'Media', 'media',  // Source/design files (not deployed)
  'design-source',
]);

const SEVERITY = {
  CRITICAL: 'critical',
  WARNING: 'warning',
  INFO: 'info',
};

function findHtmlFiles(dir) {
  const out = [];
  function walk(d) {
    let entries;
    try { entries = fs.readdirSync(d, { withFileTypes: true }); } catch { return; }
    for (const e of entries) {
      if (SKIP_DIRS.has(e.name) || e.name.startsWith('.')) continue;
      const full = path.join(d, e.name);
      if (e.isDirectory()) walk(full);
      else if (e.isFile() && e.name.endsWith('.html')) out.push(full);
    }
  }
  walk(dir);
  return out;
}

function auditHtmlFile(filePath, html, siteRoot) {
  const findings = [];
  const rel = siteRoot ? path.relative(siteRoot, filePath).replace(/\\/g, '/') : path.basename(filePath);
  const isHome = /index\.html?$/i.test(filePath);

  function add(severity, code, message) {
    findings.push({ file: rel, severity, code, message });
  }

  // --- HEAD essentials ---
  if (!/<html[^>]*\blang=/i.test(html)) add(SEVERITY.CRITICAL, 'no-lang', '<html> missing lang attribute');
  if (!/<meta[^>]*\bcharset=/i.test(html)) add(SEVERITY.CRITICAL, 'no-charset', '<meta charset> missing');
  if (!/<meta[^>]*\bname=["']viewport["']/i.test(html)) add(SEVERITY.CRITICAL, 'no-viewport', 'viewport meta missing (mobile)');
  if (!/<meta[^>]*\bname=["']description["']/i.test(html)) add(SEVERITY.CRITICAL, 'no-description', 'meta description missing (SEO)');
  if (!/<title>[^<]{5,}<\/title>/i.test(html)) add(SEVERITY.CRITICAL, 'no-title', '<title> missing or too short');
  if (!/<link[^>]*\brel=["']canonical["']/i.test(html)) add(SEVERITY.WARNING, 'no-canonical', 'canonical link missing');

  // Open Graph / Twitter
  if (!/<meta[^>]*\bproperty=["']og:title["']/i.test(html)) add(SEVERITY.WARNING, 'no-og', 'Open Graph tags missing (helps social + agent previews)');
  if (!/<meta[^>]*\bname=["']twitter:card["']/i.test(html)) add(SEVERITY.INFO, 'no-twitter-card', 'Twitter card meta missing');

  // --- Structured data (THE big AI signal) ---
  if (!/application\/ld\+json/i.test(html)) add(SEVERITY.CRITICAL, 'no-jsonld', 'No JSON-LD schema block (critical for AI agents + rich snippets)');

  // --- Semantic structure ---
  const h1Matches = (html.match(/<h1[\s>]/gi) || []).length;
  if (h1Matches === 0) add(SEVERITY.CRITICAL, 'no-h1', '<h1> missing');
  if (h1Matches > 1) add(SEVERITY.WARNING, 'multi-h1', `${h1Matches} <h1> tags found; should be exactly one`);

  if (!/<main[\s>]/i.test(html)) add(SEVERITY.WARNING, 'no-main', '<main> landmark missing');
  if (!/<header[\s>]/i.test(html)) add(SEVERITY.INFO, 'no-header', '<header> landmark missing');
  if (!/<footer[\s>]/i.test(html)) add(SEVERITY.INFO, 'no-footer', '<footer> landmark missing');

  // --- Accessibility ---
  // <img> without alt
  const imgs = html.match(/<img\b[^>]*>/gi) || [];
  const imgsNoAlt = imgs.filter(i => !/\balt=/i.test(i));
  if (imgsNoAlt.length) add(SEVERITY.CRITICAL, 'img-no-alt', `${imgsNoAlt.length} <img> tags missing alt attribute`);

  // <a> with no accessible text (icon-only without aria-label)
  const aTags = html.match(/<a\b[^>]*>([\s\S]*?)<\/a>/gi) || [];
  const emptyA = aTags.filter(a => {
    const inner = a.replace(/<a\b[^>]*>/i, '').replace(/<\/a>/i, '').replace(/<[^>]+>/g, '').trim();
    const hasAria = /\baria-label=/i.test(a) || /\btitle=/i.test(a);
    return !inner && !hasAria;
  });
  if (emptyA.length) add(SEVERITY.WARNING, 'empty-link', `${emptyA.length} <a> tags with no text and no aria-label`);

  // <button> with no accessible text
  const btnTags = html.match(/<button\b[^>]*>([\s\S]*?)<\/button>/gi) || [];
  const emptyBtn = btnTags.filter(b => {
    const inner = b.replace(/<button\b[^>]*>/i, '').replace(/<\/button>/i, '').replace(/<[^>]+>/g, '').trim();
    return !inner && !/\baria-label=/i.test(b);
  });
  if (emptyBtn.length) add(SEVERITY.WARNING, 'empty-button', `${emptyBtn.length} <button> with no text and no aria-label`);

  // form inputs without labels (rough check)
  const inputs = html.match(/<input\b[^>]*>/gi) || [];
  const labelable = inputs.filter(i => !/type=["'](hidden|submit|button|reset)["']/i.test(i));
  const inputsNoLabel = labelable.filter(i => {
    const idMatch = i.match(/\bid=["']([^"']+)["']/i);
    if (!idMatch) return !/\baria-label=/i.test(i);
    const id = idMatch[1];
    const hasLabel = new RegExp(`<label[^>]*\\bfor=["']${id}["']`, 'i').test(html);
    return !hasLabel && !/\baria-label=/i.test(i);
  });
  if (inputsNoLabel.length) add(SEVERITY.WARNING, 'input-no-label', `${inputsNoLabel.length} form inputs without <label> or aria-label`);

  // --- Em-dash check (house style rule) ---
  // strip script/style first
  const visible = html.replace(/<script[\s\S]*?<\/script>/gi, '').replace(/<style[\s\S]*?<\/style>/gi, '');
  if (/—/.test(visible)) add(SEVERITY.WARNING, 'em-dash', 'em-dash present in visible copy (house style: use regular dashes)');

  return findings;
}

function auditSite(siteDir) {
  const result = {
    site: siteDir,
    siteRoot: null,
    files: 0,
    findings: [],
    siteLevel: [],
  };

  if (!fs.existsSync(siteDir)) {
    result.siteLevel.push({ severity: SEVERITY.CRITICAL, code: 'no-dir', message: `Site directory not found: ${siteDir}` });
    return result;
  }

  // Find a "site root" - the dir that contains index.html
  const candidates = [siteDir, path.join(siteDir, 'public'), path.join(siteDir, 'dist'), path.join(siteDir, 'build'), path.join(siteDir, 'docs'), path.join(siteDir, 'site'), path.join(siteDir, 'web'), path.join(siteDir, 'www')];
  const root = candidates.find(d => fs.existsSync(path.join(d, 'index.html'))) || siteDir;
  result.siteRoot = root;

  // If there's no index.html anywhere, this is a code/docs repo, not a deployed site.
  // Skip site-level meta-file checks entirely.
  const hasIndexHtml = fs.existsSync(path.join(root, 'index.html'));
  if (!hasIndexHtml) {
    result.siteLevel.push({ severity: SEVERITY.INFO, code: 'not-a-site', message: 'No index.html found - treating as code/docs repo, skipping site-level meta-file checks' });
    result.isRepoOnly = true;
    return result;
  }

  // Site-level checks (deployed-site only)
  if (!fs.existsSync(path.join(root, 'robots.txt'))) result.siteLevel.push({ severity: SEVERITY.CRITICAL, code: 'no-robots', message: 'robots.txt missing at site root' });
  if (!fs.existsSync(path.join(root, 'sitemap.xml'))) result.siteLevel.push({ severity: SEVERITY.CRITICAL, code: 'no-sitemap', message: 'sitemap.xml missing at site root' });
  if (!fs.existsSync(path.join(root, 'llms.txt'))) result.siteLevel.push({ severity: SEVERITY.CRITICAL, code: 'no-llms', message: 'llms.txt missing at site root (AI agent overview)' });

  // Per-file checks
  const htmls = findHtmlFiles(root);
  result.files = htmls.length;
  for (const f of htmls) {
    let html;
    try { html = fs.readFileSync(f, 'utf8'); } catch { continue; }
    result.findings.push(...auditHtmlFile(f, html, root));
  }

  return result;
}

function summarize(result) {
  const counts = { critical: 0, warning: 0, info: 0 };
  for (const f of [...result.siteLevel, ...result.findings]) counts[f.severity]++;
  return counts;
}

function renderReport(result) {
  const lines = [];
  lines.push(`\n=== ${result.site} ===`);
  if (result.siteRoot && result.siteRoot !== result.site) lines.push(`(site root: ${path.relative(process.cwd(), result.siteRoot)})`);
  lines.push(`HTML files scanned: ${result.files}`);
  const counts = summarize(result);
  lines.push(`Findings: ${counts.critical} critical, ${counts.warning} warning, ${counts.info} info`);

  if (result.siteLevel.length) {
    lines.push('\nSite-level:');
    for (const f of result.siteLevel) lines.push(`  [${f.severity.toUpperCase()}] ${f.code}: ${f.message}`);
  }

  if (result.findings.length) {
    // Group by file
    const byFile = {};
    for (const f of result.findings) {
      byFile[f.file] = byFile[f.file] || [];
      byFile[f.file].push(f);
    }
    lines.push('\nPer-file:');
    for (const [file, items] of Object.entries(byFile)) {
      lines.push(`  ${file}:`);
      for (const f of items) lines.push(`    [${f.severity.toUpperCase()}] ${f.code}: ${f.message}`);
    }
  }

  if (counts.critical === 0 && counts.warning === 0) lines.push('\n✓ Clean (no critical, no warnings).');
  else if (counts.critical === 0) lines.push('\n△ No criticals; address warnings before next deploy.');
  else lines.push('\n✗ FAILS web-standards rule. Fix criticals before deploy.');

  return lines.join('\n');
}

function main() {
  const args = process.argv.slice(2);
  const json = args.includes('--json');
  const all = args.includes('--all');
  const targets = all ? KNOWN_SITES : args.filter(a => !a.startsWith('--'));

  if (!targets.length) {
    console.error('Usage: node tools/web-audit.js <site-dir> | --all  [--json]');
    console.error('Known sites: ' + KNOWN_SITES.join(', '));
    process.exit(2);
  }

  const results = targets.map(t => auditSite(path.resolve(t)));

  if (json) {
    console.log(JSON.stringify(results, null, 2));
  } else {
    for (const r of results) console.log(renderReport(r));
    const totals = results.reduce((acc, r) => {
      const c = summarize(r);
      acc.critical += c.critical; acc.warning += c.warning; acc.info += c.info;
      return acc;
    }, { critical: 0, warning: 0, info: 0 });
    console.log(`\n=== TOTAL ===`);
    console.log(`${totals.critical} critical, ${totals.warning} warning, ${totals.info} info across ${results.length} site(s)`);
  }

  // Exit code: 1 if any criticals, else 0
  const anyCrit = results.some(r => summarize(r).critical > 0);
  process.exit(anyCrit ? 1 : 0);
}

if (require.main === module) main();

module.exports = { auditSite, auditHtmlFile, KNOWN_SITES };
