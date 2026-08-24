# Contributing to ModpackPilot

First off, thanks for taking the time to contribute — whether that's a bug
report, a feature idea, a docs fix, or actual code, it's genuinely
appreciated. This project is built in the open and welcomes all of it. 🙌

No contribution is too small. Fixing a typo, reporting a confusing error
message, or asking a question that highlights a gap in the docs all help.

## Ways to contribute

- **Report a bug** — open an issue with steps to reproduce, what you
  expected, and what actually happened. Logs help a lot (Settings → Danger
  Zone → Export logs, or `%APPDATA%/com.modpackpilot.app/logs`).
- **Suggest a feature** — open an issue describing the problem you're
  trying to solve, not just the solution — it's easier to find a good
  implementation when the underlying need is clear.
- **Fix something** — pick up an open issue (or file one first for
  anything non-trivial, so we can talk through the approach before you
  spend time on it) and open a PR.
- **Improve the docs** — the README, this file, and code comments are all
  fair game.

## Getting set up

### Prerequisites

- [Node.js](https://nodejs.org) 18+
- [Rust](https://www.rust-lang.org/tools/install) (via `rustup`)
- Windows: the "Desktop development with C++" workload from the
  [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
  (this is just the compiler toolchain — no Visual Studio IDE or Microsoft
  account required)

### Run it

```bash
npm install
npm run tauri dev
```

This opens the real desktop window with hot reload on both the React and
Rust sides — edits to `src/` and `src-tauri/src/` are picked up live.

### Before opening a PR

```bash
npm run build          # TypeScript + frontend build
cd src-tauri && cargo check   # Rust compiles cleanly
```

Both should pass without errors. There's no separate test suite yet — if
you're adding one, that's a welcome contribution on its own.

## Project conventions

A few things that keep the codebase consistent — worth skimming before you
dive in:

- **Rust owns the filesystem, processes, and database. React owns
  presentation and state.** The frontend never touches disk or spawns
  processes directly — it always goes through a Tauri command. Keeping this
  boundary firm is the single most important convention in the codebase.
- **One command module per feature area** under `src-tauri/src/commands/`
  (`instance.rs`, `server.rs`, `mods.rs`, etc.) — add new commands to the
  module they belong to, and register them in `lib.rs`'s
  `invoke_handler![...]` list.
- **One feature folder per area** under `src/features/` (`dashboard`,
  `console`, `settings`, `java`) — components, not just pages, live there
  if they're specific to that feature.
- **Never edit an already-applied SQL migration** in
  `src-tauri/migrations/` — SQLx checksums migration files, so editing one
  breaks every existing database. Add a new migration instead, even to
  undo something.
- Match the style already in the file you're editing (naming, comment
  density, formatting) rather than introducing a new convention.

## Commit messages / PRs

- Keep commits focused — one logical change per commit is easier to
  review and revert if needed.
- Describe *what* changed and *why* in the PR description; the diff
  already shows *how*.
- Small, focused PRs get reviewed faster than large ones. If a change
  naturally splits into independent pieces, consider separate PRs.

## Questions?

Open a [Discussion](../../discussions) or an issue — there's no such
thing as a silly question here.
