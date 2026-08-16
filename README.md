# TongDock

TongDock is an extensible desktop productivity dock built with Tauri 2, Vue 3, and TypeScript. It is designed as a modular workspace where tools can evolve over time without changing the core shell.

## Current Modules

- ClipTrail: clipboard capture, search, and recall.
- MarkLens: markdown reading and live preview.
- TextForge: text transformation and formatting utilities.

## Platform Features

- Unified module shell with consistent navigation and theming.
- Keyboard-first interactions and local-first data handling.
- Tray integration and global shortcut support.
- Modular architecture ready for future tools.

## Development

### Prerequisites

- Node.js and npm
- Rust toolchain
- Tauri platform prerequisites: https://v2.tauri.app/start/prerequisites/

### Run Locally

```bash
npm install
npm run tauri dev
```

### Build

Frontend build only:

```bash
npm run build
```

Desktop bundle:

```bash
npm run tauri build
```

## Stack

- Tauri 2
- Vue 3
- TypeScript
- Vite