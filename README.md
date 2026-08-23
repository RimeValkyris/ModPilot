# ModForge

A native desktop launcher and manager for Minecraft modpack servers. Import
existing server files or create one from scratch, then configure, launch,
monitor, and manage it — no browser tab, no hosted backend, no Docker.

<!-- Tech stack -->
![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-000000?logo=rust&logoColor=white)
![React](https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=black)
![TypeScript](https://img.shields.io/badge/TypeScript-5-3178C6?logo=typescript&logoColor=white)
![Vite](https://img.shields.io/badge/Vite-7-646CFF?logo=vite&logoColor=white)
![Tailwind CSS](https://img.shields.io/badge/Tailwind_CSS-4-06B6D4?logo=tailwindcss&logoColor=white)
![shadcn/ui](https://img.shields.io/badge/shadcn%2Fui-000000?logo=shadcnui&logoColor=white)
![Zustand](https://img.shields.io/badge/Zustand-state-443E38?logo=react&logoColor=white)
![SQLite](https://img.shields.io/badge/SQLite-003B57?logo=sqlite&logoColor=white)
![SQLx](https://img.shields.io/badge/SQLx-async-blue)
![Tokio](https://img.shields.io/badge/Tokio-async_runtime-orange)
![Platform](https://img.shields.io/badge/platform-Windows-0078D6?logo=windows&logoColor=white)
![License](https://img.shields.io/badge/license-Unlicensed-lightgrey)

## What it does

- **Import** an existing Minecraft server from a ZIP file (drag-and-drop or
  file picker) or an existing folder. ModForge detects the Minecraft
  version, mod loader (Forge / NeoForge / Fabric / Quilt / Vanilla), server
  JAR, mods, config, world folder, and start scripts on a best-effort basis
  before anything is copied.
- **Create** a new empty instance and configure it manually.
- **Launch, stop, restart, and force-stop** the server as a real child
  process managed by the Rust backend — never through the webview.
- **Live console**: real-time stdout/stderr streaming, a command input for
  sending things like `say Hello` or `whitelist add PlayerName`, and
  persisted per-instance logs (`logs/latest.log`, `logs/<date>.log`).
- **Java management**: detects installed JDKs (`JAVA_HOME`, `PATH`, common
  install locations), shows version/vendor/architecture, and lets you
  assign which one an instance launches with.
- **Per-instance settings**: server JAR, JVM/server arguments, min/max RAM,
  auto-start, auto-restart, and an optional wallpaper image for the
  instance's card and detail page.
- **Resource monitoring**: CPU%, memory, and uptime for each running
  server, sampled from the actual OS process.
- Everything is stored under an OS-appropriate app data directory —
  metadata in SQLite, server files on disk. No web server, no cloud
  dependency.

## Tech stack

| Layer | Technology |
|---|---|
| Desktop shell | [Tauri 2](https://tauri.app) |
| Backend | [Rust](https://www.rust-lang.org), [Tokio](https://tokio.rs) |
| Database | [SQLite](https://sqlite.org) via [SQLx](https://github.com/launchbadge/sqlx) |
| Frontend | [React 19](https://react.dev), [TypeScript](https://www.typescriptlang.org), [Vite](https://vitejs.dev) |
| Styling / UI | [Tailwind CSS 4](https://tailwindcss.com), [shadcn/ui](https://ui.shadcn.com) |
| State | [Zustand](https://github.com/pmndrs/zustand) |

Rust owns everything filesystem-, process-, and database-related: archive
extraction, server detection, Java discovery, process lifecycle, and SQLite
access. React owns presentation, navigation, and forms only — it talks to
Rust exclusively through Tauri commands (request/response) and events
(real-time console output, status changes).

## Getting started

### Prerequisites (development only — end users never need these)

- [Node.js](https://nodejs.org) 18+
- [Rust](https://www.rust-lang.org/tools/install) (via `rustup`)
- Windows: the "Desktop development with C++" workload from the
  [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)

### Run in development

```bash
npm install
npm run tauri dev
```

This opens the actual desktop window (not a browser) with hot reload on
both the React and Rust sides.

### Build a distributable installer

```bash
npm run tauri build
```

Produces a Windows `.msi` and an NSIS `.exe` installer under
`src-tauri/target/release/bundle/`.

## Project structure

```
src/                      React frontend
├── components/           Shared UI (layout, shadcn primitives)
├── features/              One folder per feature area (dashboard, console,
│                          java, servers, settings)
├── stores/                Zustand stores
├── hooks/                 Shared React hooks
├── lib/                   Tauri command wrapper, formatting, utilities
└── types/                 TypeScript mirrors of the Rust models

src-tauri/src/             Rust backend
├── commands/              Tauri commands (one module per feature area)
├── server/                Process lifecycle, log capture, resource monitor
├── importer/              Safe ZIP/folder import + server detection
├── java/                  Java installation detection
├── filesystem/            App-data paths, name sanitization
├── database/              SQLite pool + migration runner
├── models/                Shared data types
└── logging/               Application-level log file

src-tauri/migrations/      SQL schema migrations
```

## Security notes

- ZIP extraction validates every entry path against directory-traversal
  ("zip slip") before writing anything to disk.
- Imported `.bat`/`.sh` files are never executed automatically.
- The Minecraft server process is spawned with an argument vector, never a
  shell string — no shell injection surface.
- A restrictive Content-Security-Policy is set on the webview.

## Status

ModForge is under active incremental development. Core instance
management, importing, Java detection, process control, the live console,
per-instance settings, and resource monitoring are implemented. Auto
Java installation and auto-updates are planned but not yet built.
