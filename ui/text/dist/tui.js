#!/usr/bin/env node
'use strict';

const { spawn } = require('node:child_process');

const HELP = `BharatCode TUI launcher.
Usage:
  bharatcode tui
  bharatcode tui --help
  bharatcode tui -- --help
  bharatcode tui -- --version

This script starts the locally installed BharatCode CLI binary in interactive
mode. ` +
  `Pass arguments after -- to forward them to the CLI binary directly.`;

function printUsage() {
  console.log(HELP);
}

function showError(message) {
  console.error(`[bharatcode-tui] ${message}`);
}

function main() {
  const args = process.argv.slice(2);
  const hasForwarding = args.length > 0 && args[0] === '--';

  if (!hasForwarding && args.some((arg) => arg === '-h' || arg === '--help')) {
    printUsage();
    return 0;
  }

  const binaryPath = process.env.BHARATCODE_BINARY;
  if (!binaryPath) {
    showError(
      'BHARATCODE_BINARY is not set. This launcher should be executed via `bharatcode tui`.',
    );
    return 1;
  }

  const delegateArgs = hasForwarding ? args.slice(1) : [];

  const child = spawn(binaryPath, delegateArgs, {
    env: {
      ...process.env,
      BHARATCODE_TUI_ACTIVE: '1',
    },
    stdio: 'inherit',
  });

  child.on('error', (error) => {
    showError(`failed to spawn ${binaryPath}: ${error.message}`);
    process.exitCode = 1;
  });

  child.on('exit', (code, signal) => {
    if (signal) {
      process.kill(process.pid, signal);
    }
    process.exitCode = typeof code === 'number' ? code : 1;
  });

  return 0;
}

process.exitCode = main();
