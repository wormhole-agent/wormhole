// Shipped with WormHole v0.1.0. See workspace/tools/TOOLS.md for usage.
//
// Quick listing of available secrets (masked only)
const { list } = require('./secrets/accessor');
const items = list();
for (const it of items) {
  console.log(`${it.section} | ${it.label} | id=${it.id} | ${it.masked}`);
}
console.log(`\n(${items.length} secrets total)`);
