# ModpackPilot: A Modpack Server Launcher that simplifies PC-Server Hosting

A native desktop launcher and manager for Minecraft modpack servers. Import
existing server files or create one from scratch, then configure, launch,
monitor, and manage it.

Built as a side/hobby or what ever you call this project and shared as open source, Im tired boss. Just do what ever the hell you can improve this shitty app. This is also built using 50/50 AI Assisted coding (Mix of Human Code and AI Code), It can be shitty sometimes, it still needs human intervention in code review.

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
![License](https://img.shields.io/badge/license-MIT-green)
![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)

## What it does

- **Import** an existing Minecraft server from a ZIP file (drag-and-drop or
  file picker) or an existing folder. Detects the Minecraft version, mod
  loader (Forge / NeoForge / Fabric / Quilt / Vanilla), server JAR, mods,
  config, world folder, and start scripts on a best-effort basis before
  anything is copied.
- **Install an FTB modpack** directly: search Feed the Beast's public packs,
  pick a version, and ModpackPilot downloads the server files (verifying
  each one's checksum) and runs the Forge/NeoForge/Fabric server install
  itself - no `serverinstall_*.exe` needed.
- **Create** a new empty instance, or **duplicate** an existing one (server
  files and settings, not logs/backups) and configure it manually.
- **Launch, stop, restart, and force-stop** the server as a real child
  process managed by the Rust backend — never through the webview. Optional
  **auto-start** on app launch and **auto-restart** on crash, per instance.
- **Live console**: real-time stdout/stderr streaming, a command input for
  sending things like `say Hello` or `whitelist add PlayerName`, and
  persisted per-instance logs (`logs/latest.log`, `logs/<date>.log`).
- **Java management**: detects installed JDKs (`JAVA_HOME`, `PATH`, common
  install locations), shows version/vendor/architecture, lets you assign
  which one an instance uses, and recommends/flags mismatches against the
  Java version Mojang actually requires for that instance's Minecraft version.
- **Per-instance settings**: server JAR, JVM/server arguments, min/max RAM,
  auto-start, auto-restart.
- **World backups**: one-click zip snapshot of an instance's world folder,
  with restore and delete.
- **Mods management**: list, enable/disable (renames to `.jar.disabled`
  rather than deleting), and remove mods without touching the filesystem by hand.
- **Modpack updates**: link an instance to its Modrinth project or FTB
  modpack, then check for and one-click install newer versions - the world,
  whitelist/ops/bans, and `server.properties` are always left untouched. An
  FTB update also reinstalls the mod loader when the pack moves to a new
  build of it.
- **Whitelist / operators / banned players**: edit `whitelist.json`,
  `ops.json`, and `banned-players.json` from a form instead of hand-editing JSON.
- **Resource monitoring**: CPU%, memory, and uptime for each running
  server, sampled from the actual OS process, polled in one batched call
  regardless of how many servers are running.
- **Desktop notifications** when a server finishes starting or crashes, a
  **system tray** icon with minimize-to-tray, and optional **launch on
  system startup**.
- **App settings**: theme (Light/Dark/Dracula/Nord), configurable instances
  storage location, new-instance defaults (RAM/JVM args), console
  performance tuning, and one-click crash-log export/open-logs-folder.
- Everything is stored under an OS-appropriate app data directory —
  metadata in SQLite (WAL mode), server files on disk. No web server, no
  cloud dependency.

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
├── modrinth/              Modrinth API client + .mrpack update installer
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

ModpackPilot is under active incremental development, It's either I am going to have an Idea or not.

## Disclaimer

**NOT AN OFFICIAL MINECRAFT PRODUCT. NOT APPROVED BY OR ASSOCIATED WITH MOJANG OR
MICROSOFT.**

ModpackPilot is an independent, unaffiliated tool. It is not endorsed by, sponsored by,
or connected to Mojang, Microsoft, Feed the Beast Limited, or Rinth, Inc. (Modrinth).
Those names are used here only descriptively, to say which services ModpackPilot
interoperates with.

ModpackPilot **does not distribute Minecraft, mods, or modpacks.** It contains no
third-party game content and hosts, mirrors, and proxies nothing. When you ask it to
install a pack, the copy of ModpackPilot on your own machine downloads the files directly
from the official servers of the service that publishes them, using the URLs that service
itself publishes — exactly as your browser would if you clicked the download link.

Everything ModpackPilot installs stays governed by the licence and terms of whoever
published it, and **you are responsible for complying with them**:

- [Feed the Beast Modpack/Mods Policy](https://www.feed-the-beast.com/policies/modpacks-mods-policy)
  — note that FTB's download licence is non-transferable, grants no right to sublicense,
  and is limited to personal use
- [Modrinth Terms of Use](https://modrinth.com/legal/terms), plus each project's own licence
- [Minecraft EULA](https://aka.ms/MinecraftEULA)
- The individual licence of every mod in a pack, which may be more restrictive than the
  pack's own terms

See [THIRD_PARTY.md](THIRD_PARTY.md) for the full statement, and [NOTICE](NOTICE) for the
condensed one.

## License

[MIT](LICENSE) — do what you like with it, just keep the copyright notice.

The MIT licence covers **ModpackPilot's own source code only**. It does not extend to any
Minecraft content, mod, or modpack that ModpackPilot downloads for you.
