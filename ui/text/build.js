#!/usr/bin/env node
'use strict';

const { cpSync, mkdirSync } = require('node:fs');
const { resolve, join } = require('node:path');

const root = resolve(__dirname);
const src = join(root, 'src', 'tui.js');
const dist = join(root, 'dist');
const out = join(dist, 'tui.js');

mkdirSync(dist, { recursive: true });
cpSync(src, out);
