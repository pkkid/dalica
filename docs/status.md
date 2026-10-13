# Status

Where the project stands right now, for agents and humans. The **lead** keeps it current (workers
report numbers in their PRs). Live task state is on the task board (`cargo xtask tasks status`,
[`task-board.md`](task-board.md)); the narrative history is the private dev log (`log/devlog.md`)
and git.

## Current

- **Milestone:** M7, the edit view, is done: M7.1–M7.31 are merged and `cargo xtask accept M7`
  passes (53 s; 117 s with the release budgets). The edit view with its filmstrip, zoom and pan;
  Basic Fixes (Crop with suggestions, Straighten, Retouch, Auto Fix, Auto Contrast, Auto Color,
  Fill Light) and the Tuning tab; each photo's own Undo / Redo history; edits shown on every
  thumbnail. Next: M8, Save and export; its issues open when the M7.R review PR is merged.
- **M7 review (2026-10-13):** the maintainer answered four questions: a real Windows file
  identity for the read-before-write guard (new M8.27); the Tuning sliders show the photo's last
  Tuning after later edits (new M8.28); starring in the edit view stays a library step and Fill
  Light stays part of Tuning; all 86 fingerprint baselines approved, with the look of Auto Fix,
  Auto Contrast, Auto Color and the Tuning controls. Check-in pages now open every image larger on
  click (a `/milestone-review` rule). Fix tasks M8.27–M8.30 carry what the run found.
- **Numbers (M7.R, 53ca1f0, 2026-10-13):** tests 2 242 passed, 4 ignored; `cargo xtask ci` 412 s
  cold, 196 s warm; accept M0–M7 5 / 6 / 12 / 39 / 52 / 62 / 66 / 53 s; edit view (release, 24 MP):
  open p50 77.2 ms cold, 21.7 ms cached (budget 150), slider draft p95 8.4 ms (16), final p95
  14.1 ms (100), next/previous p95 0.0 ms (prefetched; budget 50); GitHub CI Ubuntu 14:13, macOS
  13:47, Windows 24:11, Budgets 6:34; parity ✅ 22 · 🟡 59 · ⬜ 63 · 🚫 16 (160 rows, 132 commands).
  Run: 31 tasks + 1 red-main fix in 12 h 27 min with 7 workers; 12 send-backs on 9 tasks.
- **Milestone:** M6, photo info, metadata and Release 0.1, is done: M6.2–M6.43 and the pulled M7.1
  are merged and `cargo xtask accept M6` passes (62 s standalone). Rotate, hide, captions, tags and
  quick tags, dates, folder info, the Properties pane, About and Release Notes; captions, tags and
  dates written into JPEG, PNG, WebP and TIFF and imported on scan; packages for Linux, Windows and
  macOS at version 0.1.0. Next: shipping 0.1 (the maintainer creates the `release` environment and
  pushes `release`), then M7, the edit view; its issues open when the M6.R review PR is merged.
- **M6 review (2026-10-12, the Release 0.1 gate):** the maintainer signed off on shipping 0.1;
  confirmed both xmp-interop.md §9 rules as built (a removed tag stays in `lr:hierarchicalSubject`;
  IPTC is written whenever a JPEG's caption or tags change); kept HEIF out of release builds until
  M10 decides; left `dalica.exe --version` in a console for later; approved the 53 fingerprint
  baselines, asking that the title bar show the app icon (a fix PR with this review). Mid-run the
  maintainer replaced the app icon (#313), approved publishing our corpus sets, and answered
  reserved decision 11 (option b, M7.1). Fix tasks M7.25–M7.29 carry what the run found.
- **Milestone (M5):** M5, folders, scanning and formats, is done: M5.1–M5.25 are merged (and M6.1,
  the application id `org.dalica.Dalica`) and `cargo xtask accept M5` passes (98 s; 651 s with the
  release budgets). The library follows the user's folders (Folder Manager, first run, watching,
  verification, offline volumes) and reads every format of `formats.md` §1. Next: M6, photo info,
  metadata and Release 0.1; its issues open when the M5.R review PR is merged.
- **M5 review (2026-10-12):** the maintainer answered six questions: the display stays sRGB for
  now; the speed thumb keeps its defaults; the lead fills our own CC0 corpus in M6 (M6.42); our own
  standard-places lookup stays (no `directories`); the ≤ 10 s mount-point window gets a fix task
  (M6.43); all 70 fingerprint baselines approved. Mid-run, reserved decision 10 was answered
  (`org.dalica.Dalica`). Fix tasks M6.38–M6.41 carry what the run found.
- **Milestone (M4):** M4, library browsing, is done: M4.1–M4.20 are merged and `cargo xtask accept M4`
  passes (90 s; 155 s with the release budgets). Dalica browses a library: the folder tree, the
  continuous light table with section headers, badges and captions, the speed-control scroll bar,
  selection, and the Photo Tray with Hold. Next: M5, folders, scanning and formats; its issues open
  when the M4.R review PR is merged. M6's task blocks are written too.
- **M4 review (2026-10-11):** the maintainer accepted the split check of "a scanned 100 k library at
  full thumb speed" (100 k sketched + 10 k scanned; M5.16 adds a scanned 100 k pass); asked for a
  Linux release-budget CI job on `main` (new M5.24); left the speed thumb's feel for M5.R (defaults:
  exponent 2, top speed 40); approved all 46 fingerprint baselines. Two review blocks carry M4's
  loose ends: M5.22 (board tooling; running acceptance once in ci was done on the review branch at
  the maintainer's request) and M5.23 (band autoscroll,
  tray keys, centred minis).
- **M3 review (2026-10-10):** the maintainer approved all 28 fingerprint baselines; asked for the dim
  text to reach 4.5:1 on the chrome (new M4.20); allowed `inotify-sys` and `fsevent-sys` for the
  `notify` watcher (M5.11); and will create the `pkkid/dalica-corpus` repository before M5 (M5.15).
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
| 2026-10-10 | maintainer desktop, clean checkout of 5c41c00 (M3.11), target dir on NVMe | `cargo xtask ci` cold / warm | 266 s / 158 s |
| 2026-10-10 | same | tests in `cargo xtask ci` | 605 pass, 2 ignored (`panic_hunt`) |
| 2026-10-10 | same | `cargo xtask accept M3` (live headless app 2.8 s, crate suites 37.9 s) / `accept` M0 / M1 / M2 | 41 s / 3.2 s / 3.8 s / 12.2 s |
| 2026-10-10 | same | `panic_hunt` (16 registered commands) | 0.33 s |
| 2026-10-10 | same | `dalica-cli snapshot --demo-fresh` 1400 × 900 / 900 × 600 (dev build) | 1.1 s / 0.45 s |
| 2026-10-10 | GitHub ubuntu / macos / windows | CI job `cargo xtask ci` (run 38103647913, M3.10) | 4 min 45 s / 4 min 18 s / 8 min 27 s |
| 2026-10-10 | maintainer desktop | the measurement target dir / all target dirs; free disk | 9.2 GB / 156 GB; 65 GB |
| 2026-10-10 | — | M3 run: 12 tasks + 1 lead task, 4 workers, 19:19–22:21; send-backs 3 (M3.4, M3.9 rebase, M3.10); extra files granted to 3 tasks; `main` red twice on macOS (#116, and the M3.11 floor) | — |
| 2026-10-10 | — | parity summary (M3.R) | 2 ✅ / 16 🟡 / 110 ⬜ / 16 🚫 (144 rows, 16 registered commands); VIEW-FULLSCREEN 🟡 until F11 is seen on a real screen |
| 2026-10-11 | maintainer desktop (i7-13700K, 24 cores, 31 GB, Fedora 44, Rust 1.98.1), clean checkout of e73af84 (M4.18), load 1–6 | `library_bench` 100 k photos, release: scroll p50 / p95 / max | 0.234 / 0.261 / 0.737 ms (budget p95 ≤ 8 ms); 10 k p95 0.265 ms |
| 2026-10-11 | same | cells visited per frame, 10 k / 100 k (min / p50 / max) | 13 / 35 / 42 and 12 / 35 / 42 (flat) |
| 2026-10-11 | same | speed thumb at top speed through 100 k sketched photos (release) | 0 placeholder cells in 5 317 frames; p95 0.31 ms |
| 2026-10-11 | same | packed cache vs loose files, 100 k entries of ~6 KB, µs per read (warm) | first (CRC) 3.5 / 86 · in order 1.4 / 7.4 · shuffled 4.7 / 2.5; write 0.94 s / 6.85 s; 604 MB in 11 files / 799 MB in 100 000 |
| 2026-10-11 | same | colour sketch; index growth for 100 k sketches | 24–26 bytes (cap 32); journal +4.49 MB, snapshot 11.18 → 15.67 MB (45 B per photo) |
| 2026-10-11 | same | tree model, 5 000 folders / 100 k photos (release) | flat 3.0–4.3 ms, tree 6.7–7.3 ms |
| 2026-10-11 | same | `cargo xtask accept M4` debug / with `DALICA_ACCEPT_RELEASE=1` | 89.9 s / 154.9 s |
| 2026-10-11 | same | `cargo xtask accept` M0 / M1 / M2 / M3 | 3.6 / 4.5 / 12.5 / 47.2 s |
| 2026-10-11 | same | `cargo xtask ci` cold / warm | 575.9 s / 411.7 s (test step 474 / 342 s; acceptance runs twice, M5.22) |
| 2026-10-11 | same | tests | 820 pass, 3 ignored (`panic_hunt` ×2, `make_the_large_demo_library`) |
| 2026-10-11 | GitHub ubuntu / macos / windows | CI job `cargo xtask ci` (run 38161284768, e73af84) | 20 min 37 s / 15 min 44 s / 30 min 42 s |
| 2026-10-11 | maintainer desktop | the measurement target dir / all target dirs; free disk | 14 GB / 267 GB; 39 GB |
| 2026-10-11 | — | M4 run: 20 tasks + 1 fix, 6 workers, 23:11–13:50 (idle 02:15–09:30 on a push permission); send-backs 6; 8 tasks landed by the lead (parity summary conflicts, In Review re-submits); `main` red once on macOS (#155) | — |
| 2026-10-11 | maintainer desktop | `cargo xtask ci` warm after the CI-time fix (review/M4.R) | 488.1 s → 84.8 s (test step 329.2 → 66.6 s + `test acceptance` 1.4 s; `accept` M0–M4 157.2 → 15.3 s; 827 → 821 test results, the 7 acceptance tests once instead of twice, +1 new `xtask` test); standalone `accept M4` 89.1 → 47.5 s; `dalica-ui --test never_blank` 30.4 → 10.7 s |
| 2026-10-11 | — | parity summary (M4.R) | 8 ✅ / 31 🟡 / 89 ⬜ / 16 🚫 (144 rows, 54 registered commands) |
| 2026-10-12 | maintainer desktop (i7-13700K, 24 threads, 31 GB), 6d94f5f (M5.20) | first screenful of a just-added folder, 1 k / 10 k JPEGs (release, `first_thumbnails`) | 67–134 ms / 500–517 ms (budget 1 s) |
| 2026-10-12 | same | index scan; thumbnails made | ≈ 19 900 photos/s; 1 224–1 562 photos/s |
| 2026-10-12 | same | scanned 100 k library at top thumb speed (release) | 0 placeholder cells; owner frame p95 0.75 ms; scanned with thumbnails in 30.3 s (3 299 photos/s) |
| 2026-10-12 | same | `cargo xtask budgets` (4 budgets: 100 k scroll, thumb at top speed, first thumbnails, scanned 20 k) | all held in 170 s; p95 0.29 / 0.34 / — / 0.52 ms |
| 2026-10-12 | same | `cargo xtask accept M5` debug standalone / with `DALICA_ACCEPT_RELEASE=1` / inside ci | 97.7 s / 651 s / ≈ 6 s |
| 2026-10-12 | same | `panic_hunt` (61 registered commands) | 0.63 s |
| 2026-10-12 | same | corpus: PngSuite decoded / corrupt refused; CC0 raws decoded, embedded thumbnails | 162 of 176 / 14 of 14; 20 of 26, 13 |
| 2026-10-12 | same | local `cargo xtask ci` cold / warm | not measured: the scratch tmpfs ran out of room (9.6 GB target) |
| 2026-10-12 | GitHub ubuntu / macos / windows | CI job `cargo xtask ci` (run 38234692409, 6d94f5f); tests | 8 min 49 s / 10 min 13 s / 19 min 43 s; 1 440 / 1 439 / 1 415 pass |
| 2026-10-12 | — | M5 run: 25 tasks + M6.1 + 1 red-main fix, 7 workers, 21:20–05:40 (paused ~1 h on a push permission); 26 re-submits over 14 tasks (mostly neighbouring parity rows); median claim → merge 1.0 h; `main` red once on Windows (a test race, #222) | — |
| 2026-10-12 | — | parity summary (M5.R) | 9 ✅ / 47 🟡 / 85 ⬜ / 16 🚫 (157 rows, 61 registered commands) |
| 2026-10-12 | maintainer desktop (i7-13700K, 24 threads, 31 GB, Fedora 44) | local `cargo xtask ci` cold / warm (c12d1e3); tests | 423 s / 146 s; 1 858 pass, 4 ignored |
| 2026-10-12 | same | `cargo xtask accept M0`–`M6` standalone, debug | 4.4 / 6.2 / 12.0 / 46.9 / 55.1 / 63.4 / 62.0 s |
| 2026-10-12 | same | `panic_hunt` (98 registered commands) | 1.11 s |
| 2026-10-12 | same | corpus (pinned 6239cad): PngSuite; raw; photos; heif; metadata writes | 162 of 176 (14/14 corrupt refused); 26 of 26 (15 embedded); 57 of 57 (41); 3 of 3; 441 writes, 325 written, 0 failures |
| 2026-10-12 | same | first thumbnails, release: 1 k / 10 k JPEGs (budget 1 s) | 133.5 ms / 500 ms |
| 2026-10-12 | GitHub ubuntu (4 CPUs) | Budgets job: first thumbnails 10 k (c12d1e3 / 033db0e / fix branch) | 850 / 801 / 684 ms (M6.32 alone: 1 217 ms, fixed by #312) |
| 2026-10-12 | same | tag index, 100 k photos × 5 tags, release | 110 ms |
| 2026-10-12 | GitHub ubuntu / macos / windows | CI job `cargo xtask ci` (run 38323360294, #310) | 7 min 25 s / 13 min 27 s / 16 min 07 s |
| 2026-10-12 | GitHub release dry run (run 38333449479) | packages: Linux x86_64 (tar, deb, rpm, AppImage, Flatpak), Windows (MSI, zip), macOS (DMG, CLI zip) | tar 18 MB, deb 18, rpm 18, AppImage 17, Flatpak 12; MSI 12.7 MiB, zip 15.8 MiB; DMG 20.2 MB, CLI zip 12.6 MB |
| 2026-10-12 | — | M6 run: 43 tasks + M7.1 + 2 red-main fixes (#312, #315), 7 workers, 09:56–21:43; ~30 send-backs (rebases, parity rows), 9 needs-file round-trips; 3 Windows flakes (accept M1 root-caused in M6.36); /home full once | — |
| 2026-10-12 | — | parity summary (M6.R) | 15 ✅ / 55 🟡 / 74 ⬜ / 16 🚫 (160 rows, 98 registered commands) |
