// example-module/tool-stub.js
//
// A tool is a short Node script the agent invokes via the `shell` tool.
// Tools live either at workspace/tools/ (shared) or modules/<name>/tools/
// (module-scoped). This file is the module-scoped shape.
//
// Convention: every tool starts with a module-level docstring like this one
// explaining what the tool does and how to invoke it. The readability standard
// in CONTRIBUTING.md treats the docstring as required.

'use strict';

function main() {
  const args = process.argv.slice(2);
  if (args.length === 0) {
    console.error('Usage: node tool-stub.js <input>');
    process.exit(2);
  }
  const input = args[0];
  console.log(JSON.stringify({ tool: 'tool-stub', input, ok: true }));
}

main();
