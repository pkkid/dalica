# Status

Where the project stands right now, for agents and humans. The **lead** keeps it current (workers
report numbers in their PRs). Live task state is on the task board (`cargo xtask tasks status`,
[`task-board.md`](task-board.md)); the narrative history is the private dev log (`log/devlog.md`)
and git.

## Current

- **Milestone:** M2, agent eyes, is done: M2.1–M2.9 are merged and `cargo xtask accept M2`
  passes (12 s) on all three OSes. Next: M3, the window shell; its issues open when the M2.R review
  PR is merged.
- **Gates:** `cargo xtask ci` runs fmt, clippy, tests, `layers`, `deps`, `assets`, `names`, `parity`,
  `docs` and the roadmap parse; `cargo xtask accept M0` shows 6/6 gates bite. Layer rule 4 now scans
  code after a test module (it used to stop at the first `#[cfg(test)]`).
- **Board:** `cargo xtask tasks` drives the issues on project 3; four agents ran in parallel in M2.
  One review per milestone, at its end (`M<n>.R`, the maintainer's); a milestone's issues are created
  when the previous review closes (`pulled:` brings one task forward); `merge` waits for GitHub's
  mergeability and syncs the merged roadmap; fingerprint baselines are approved in bulk at the
  review; board reads cost 2 GraphQL points.
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
| 2026-10-10 | maintainer desktop, fresh clone of 22583c0 (M1.R), target dirs on NVMe | cold build `cargo build --workspace --all-targets` | 28.7 s |
| 2026-10-10 | same | `cargo xtask ci` cold (clippy 14.1, test 82.7, accept M0 10.2) / warm | 111.3 s / 29.0 s |
| 2026-10-10 | same | `cargo test --workspace` (warm) | 25.5 s, 319 tests (+2 ignored: `panic_hunt`) |
| 2026-10-10 | same | `cargo xtask accept M0` / `accept M1` / `accept M1 --headless` | 3.0 s / 5.0 s / 1.8 s |
| 2026-10-10 | same | `panic_hunt` (10 engine commands) | 0.33 s |
| 2026-10-10 | same | widget gallery render 480 × 200 / `dalica-cli snapshot --demo` 1400 × 900 / 900 × 600 (dev build) | 76 ms / 1.5 s / 0.8 s |
| 2026-10-10 | same | target dir after a cold build / after ci; free disk | 3.9 GB / 6.7 GB; 110 GB |
| 2026-10-10 | GitHub ubuntu / macos / windows | CI job `cargo xtask ci` (run 38057753715, 22583c0) | 3 min 16 s / 5 min 14 s / 5 min 19 s |
| 2026-10-10 | maintainer desktop | `cargo xtask tasks merge` per task (local ci in `target/merge` + GitHub checks + squash), M1.16–M1.19 | 4:23 / 4:25 / 4:47 / 6:28 |
| 2026-10-10 | — | parity summary (M1.R) | 1 ✅ / 4 🟡 / 121 ⬜ / 16 🚫 (142 rows, 12 commands: 10 engine + 2 UI) |
| 2026-10-10 | maintainer desktop (dev build) | `cargo xtask accept M2` (headless control session + 9 security tests) | 12.1 s (session ~1.5 s, security suite ~10.4 s) |
| 2026-10-10 | GitHub ubuntu / macos / windows | `accept M2` inside CI (PR #99 checks) | pass / pass / pass |
| 2026-10-10 | maintainer desktop (dev build) | control channel: connect + auth / `ui.state` round trip / click / Ctrl+8 / 1400 × 900 screenshot (headless) | 1–3 ms / 0.09 ms / 6 ms / 1 ms / ~1.0 s |
| 2026-10-10 | maintainer desktop (dev build) | windowed demo under control: frame time mean / p95 | 1.23 ms / 1.5 ms (64 fps) |
| 2026-10-10 | maintainer desktop (dev build) | headless frame mean / p95 and most cells laid out in a frame, 200 vs 2 000 photos | 1.71 / 5.65 ms vs 1.63 / 5.85 ms; 42 cells both |
| 2026-10-10 | maintainer desktop | refusing a 1 MiB request line / a 1 MiB line nested too deep (debug, worst case) | 3.6 ms / 3.2 ms |
| 2026-10-10 | maintainer desktop | `cargo xtask ci` test step during M2 (warm) | 38–62 s |
| 2026-10-10 | — | parity summary (M2.R) | 143 rows, 12 registered commands; SYS-CONTROL and SYS-HEADLESS 🟡 |
