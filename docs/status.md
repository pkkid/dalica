# Status

Where the project stands right now, for agents and humans. The **lead** keeps it current (workers
report numbers in their PRs). Live task state is on the task board (`cargo xtask tasks status`,
[`task-board.md`](task-board.md)); the narrative history is the private dev log (`log/devlog.md`)
and git.

## Current

- **Milestone:** M15, video, languages, the OS and Release 1.0, is done: M15.1–M15.54 are merged (53 tasks with 7
  workers, plus 13 fix PRs during the run and the review's fixes) and `accept M15` passes (7 / 7, 168 s). Videos in the
  library, the player with trim, Save Frame and Export Clip on a pure-Rust H.264 decoder with AAC sound (HEVC behind the
  non-default `hevc` feature); English and Thai switched live (Thai as drafts); screen-reader names and keyboard reach
  for every widget; Locate on Disk, Open With, external editor, clipboard files, drag out, default viewer, one Dalica per
  user, cameras over USB on Linux; SBOMs and advisories per release, a from-source Flathub manifest. Version 1.0.0:
  M15.R is the 1.0 release gate, published unsigned once the review PR's runs are green and the maintainer merges it.
  Next: M16 (release 1.1.0), themes and colour search plus M15.R's follow-ups; its issues open when the M15.R review PR
  is merged.
- **M15 review (2026-10-20):** HEVC non-default like HEIC; the decoders' unsafe accepted with fuzz targets as the gate,
  fuzz run at reviews only (security.md §7.2 rewritten); 1080p30 is the playback bar, 4K60 and HDR best effort; the 12
  phone clips published to `dalica-corpus`; cameras over USB on Linux only; Wayland without drag out, the macOS default
  viewer as a how-to, cpal for sound on Windows and macOS confirmed; folder music, the webpki 0.102 copy and the audit's
  parity fixes to M16; the 0.9.0 legacy-folder bug fixed with release notes only (no migration); the withheld-names path
  limit accepted; the Flathub manifest required for 1.0; 1.0 ships unsigned; Thai ships marked as drafts; M16 is 1.1.0
  with the Soft themes; all 50 fingerprint baselines approved; ship after the fixes.
- **Numbers (M15.R, 3e84c2c3, 2026-10-20):** `cargo xtask ci --full` 20.7 min (i7-13700K, peak 2.3 GB): tests 4 715
  passed, 1 failed (accept M10, a test broken by the Language option, fixed at the review), 2 ignored on purpose; every
  milestone's acceptance M0–M15 otherwise passes; release budgets 24 / 24 held (100 k scroll 1.39 ms p95, first screenful
  767 / 500 ms for 1 k / 10 k, cold start 202 ms, idle 90 MiB, import 29.1 files/s, camera import 20.0 files/s, video 4K
  decode 6.21 ms, focal-zoom draft 12.82 ms of 16); `edit_preview` all held (open cold 64.4 ms median, drag 6.68 ms p95);
  parity ✅ 187 · 🟡 47 · ⬜ 16 · 🚫 17 (267 rows, 428 commands, all 113 P0 ✅); workflows 17 / 17; i18n 1 603 keys,
  English and Thai 100 %; +94 606 / −6 154 lines. Run: 53 tasks with 7 workers from 14:54 on 2026-10-19 to 09:35 on
  2026-10-20; median claim→merge about 1 h 16 min (long tails while `main` was red); 9 send-backs over 8 tasks; two
  squash merges reverted siblings' work (restored by #1106); three app crashes from a decoder harness's memory.
- **Milestone:** M14, collage and legacy sidecars and Release 0.9, is done: M14.1–M14.29 are merged (26 planned
  tasks, three added during the run: M14.27 an exact-name create so a folder's first `.dalica.json` never replaces
  another program's file, M14.28 five loose ends, M14.29 the maintainer's Places bubble fix) and `cargo xtask accept
  M14` passes (130 s; 137 s with the release budgets). The Collage room with six styles, the canvas, Create Collage,
  Save Draft and Edit Collage, the Collages project; the previous organizer's per-folder files read on the first scan,
  never written, the first change writing `.dalica.json`, Undo Save and Revert from its kept originals; W15 and W16
  over MCP. Version 0.9.0: M14.R is the 0.9 release gate, published once the review PR's GitHub runs are green and the
  maintainer merges it. Next: M15, video, languages, the OS and Release 1.0; its issues open when the M14.R review PR
  is merged.
- **M14 review (2026-10-19):** reserved decision 2 = pure-Rust decoders for H.264 and HEVC after a spike on real
  phone files (tiles only for a codec that fails); sound = pure-Rust output per OS; the first languages = English and
  Thai for 1.0 if Thai can be done (shaping and line breaking to be proven), the maintainer's wife reviews the Thai;
  the release budgets run locally only, the Budgets workflow removed from GitHub (M15.43, a same-runner base for
  M14's budgets, dropped); accept M12's narrower frame check accepted; five look issues from the screenshots carried
  to M15 (M15.44); all 56 fingerprint baselines approved; ship 0.9 once the review PR's runs are green; the by-hand checks EDIT-ZOOM and SYS-WINDOW split off as P2 rows that never gate CI (the maintainer, after the review PR's accept jobs failed on them).
- **Numbers (M14.R, 01170527, 2026-10-19):** `cargo xtask ci --full` 18.7 min (i7-13700K, 10 jobs): tests 4 329
  passed, 0 failed, 2 ignored on purpose; every milestone's acceptance M0–M14 passes; M14 release budgets: layouts of
  200 pictures 0.00–0.68 ms (budgets 0.05–1.10), a 50-photo 3000 px render 1 400 ms and 43 MiB (2 105 ms, 65 MiB), a
  10 000-photo legacy scan 124.5 ms (195), a 64 MiB legacy file 648 ms (995), the canvas drag frame p95 0.22 ms (0.5);
  parity ✅ 176 · 🟡 45 · ⬜ 29 · 🚫 17 (267 rows, 387 commands); workflows 16 / 16 (floor 16); +44 630 / −1 791 lines.
  Run: 29 tasks with 7 workers from 23:53 on 2026-10-18 to 10:35 on 2026-10-19; median claim→merge about 27 min;
  5 send-backs (3 rebase conflicts in `docs/parity.md`); 14 tasks needed files outside their list; `main` red on GitHub
  for 5 h after the merges, fixed by #976, #977, #978; accept M14 found a write-roots bug, fixed by #982.
- **Milestone:** M13, people and places and Release 0.8, is done: M13.1–M13.51 are merged (44 planned tasks, seven
  tasks added during the run and at the review: M13.45 a data-loss gap, M13.46 a flaky test, M13.47 effect speed
  margin, M13.48–M13.51 follow-ups and the maintainer's answers) and `cargo xtask accept M13` passes (64 s warm). Local
  faces with downloadable or file-installed models behind a licence dialog, the background face scan, the Unnamed
  grid, suggestions, the People group, person sections and the People pane, MWG names in files; geotags in the files,
  the Places pane with an offline map, drag-to-geotag and opt-in OpenStreetMap tiles and search; every effect within
  its 16 ms / 100 ms caps, now budgets; W13 and W14 over MCP. Version 0.8.0: M13.R is the 0.8 release gate,
  published once the review PR is merged. Next: M14, collage and legacy sidecars; its issues open when the M13.R
  review PR is merged.
- **M13 review (2026-10-18):** asked early, when nothing else could run: reserved decision 9 = pure-Rust TLS (rustls +
  rustls-rustcrypto, a licence exception for webpki-roots); 5 = YuNet + SFace (default) / AuraFace, pinned; the
  public-faces corpus published to `dalica-corpus`; 4 = OpenStreetMap tiles and Nominatim, off until allowed;
  people names and face points withheld from automation without the people permission (M13.49); the task gate also
  tests direct dependents (M13.50); Focal Zoom's half-resolution blur accepted. At sign-off: ship 0.8 after the slideshow fix and green
  review-PR runs; all 57 fingerprint baselines approved; the security fuzz targets, SBOMs and an
  advisory check planned as M15.39 and M15.40; M13's automation follow-ups as M14.25. The review PR's
  GitHub runs: Ubuntu, corpus, the four fuzz targets and the release budgets green; macOS (one
  geocoder spacing test) and Windows (cancelled at the 60-minute limit, with two acceptance
  failures) deferred by the maintainer to M14.26.
- **Numbers (M13.R, 75b83f7c, 2026-10-18):** `cargo xtask ci --full` 12.7 min cold / 8.9 min warm (i7-13700K, 16
  jobs): tests 4 158 passed, 2 ignored, 1 failed (an intermittent slideshow upload test: the test raced a worker
  thread; fixed by #914, 0 / 200 under load); every milestone's acceptance M0–M13 passes; release budgets: face detection 41 ms, SFace
  embedding 45 ms, face scan 11 faces/s with the frame at 0.2 ms p95, Unnamed grid of 10 000 faces opens in 21.5 ms,
  Places pane of 10 000 photos 1.05 ms; all 27 effects' drafts 2.3–12.9 ms and finals 13.4–66.2 ms at p95 on two
  cores; public-faces floors: detection 45 / 45, SFace 8 / 10 people grouped, AuraFace 10 / 10, every group pure,
  every suggestion right; parity ✅ 172 · 🟡 47 · ⬜ 29 · 🚫 17 (265 rows, 324 commands); workflows 14 / 14 (floor
  14); +100 139 / −2 822 lines. Run: 51 tasks with 7 workers from 10:30 to 20:00 on 2026-10-18; median claim→merge
  28 min; 10 send-backs (3 rebase conflicts in `docs/parity.md`), 18 tasks needed files outside their list; `main`
  red twice through a dependent's test; one GitHub rate-limit stall (15 min); one 50-minute stop on unasked
  reserved decisions.
- **Milestone:** M12, effects and Release 0.7, is done: M12.1–M12.32 are merged (25 planned tasks, seven fix tasks
  added during the run and at the review, and the lead's M14 blocks) and `cargo xtask accept M12` passes (122 s; 309 s
  with the release budgets). The edit view's tabs 3–5 with all 27 effects: background tile grids, one-click effects,
  the parameter panel with swatches, a picker and an eyedropper, the crosshair, Soften's brush; four effects that grow
  the picture, read at the grown size everywhere; Batch Edit's five effects; W11 over MCP. Version 0.7.0: M12.R is the
  0.7 release gate, published once the review PR is merged. Next: M13, people and places (with M13.34–M13.44, the
  effect speed tasks and the framed tiles); its issues open when the M12.R review PR is merged.
- **M12 review (2026-10-18):** the maintainer answered: the effect render caps (draft 16 ms, final 100 ms) become
  per-effect speed tasks in M13 and 0.7 ships with "effects slow at large sizes" as a known issue (M12.21 landed the
  met budgets); the import listing and first screenful are judged relative to the last release on the same runner,
  interleaved three times (M12.26); Sharpen's and Film Grain's two-size checks at their default and milder settings
  stand; all 47 fingerprints and the 13 re-blessed acceptance baselines approved; Drop Shadow's defaults made larger
  (M12.32); the two checks by hand (EDIT-ZOOM pinch, SYS-WINDOW on macOS) move to M13.R; ship 0.7 after the listed
  fixes and green review-PR runs. Fix tasks M12.29–M12.31 came from the review's screenshots.
- **Numbers (M12.R, 0aad5fda, 2026-10-18):** `cargo xtask ci --full` 11.0 min (i7-13700K, 10 jobs): tests 3 749
  passed, 2 ignored, 0 failed, plus accept M12 (7 passed, 122 s; run alone since the gate runs only finished
  milestones); release budgets (M12.22): effect tile grids tab 3 / 4 / 5 171 / 168 / 161 ms (≤ 355 / 350 / 360),
  slideshow upload frame p95 5.07 ms (≤ 6.8), empty-recipe final p95 6.21 ms (≤ 12) and Save 27.5 ms (≤ 55), the 27
  effects' 24 MP Saves within their budgets; effect drafts and finals printed for M13 (two cores, 24 MP: 15 finals
  over 100 ms, Focal Zoom 2 683 ms; 13 drafts over 16 ms); parity ✅ 167 · 🟡 42 · ⬜ 39 · 🚫 17 (265 rows, 263
  commands); workflows 12 / 12 (floor 12); +66 092 / −1 293 lines. Run: 25 tasks with 7 workers from 23:03 on
  2026-10-17 to 03:00 on 2026-10-18, then the review's 8 tasks; 5 send-backs, `main` red once for 50 minutes (an
  edit-only change broke a ui test), a render deadlock found by three workers and fixed.
- **Milestone:** M11, import, compare and viewing and Release 0.6, is done: M11.1–M11.29 are merged (26 planned tasks,
  three fix tasks added during the run, and the lead's M13 blocks) and `cargo xtask accept M11` passes (111 s; 480 s
  with the release budgets). Import from cards and drives with checked copies and card removal only after the
  question; compare A│B and A│A; Copy and Paste All Edits, Batch Edit and Undo All Edits; the slideshow; the quick
  preview; `dalica --view`. Version 0.6.0: M11.R is the 0.6 release gate, published once the review PR is merged.
  Next: M12, effects; its issues open when the M11.R review PR is merged.
- **M11 review (2026-10-17):** the maintainer answered: slideshow music comes with M15's video sound; `dalica --view`
  stays read-only while Dalica is open until M15's OS integration; M11's behaviour-changing decisions stand as built;
  the two checks by hand (EDIT-ZOOM pinch, SYS-WINDOW on macOS) move to M12.R; all 106 fingerprint baselines
  approved; the slideshow transition frame's budget is 1 ms; ship 0.6 once the review PR's checks are green. Fix
  tasks M12.24 (slideshow upload off the UI thread) and M12.25 (M11's small follow-ups) carry the rest.
- **Numbers (M11.R, 4f917df8, 2026-10-17):** `cargo xtask ci --full` 11.6 min (i7-13700K, 8 jobs, load up to 51):
  tests 3 545 passed, 3 ignored, 1 failed (m7-900x600, stale after M11.21; re-blessed on the review branch), plus a
  full-strength proptest counterexample in the transition easing (fixed on the review branch); release budgets:
  import first screenful 10.2 ms (≤ 35), listing 209 ms (≤ 320), copy 27.9 files/s and 2.89 MB/s (≥ 22.9, ≥ 2.3),
  Batch Edit ×1 000 4.29 ms and undo 3.05 ms (≤ 7, ≤ 5), slideshow 0 of 30 changes waited, advance 0.18 ms (≤ 24),
  transition frame 0.32 ms (budget 0.3, raised to 1 by the maintainer), cold start 193 ms, scan 22 891 photos/s, idle
  memory 85 MiB; parity ✅ 160 · 🟡 42 · ⬜ 44 · 🚫 17 (263 rows, 259 commands); workflows 11 / 12 (floor 11); +69 453 /
  −3 053 lines, 74 test files added. Run: 29 tasks with 7 workers from 13:03 to 17:33 on 2026-10-17, 7 send-backs,
  4 rebase conflicts (docs only).
- **Milestone:** M10, preferences, MCP and Release 0.5, is done: M10.1–M10.57 are merged (36 planned tasks and 20
  fix tasks added during the run; M10.30 dropped) and `cargo xtask accept M10` passes (68 s; 158 s with the release
  budgets). Tools ▸ Options… with General (UI scale, font size, tooltips, animations), File Types, Export and the
  Keyboard Shortcuts editor; `dalica-cli mcp`, an MCP server headless or connected to a running app, with roots and a
  read-only mode; 9 of 12 agent workflows pass; P0 parity 109 / 111 ✅ (two rows await checks a person makes). Version
  0.5.0: M10.R is the 0.5 release gate, published once the review PR is merged. Next: M11, import, compare and
  viewing; its issues open when the M10.R review PR is merged.
- **M10 review (2026-10-16):** the maintainer answered: 12-bit JPEG decoding and the DCT/MPF speed-ups move to later
  rows (DATA-FMT-JPEG ✅ for what 0.5 decodes); release builds ship without an HEVC decoder (patent licensing): HEIC
  photos are organized, read their date and EXIF from the container and show a tile with a hint (M10.56), the `heif`
  feature stays for people who build Dalica themselves, and the HEIF test set stays local; Windows keeps the
  check-then-rename for Save's original; a folder with a damaged sidecar holds write-back into its photo files too
  (M10.57); agents may save new exports and Save As files into a watched folder (`control-protocol.md` §5.7); the P0
  audit's moves to later milestones stand, with a test task for the details it named untested (M11.26); the planning
  decisions that change behaviour stand as built.
- **Numbers (M10.R, 757bf7a, 2026-10-16):** `cargo xtask ci --full` 8.6 min (i7-13700K, 8 jobs, under load): tests
  3 304 passed, 5 ignored, 3 failed (three acceptance pictures that late UI changes moved; re-blessed in M10.27);
  release budgets: scan 22 779 photos/s (≥ 500), cold start at 100 000 photos 200 ms median (≤ 2 s), idle memory
  84 MiB (≤ 600), M9's search and M8's save budgets met; the first thumbnails at 10 000 photos missed once under load
  (6 769 ms, budget 1 000; 517 ms on a re-run); parity ✅ 147 · 🟡 36 · ⬜ 62 · 🚫 17 (262 rows, 181 commands; after M10.56);
  workflows 9 / 12 (floor 9); +49 194 / −4 793 lines, 84 test files added. Run: 54 tasks with 7 workers from 15:36 on
  2026-10-15 to 08:27 on 2026-10-16, 3 send-backs; four data-safety bugs found and fixed before release (a failed
  journal append, captions written into multi-picture JPEGs, a killed Save on Windows, pending sidecar writes
  reported as none).
- **Milestone:** M9, albums, search and file operations, is done: M9.1–M9.30 are merged and `cargo xtask accept M9`
  passes (93 s alone; 92 s with the release budgets). Albums across folders with a manual order that survives a
  rebuild, Starred Photos, Album Properties, Add to Album; search as you type with filters, the age slider, the banner,
  Save Search Results and Show Duplicate Files; move, copy, rename, Move to New Folder, Split Folder Here, folder moves
  and delete to the trash, by menu, keyboard and drag and drop, each undoable and repaired at the next start if
  interrupted. Version 0.4.0: M9.R is the 0.4 release gate, published once the review PR is merged. Next: M10,
  preferences, MCP and Release 0.5; its issues open when the M9.R review PR is merged.
- **M9 review (2026-10-15):** the maintainer answered six questions: Recently Updated counts only changes after a
  photo is first indexed, then shows (fix task M10.29); the search budgets are re-measured on the Budgets job's
  slower runner class (M10.30); `color:` search comes after 1.0 (M16); the planning decisions that change behaviour
  stand as built (delete undo on Linux and Windows only, copies' undo trashes them, folders move within the library,
  same-name folders merge, English month names, Faces greyed until M13, duplicates in one section); all 83
  fingerprint baselines approved; ship 0.4 after fixing the picture that stayed the pre-Save one until a restart (a
  separate fix PR, #521, merged 2026-10-15; it also fixed the flaky drag test and re-blessed the two M8 header
  baselines, whose old images had the stale picture in them; those two are approved at the next review). Fix tasks
  M10.29–M10.33 carry the rest of the run's follow-ups.
- **Numbers (M9.R, 9023bea, 2026-10-15):** tests 3 098 passed, 4 ignored; `cargo xtask ci` 837 s cold, 417 s warm;
  accept M0–M9 alone 7 / 8 / 12 / 53 / 57 / 66 / 89 / 100 / 111 / 93 s; release (24 cores, p50): search at 100 k photos
  index build 428 ms (budget 630), one word 1.1 ms (1.7), three words and a filter 0.37 ms (0.6), `-word` 3.2 ms (5.1),
  `search.set` to applied 10.6 ms (22.3); Save 271 ms, rotate-only 286 ms (budget tightened to 455), export 6.6 s;
  interrupted file operations ≈ 690 cases, none losing data; GitHub CI Ubuntu 20:55, macOS 19:48, Windows 42:50,
  Budgets 7:45 (the index-build row misses on GitHub's slower runner class, a review question); parity ✅ 29 · 🟡 72 ·
  ⬜ 46 · 🚫 16 (163 rows, 172 commands). Run: 30 tasks in about 16 h 36 min with 7 workers; 6 send-backs; the run's
  tests found and fixed seven data-safety bugs before anything shipped.
- **Milestone:** M8, save and export, is done: M8.1–M8.30 are merged and `cargo xtask accept M8`
  passes (105 s alone; 148 s with the release budgets). Save writes edits into the photo and keeps
  the original in `.dalica-originals/` (Undo Save and Revert bring it back byte for byte); a second
  Save renders from the kept original; rotate-only JPEG saves are lossless; formats Dalica cannot
  write get a JPEG beside them; Save a Copy and Save As; Export to Folder with sizes, quality,
  numbered names and a watermark, in the background with Cancel; exported folders under Projects;
  an interrupted Save is repaired at the next start. Version 0.3.0: M8.R is the 0.3 release gate,
  published once the review PR is merged. Next: M9, albums, search and file operations (0.4); its
  issues open when the M8.R review PR is merged.
- **Numbers (M8.R, 8b5bcf5, 2026-10-14):** tests 2 662 passed, 4 ignored; `cargo xtask ci` 602 s
  cold, 308 s warm; accept M0–M8 alone 5 / 6 / 12 / 42 / 53 / 62 / 69 / 57 / 105 s; release
  (24 cores): Save of a 24 MP JPEG p50 292 ms (budget 1050 ms on 2 cores), lossless rotation p50
  357 ms (budget 815), 200-photo mixed export 6.8 s, 34 ms per photo (budget 24 s); JPEG quality
  estimate error 0 over 33 files; GitHub CI Ubuntu 16:51, macOS 24:06, Windows 32:22, Budgets 8:39;
  parity ✅ 24 · 🟡 62 · ⬜ 60 · 🚫 16 (162 rows, 142 commands). Run: 30 tasks and 3 red-main or
  flake fixes in about 11 h 45 min with 7 workers; 23 send-backs. Fault injection found and fixed a
  failed-Save path that orphaned the original (never shipped).
- **Milestone:** M7, the edit view, is done: M7.1–M7.31 are merged and `cargo xtask accept M7`
  passes (53 s; 117 s with the release budgets). The edit view with its filmstrip, zoom and pan;
  Basic Fixes (Crop with suggestions, Straighten, Retouch, Auto Fix, Auto Contrast, Auto Color,
  Fill Light) and the Tuning tab; each photo's own Undo / Redo history; edits shown on every
  thumbnail. Version 0.2.0: M7.R is the 0.2 release gate, published once the review PR is merged.
  Next: M8, Save and export (0.3); its issues open when the M7.R review PR is merged.
- **M7 review (2026-10-13):** the maintainer answered four questions: a real Windows file
  identity for the read-before-write guard (new M8.27); the Tuning sliders show the photo's last
  Tuning after later edits (new M8.28); starring in the edit view stays a library step and Fill
  Light stays part of Tuning; ship 0.2 now, and every milestone up to M15 is a point release
  published on GitHub (0.3 … 1.0); all 86 fingerprint baselines approved, with the look of Auto Fix,
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
