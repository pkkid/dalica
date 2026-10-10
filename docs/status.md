# Status

Where the project stands right now, for agents and humans. The **lead** keeps it current (workers
report numbers in their PRs). Live task state is on the task board (`cargo xtask tasks status`,
[`task-board.md`](task-board.md)); the narrative history is the private dev log (`log/devlog.md`)
and git.

## Current

- **Milestone:** M0, skeleton and gates. Merged on `main`: M0.1–M0.6, M0.8, M0.10. M0.7 (CI on
  three OSes) is waiting for its first run on GitHub (PR #33); M0.9 (docs match reality) is in
  progress. Then the **M0.R** review gate (the maintainer's).
- **Gates:** `cargo xtask ci` runs fmt, clippy, tests, `layers`, `deps`, `assets`, `names`, `parity`,
  `docs` and the roadmap parse; `cargo xtask accept M0` shows 6/6 gates bite.
- **Board:** `cargo xtask tasks` drives issues #1–#32 on project 3 (adopted 2026-10-09 with no
  duplicate). M0 and M1 are still built by one agent (`AGENTS.md` §4).
- **Needs the maintainer's review:** the reviewed dependency exceptions in `xtask/src/deps.rs`
  (`C_BUILD_ALLOW`: wayland-sys, wayland-backend, x11-dl; `objc-sys` in `SYS_ALLOW`), and dropping
  eframe's `default_fonts` (Ubuntu Font Licence) until Inter is installed (M1.16).
- **Blockers:** none.

## Metrics

| Date | Machine | Metric | Value |
|---|---|---|---|
| 2026-10-09 | maintainer desktop (24 cores, 31 GB, Fedora 44, Rust 1.98.1) | `cargo xtask accept M0`, warm (6/6 gates bite) | 1.3 s wall |
| 2026-10-09 | GitHub ubuntu-latest | CI job `cargo xtask ci`, cold cache / warm cache (run 38018640880) | 3 min 04 s / 50 s |
| 2026-10-09 | GitHub macos-latest | CI job `cargo xtask ci`, cold cache / warm cache (run 38018640880) | 2 min 25 s / 37 s |
| 2026-10-09 | GitHub windows-latest | CI job `cargo xtask ci`, cold cache (no warm run yet: the first run failed, so no cache was saved) | 4 min 36 s |
| 2026-10-09 | maintainer desktop | `cargo xtask ci` on a clean clone, cold target dir | 46.5 s wall |
