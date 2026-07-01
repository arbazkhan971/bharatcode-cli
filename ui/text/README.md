# BharatCode local terminal UI

This folder contains a local-first terminal UI artifact for `bharatcode tui`.

The Rust CLI will resolve UI in this order:

1. `BHARATCODE_TUI_SCRIPT` (absolute path)
2. `<repo>/ui/text/dist/tui.js`
3. `npx --package <spec> -- bharatcode-tui`

## Build the local artifact

```bash
cd ui/text
npm install
node build.js
```

`node build.js` copies `src/tui.js` to `dist/tui.js`.

## Run local UI script

- `bharatcode tui` from repo root will run the local artifact automatically.
- `bharatcode tui -- --help` forwards args to the spawned `bharatcode` binary.
