# Status

Where the project stands right now, for agents and humans. The **lead** keeps it current (workers
report numbers in their PRs). Live task state is on the task board (`cargo xtask tasks status`,
[`task-board.md`](task-board.md)); the narrative history is the private dev log (`log/devlog.md`)
and git.

## Current

- **Milestone:** M1, star a photo end to end. M1.1–M1.15 and M1.20 are merged; the data path works
  headlessly and `cargo xtask accept M1 --headless` passes. The maintainer confirmed the M1.A review
  on 2026-10-10 (all decisions, all recommendations; dev log). Next: M1.16–M1.19 (the UI) through
  the full board loop, then M1.R.
- **Gates:** `cargo xtask ci` runs fmt, clippy, tests, `layers`, `deps`, `assets`, `names`, `parity`,
  `docs` and the roadmap parse; `cargo xtask accept M0` shows 6/6 gates bite. Layer rule 4 now scans
  code after a test module (it used to stop at the first `#[cfg(test)]`).
- **Board:** `cargo xtask tasks` drives issues #1–#35 on project 3. M1 is built by one agent; from
  M1.16 it uses `claim` / `submit` / `merge` (`AGENTS.md` §4).
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
| 2026-10-10 | maintainer desktop | `dalica-core` position keys, longest key after 10 000 inserts: append / prepend / at index 1 / after the previous insert | 6 / 6 / 8 / 9 bytes |
| 2026-10-10 | maintainer desktop | `dalica-core` position keys, inserts into the middle before a key passes 512 bytes | 2 379 |
| 2026-10-10 | maintainer desktop | `cargo xtask accept M1 --headless` (200 JPEGs, star/undo, rebuild, lock), warm | 2.0 s (test 1.45 s) |
| 2026-10-10 | maintainer desktop | scan of 1 000 `scenes` JPEGs in 10 folders: walk + headers / with diff and apply (tmpfs, warm cache, dev build) | 6.4 ms / 8.2 ms |
| 2026-10-10 | maintainer desktop | 256 px thumbnail of a 6000 × 4000 JPEG (decode + area-average resize, dev build at opt-level 3) | 108 ms |
| 2026-10-10 | maintainer desktop | `demo_library(3, 200)` (200 JPEGs with EXIF and thumbnails, dev build) | 1.05 s, 2.1 MiB |
| 2026-10-10 | maintainer desktop | `cargo test -p dalica-engine --test panic_hunt -- --ignored` (10 commands × 300 cases + 64 sequences) | 0.3 s |
| 2026-10-10 | maintainer desktop | `cargo xtask ci`, warm, test step | 7–16 s (≈ 5 s of it generating 1 000 scan-test photos) |
| 2026-10-10 | GitHub ubuntu / macos / windows | CI job `cargo xtask ci`, warm (run 38029115337, M1.14) | 1 min 51 s / 2 min 17 s / 2 min 31 s |
| 2026-10-10 | maintainer desktop, fresh clone of 193a89b, target dirs on NVMe | cold build `cargo build --workspace --all-targets` | 27.7 s |
| 2026-10-10 | same | `cargo xtask ci` cold / warm | 95.2 s / 20.6 s |
| 2026-10-10 | same | `cargo test --workspace` (warm) | 16.5 s, 285 tests (+2 ignored: `panic_hunt`) |
| 2026-10-10 | same | `cargo xtask accept M0` / `accept M1 --headless` | 2.9 s / 1.7 s |
| 2026-10-10 | same | target dir after a cold build / after ci; free disk | 3.3 GB / 5.4 GB; 124 GB |
| 2026-10-10 | GitHub ubuntu / macos / windows | CI job `cargo xtask ci` (run 38029673539, 193a89b) | 1 min 55 s / 2 min 15 s / 3 min 25 s |
| 2026-10-10 | — | parity summary | 1 ✅ / 2 🟡 / 123 ⬜ / 16 🚫 (142 rows, 10 commands) |
