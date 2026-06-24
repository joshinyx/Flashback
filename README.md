<p align="center">
  <img src="static/flashback-header.png" alt="Flashback" width="1200">
</p>

## Overview

Flashback is a lightweight game clip capture and editor for Windows, built with Tauri v2 and Rust. It stays extremely light and fast and does one thing well — capture and edit clips, nothing else. No social feed, no achievements, no accounts, no forced cloud.

The goal is an app you can open, start, and forget is even running.

---

## Features

- **Instant Replay** › Save the last seconds or minutes with a global hotkey
- **Manual recording** › Start and stop on demand
- **Configurable quality** › Adjustable resolution, FPS and quality
- **Hardware-accelerated encoding** › Uses the best available encoder, with software fallback
- **Simple editor** › Trim, cut, and export in a few clicks
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

## Download

Grab the latest installer from the [Releases](https://github.com/joshinyx/Flashback/releases) page.

---

## License

GPL-3.0

## About

Built by [Josh Bernal](https://joshiny.dev).
