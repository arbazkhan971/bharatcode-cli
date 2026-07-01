const assert = require('node:assert/strict');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const script = path.join(__dirname, 'dist', 'tui.js');
const result = spawnSync(process.execPath, [script, '--help'], {
  encoding: 'utf8',
  env: {
    ...process.env,
    BHARATCODE_BINARY: '/usr/bin/env-not-set',
  },
});

if (result.error) {
  throw result.error;
}

assert.strictEqual(result.status, 0, 'tui.js --help should exit 0');
assert.ok(result.stdout.includes('BharatCode TUI launcher'), result.stdout);
