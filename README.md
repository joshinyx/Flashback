<p align="center">
  <img src="static/flashback-header.png" alt="Flashback" width="1200">
</p>

## Overview

Flashback is a lightweight game clip capture and editor for Windows, built with Tauri v2 and Rust. Inspired by Medal and SteelSeries Moments, but with the opposite philosophy: stay extremely light and fast and do one thing well — capture and edit clips, nothing else. No social feed, no achievements, no accounts, no forced cloud.

The goal is an app you can open, start, and forget is even running.

> **Status:** early development. The feature list below is the target scope, not a finished product.

---

## Features

- **Instant Replay** › Save the last X seconds or minutes with a global hotkey
- **Manual recording** › Start/stop on demand
- **Configurable quality** › Free resolution, FPS and quality — from ultra-light 480p/20fps to high-end when the hardware allows
- **Hardware-accelerated encoding** › Auto-selects the best available encoder (NVENC / AMF / Quick Sync), with software fallback
- **Simple editor** › Trim start and end, remove middle segments, auto-join the rest, export in a few clicks
- **Local library** › Clips stored locally, no cloud dependency

---

## Stack

| Layer     | Technology                                      |
| --------- | ----------------------------------------------- |
| Shell     | Tauri v2 (Rust)                                 |
| Frontend  | SvelteKit + Svelte 5 + TypeScript               |
| Capture   | Windows Graphics Capture (WGC)                  |
| Encoding  | Hardware (NVENC / AMF / Quick Sync) + fallback  |
| Platform  | Windows 10 / 11 (WGC-capable)                   |

---

## Architecture

```
src/                   SvelteKit frontend (config, library, editor)
└── routes/            UI views
static/                Static assets
src-tauri/
├── src/lib.rs         Tauri commands + app logic
├── src/main.rs        Entry point
├── icons/             App icons
└── tauri.conf.json    Window + bundle config
```

The Rust backend does the heavy lifting — capture, the instant-replay buffer, hardware encoding, the local library and editing. The SvelteKit frontend is only for configuration and editing; it sends intents to the backend over Tauri's `invoke` and reflects state.

---

## Development

### Prerequisites

- [Node.js](https://nodejs.org/) + [pnpm](https://pnpm.io/)
- [Rust](https://rustup.rs/) + Cargo
- [Tauri CLI prerequisites for Windows](https://v2.tauri.app/start/prerequisites/)

```bash
git clone https://github.com/joshinyx/Flashback.git
cd Flashback
pnpm install

pnpm tauri dev    # dev mode with hot reload
pnpm tauri build  # production build + installer
```

---

## License

MIT

## About

Built by [Josh Bernal](https://joshiny.dev) at [Euxora](https://euxora.net).
