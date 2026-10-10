# Status

Where the project stands right now, for agents and humans. The **lead** keeps it current (workers
report numbers in their PRs). Live task state is on the task board (`cargo xtask tasks status`,
[`task-board.md`](task-board.md)); the narrative history is the private dev log (`log/devlog.md`)
and git.

## Current

- **Milestone:** M0, skeleton and gates. Every M0 task is merged on `main` (M0.1–M0.10); CI is
  green on Linux, Windows and macOS. The **M0.R** review is done; closing #11 starts M1.
- **Gates:** `cargo xtask ci` runs fmt, clippy, tests, `layers`, `deps`, `assets`, `names`, `parity`,
  `docs` and the roadmap parse; `cargo xtask accept M0` shows 6/6 gates bite.
- **Board:** `cargo xtask tasks` drives issues #1–#32 on project 3 (adopted 2026-10-09 with no
  duplicate). M0 and M1 are still built by one agent (`AGENTS.md` §4).
- **M0 review (2026-10-09):** the maintainer confirmed all 27 decisions listed in the review (dev
  log), including the dependency exceptions and eframe without GL or default fonts, and approved the
  edits: M1.2 gains `fs::append`, new M1.20 (app log file), M1.15/M1.16/M1.18/M1.19 adjusted,
  shared registration files, done-whens never write this file, the single agent uses the full board
  loop after M1.A.
- **Blockers:** none.

## Metrics

| Date | Machine | Metric | Value |
|---|---|---|---|
| 2026-10-09 | maintainer desktop (24 cores, 31 GB, Fedora 44, Rust 1.98.1) | `cargo xtask accept M0`, warm (6/6 gates bite) | 1.3 s wall |
| 2026-10-09 | GitHub ubuntu-latest | CI job `cargo xtask ci`, cold cache / warm cache (run 38018640880) | 3 min 04 s / 50 s |
| 2026-10-09 | GitHub macos-latest | CI job `cargo xtask ci`, cold cache / warm cache (run 38018640880) | 2 min 25 s / 37 s |
| 2026-10-09 | GitHub windows-latest | CI job `cargo xtask ci`, cold cache (no warm run yet: the first run failed, so no cache was saved) | 4 min 36 s |
| 2026-10-09 | maintainer desktop | `cargo xtask ci` on a clean clone, cold target dir | 46.5 s wall |
