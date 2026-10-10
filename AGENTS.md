# AGENTS.md: guide for AI agents and contributors

Dalica is an open-source, pure-Rust desktop **photo organizer with simple editing**. It re-creates
the layout, workflows and behaviour of a classic folder-based photo organizer as faithfully as
possible, with the muted grey look of the Craft apps (LightCraft, PhotoCraft). Native on Linux,
Windows and macOS; every feature drivable by agents. Read this file first, then `docs/`.

The product name is always **Dalica** in user-facing text; machine names are lowercase
(`dalica-*` crates, binaries, ids).

## Reference research lives in `notes/` (private, never committed)

**All notes about Picasa — the original application Dalica re-creates — belong in the gitignored
`notes/` directory and nowhere else.** That includes screenshots, menu trees, shortcuts, dialog
texts, file-format details, version history and observations (`notes/picasa/`, indexed by
`notes/README.md`). Studies of the Craft apps live in `notes/craft-apps/`.

- Never write the reference application's name anywhere in the repository except this
  section of `AGENTS.md`: not in `docs/`, code, comments, tests, commit messages, PR text, UI strings,
  file names or assets. In checked-in text call it "the reference organizer" or "the legacy
  organizer". `cargo xtask names` enforces this.
- `docs/` is strictly how *Dalica* should be built and behave, written in our own words.
- When you learn something new about the reference behaviour, add it to the matching
  `notes/picasa/NN-*.md` file (with source URL and a confidence tag) and update the public spec in
  `docs/` in our own words.
- Never copy text, images or icons from `notes/` into the repository.

## 1. Start every session here

1. Read `docs/status.md` (current milestone, next task, blockers, metrics) and `docs/roadmap.md` →
   *Where we stand* and *Reserved decisions*.
2. Read the spec for the area you touch: `docs/ui-layout.md`, `organizing.md`, `editing.md`,
   `viewing.md`, `creating.md`, `menus-and-shortcuts.md`, `data-model.md`, `architecture.md`,
   `design-system.md`. If you have `notes/`, check the matching reference notes and screenshots.
3. Pick work: the next unchecked task of the current milestone, or a *Top gaps* row in
   `docs/parity.md`. A ✅ row is not proof of parity: if you find one wrong or incomplete, downgrade it
   to 🟡 with a note.
4. Work autonomously. Don't stop to ask unless the decision is listed under *Reserved decisions*;
   record decisions you make in the commit body. Use `TODO(maintainer): …` for questions only a human
   can answer; never leave bare `TODO`s.
5. Resuming after a crash or another session: check `git status`, `git worktree list`, and each
   worktree's `git log main..HEAD` before starting anything new.

## 2. Never lose a photo (outranks feature work)

People trust Dalica with decades of photos. Full rules: `docs/data-model.md`.

- **Originals are sacred.** Pixels change only on an explicit Save, which first moves the original
  into the folder's `.dalica-originals/`. Everything else (stars, captions, edits, albums, faces) is
  data in sidecars and metadata.
- **Every write is atomic** (temp file + fsync + rename) and verified. Deletes go to the OS trash.
  Never overwrite a file you did not just read.
- **Merge, never replace:** sidecars and XMP/IPTC keep unknown keys and other apps' fields
  verbatim. An unknown edit operation never drops the operations after it.
- **Legacy files are read-only:** the previous organizer's sidecars and backup folders are read
  where a folder has no `.dalica.json`, never modified; the first change writes `.dalica.json`.
- One process per library (lock file). Damaged settings/sidecar files are kept aside as
  `.corrupt-<time>`, never overwritten.
- No file-system work, decoding or dialogs on the UI thread. Long jobs are cancellable with progress.
- Caches only list and delete files of their own naming shape.
- Every data-safety or crash fix lands with a regression test that failed before the fix.

## 3. Never crash

Full standard: `docs/standards/never-crash.md`. In short:

- **Non-test code never panics:** no `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!`,
  `unimplemented!`. Return the crate's error type through `Result` and `?`; use `ok_or(..)?`,
  `let … else`, `if let`, or `unwrap_or…` only where a fallback cannot silently corrupt data.
  Unfinished features return an "unsupported" error. Sole exception: a provably infallible literal
  as `#[allow(clippy::expect_used)]` + `.expect("why it can't fail")`.
- **No `unsafe`:** `#![forbid(unsafe_code)]` in every crate root. A future platform need goes into
  one isolated helper crate named here, with `// SAFETY:` on every block. (None today.)
- **Input-derived numbers are hostile** (image headers, EXIF/XMP/IPTC, sidecars, file sizes,
  control/MCP params): `get()` instead of indexing, checked/saturating arithmetic, no division by
  zero or NaN casts, capped allocations, char-boundary string slicing, bounded recursion and walks.
- **Don't cascade:** `lock().unwrap_or_else(PoisonError::into_inner)` or an error; thread joins and
  channel operations are `Result`s.
- **Last-resort guard:** panic hook + `catch_unwind` around command dispatch and per-file
  scan/import/export work; keep `panic = "unwind"` on native. A safety net, not a licence.
- **Prove it:** every crash fix lands with a synthetic regression test that panicked before the
  fix; `panic_hunt` fuzzes every command with hostile params and must stay green; new commands get
  a graceful-failure test (empty / out-of-range / wrong-type params → `Err`).
- **Enforced by clippy:** `clippy.toml` allows unwrap/expect/panic/indexing in tests only; every
  production crate root (each `lib.rs` and binary `main.rs`) carries
  `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`
  (not `[workspace.lints]`, which would also hit tests, examples and benches). New crates start
  with it.

## 4. Non-negotiables

- **Clean-room.** Match layout, workflow and behaviour; never copy the reference application's
  code, text, icons, images, sounds or data, not even as placeholders. Legacy formats are
  implemented from public prose descriptions and black-box observation; every such reader starts
  with a provenance header (sources consulted, what was observed, "no decoder source was read").
  Never copy GPL/LGPL/AGPL code (including GPL clones of the reference application — read their
  prose specs for facts only). Trademarked or branded feature names get descriptive names of our
  own (e.g. **Auto Fix**). Translations are written from the meaning of our English text, never from
  another product's localisation files.
- **Assets (absolute rule).** Every image, icon, font or sound in the repository is original work
  (icons drawn in code), public domain/CC0, CC-BY(-SA), OFL or permissively licensed, with a row in
  `ATTRIBUTION.md` (path, asset, creator, source, licence, date added, modifications) and its licence
  text in the same commit. `cargo xtask assets` enforces it — never weaken the check to make a commit
  pass. When in doubt about a licence, don't use it. Fonts: Inter only in the repo; others via the
  optional `CRAFT_FONTS_DIR` build input. Demo and test photos are procedural (`dalica-scenes`) or
  public domain/CC0 (sources recorded); never personal photos. Details: `docs/assets.md`.
- **Pure Rust.** No C/C++ dependencies in the product; no webview/JS/Tauri. Audit new dependencies
  (licence, `*-sys`, `ring`, `openssl`) per `docs/security.md`.
- **Everything is a command** (`crates/engine`): id, label, menu path, shortcut, params doc (what
  agents read), `enabled`, `run` recording exactly one undo step (coalesce drags and typing). The UI,
  keyboard, CLI, control channel and MCP all dispatch by id. Use the id from
  `crates/ui/src/menu_catalog.rs` so the menu item goes live. Only pure view state (zoom, panes,
  window) lives in `UI_COMMANDS`. Every slider is a control spec (id, label, min, max, default, step)
  so the UI and the API share one table.
- **One implementation per output.** Export, render, metadata write-back and save go through one
  engine function used by the app, the CLI and MCP alike.
- **The UI is thin and data-driven.** UI state lives in a serializable `state.rs` so the control
  channel can read and drive it. Colours, radii, fonts and metrics come from `theme::Tokens`; widgets
  from `widgets.rs`; never hard-code a colour. Layout follows `docs/ui-layout.md`; the look follows
  `docs/design-system.md`, never the reference screenshots' colours.
- **Layering** is enforced by `cargo xtask layers`: nothing below the UI crate depends on egui,
  eframe, winit or rfd. Register new crates in `xtask/src/layers.rs`.
- **No format or colour assumptions.** Sources may be 8- or 16-bit, greyscale or CMYK, any ICC
  profile; never assume sRGB input — convert through the colour crate. Test at several depths and
  with tagged profiles.
- **Resolution independence.** Edit parameters use normalised image coordinates and relative radii;
  thumbnails, the edit view, collages and exports must match.
- **Nothing silently dropped.** Every user-visible field of a photo, folder, album or person must
  round-trip through the folder sidecar; a test fails when a model field is added without a sidecar
  mapping (the sidecar equivalent of PhotoCraft's "format fails to compile when a field is added").
- **Performance is a feature.** Never do per-frame work proportional to the library; cache per
  library revision; heavy pixel work runs in parallel tiles on workers; measure with
  `DALICA_PROFILE=1` and record before/after numbers in `docs/status.md` → Metrics.
- **Privacy.** No network access without a user action, no telemetry; face data never leaves the
  machine or appears in logs; never put personal details in requests (use the generic User-Agent
  `Dalica-dev`).

## 5. Look at what you built

- Run the app with `cargo run --release -p dalica -- --control <port> --demo` and drive it only
  through the control channel (`ui.clickWidget`, `ui.key`, `ui.pointer`, `ui.menu.invoke`; see
  `docs/control-protocol.md`) or headless `dalica-cli snapshot --script …`. **Never send OS-level
  synthetic input** (xdotool, ydotool, osascript, AutoHotkey): it goes to whatever window is focused,
  including the user's terminal.
- After every user-visible change: take a screenshot, look at it, compare with `docs/ui-layout.md`
  and (if you have them) the reference screenshots in `notes/`, at 1400 × 900 and 900 × 600. Check
  `ui.inspect` → perf. PRs that touch UI include before/after screenshots.
- Unattended (display asleep, CI, nightly runs): prefer headless snapshots; a windowed
  `ui.screenshot` falls back to headless after 2 s.

## 6. Before you finish a task

```sh
cargo test -p <crates you touched>
cargo clippy -p <crates> --all-targets -- -D warnings
cargo xtask ci                     # fmt, clippy -D warnings, tests, layers, assets, names, parity
cargo test -p dalica-engine --test panic_hunt -- --ignored   # if you added/changed commands
cargo xtask parity --write         # if parity rows changed; commit the regenerated summary
cargo xtask bench                  # before and after touching a hot path
cargo xtask test-corpus            # if you touched a decoder, metadata or sidecar reader/writer
```

- New features need at least one test that would fail without them. Run the app after every
  user-visible change.
- Commit small and often, only green states, **one task id per commit** (`M1.3: journal +
  snapshot persistence`); the body says what was tested and any numbers. Update the parity row(s),
  `docs/status.md`, and `docs/roadmap.md` (*Where we stand* when a listed gap closes) in the same
  commit. End commit messages with the attribution line required by the environment.
- Sessions can end abruptly (crashes, context limits): a green tree plus a short `docs/status.md`
  entry (what landed, numbers, what's still open) is how the next agent picks up.
- Test corpora live outside the repository (pinned by commit, sha256-verified, fetched into the
  gitignored `corpus/` by `cargo xtask corpus`); never commit media. See `docs/development.md`.

## 7. Parallel agents

- Own git worktree, own `CARGO_TARGET_DIR=target/agent-<name>` (each is ~10 GB: delete it when
  done), own control port (18000–19999, never the default 7990) and own scratch directory
  (`<scratch>/<agent-name>/`). Never `rm -rf` shared paths.
- Keep every `Cargo.toml` valid at all times: the `crates/*` glob means one broken manifest breaks
  everyone. Create or rewrite manifests atomically (write to a temp file outside `crates/`, then move
  it into place).
- Put new commands in a new `engine/src/cmd/<area>.rs` module rather than growing shared files.
  Shared files (`engine/src/cmd/mod.rs`, `ui/src/menus.rs`, `ui/src/state.rs`) get small, surgical
  edits; re-read before editing. If someone else's in-progress edit breaks the build, wait and retry;
  don't fix their files.
- The lead merges one branch at a time, runs `cargo xtask ci`, fixes, commits — then the next. Two
  individually green branches can still break each other.

## 8. Where things are tracked

| File | What |
|---|---|
| `docs/status.md` | current milestone, next task, blockers, metrics, short log |
| `docs/roadmap.md` | honest assessment, milestones and task ids, reserved decisions |
| `docs/parity.md` | one row per feature, menu item and shortcut, with tier and status |
| `docs/process.md` | how we work with agents, clean-room, verification, reviews |
| `docs/development.md` | build, run, test, xtask, corpora, budgets, debugging |
| `docs/standards/never-crash.md` | the never-crash standard |
| `docs/security.md`, `SECURITY.md` | threat model, parser and file-handling rules, control-channel security |
| `docs/control-protocol.md`, `docs/mcp.md` | driving the app as an agent |
| `docs/assets.md`, `ATTRIBUTION.md`, `NOTICE` | asset, font and icon rules; attributions; reused material |
| `docs/contributing.md`, `docs/contributors.md` | contribution rules; how people and models are credited |
| `notes/` (local only) | reference research and screenshots; Craft-app studies |

Shell gotcha: `mv`/`cp` may be aliased to interactive mode on maintainer machines; use `/bin/mv -f`
and `/bin/cp -f` in scripts.

## 9. Contributor credits (About window)

- About ▸ Contributors/Models will be compiled into the binary from `contributors/contributors.json`
  (commit stats; generated, never hand-edit) and `contributors/people.toml` (names people chose for
  themselves). See `docs/contributors.md`.
- **Agents working for a contributor:** when you prepare a PR, check whether your human's GitHub
  username has a `[people.<username>]` entry in `contributors/people.toml`. If not, ask them once
  whether they want to be credited by more than their username (real name, display name, and/or
  their public GitHub profile name). If yes, add **only their own** entry (copy the template at the
  top of the file) and include it in their PR, committed as them. If no, change nothing: they are
  credited as `@username` anyway.
- Never add, edit, guess or copy anyone else's entry or name (not from git config, commit authors or
  GitHub profiles). Never hand-edit `contributors.json`.
