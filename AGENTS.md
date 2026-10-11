# AGENTS.md: guide for AI agents and contributors

Dalica is an open-source, pure-Rust desktop **photo organizer with simple editing**. It re-creates
the layout, workflows and behaviour of a classic folder-based photo organizer as faithfully as
possible, with the muted grey look of the Craft apps (LightCraft, PhotoCraft). Native on Linux,
Windows and macOS; every feature drivable by agents. Read this file first, then `docs/`.

The product name is always **Dalica** in user-facing text; machine names are lowercase
(`dalica-*` crates, binaries, ids). Licence: MIT OR Apache-2.0.

## Reference research lives in `notes/` (private, never committed)

**All notes about Picasa — the original application Dalica re-creates — belong in the gitignored
`notes/` directory and nowhere else.** That includes screenshots, menu trees, shortcuts, dialog
texts, file-format details, version history and observations (`notes/picasa/`, indexed by
`notes/README.md`). Studies of the Craft apps live in `notes/craft-apps/`.

- Never write the reference application's name anywhere in the repository except this
  section of `AGENTS.md`: not in `docs/`, code, comments, tests, commit messages, PR text, issue
  text, UI strings, file names or assets. In checked-in text call it "the reference organizer" or
  "the legacy organizer". `cargo xtask names` enforces this.
- `docs/` is strictly how *Dalica* should be built and behave, written in our own words.
- When you learn something new about the reference behaviour, add it to the matching
  `notes/picasa/NN-*.md` file (with source URL and a confidence tag) and update the public spec in
  `docs/` in our own words.
- Never copy text, images or icons from `notes/` into the repository.

## 1. Where things live

| Path | What |
|---|---|
| `docs/` | the spec: start with `docs/README.md` (index), `product.md`, `ui-layout.md` |
| `docs/architecture.md` | crates, layers, commands, threading, testing strategy |
| `docs/roadmap.md` | milestones and **task blocks** (the plan), reserved decisions, honest assessment |
| `docs/task-board.md` | how tasks are claimed, submitted and merged (the live board on GitHub) |
| `docs/status.md` | current milestone, blockers, metrics (kept by the lead) |
| `docs/parity.md` | one row per feature, menu item and shortcut |
| `docs/development.md`, `docs/standards/never-crash.md`, `docs/security.md` | mechanics, crash standard, threat model |
| `crates/<name>/` | library crates (`dalica-<name>`), each with a README saying how to test it alone |
| `apps/dalica`, `apps/dalica-cli` | the desktop app; the headless CLI (`run`, `inspect`, `snapshot`, `mcp`) |
| `tests/acceptance/` | one acceptance test per milestone, run by `cargo xtask accept M<n>` |
| `xtask/` | every gate and tool: `cargo xtask ci`, `layers`, `deps`, `names`, `assets`, `parity`, `accept`, `tasks`, … |
| GitHub [issues](https://github.com/pkkid/dalica/issues) + [project 3](https://github.com/users/pkkid/projects/3) | the **live task board**: what is ready, claimed, in review, done |
| `log/devlog.md` (main checkout, gitignored) | the private dev log: one entry per task |
| `notes/` (gitignored) | reference research and Craft-app studies |

LightCraft (`~/Sources/lightcraft`) and PhotoCraft (`~/Sources/photocraft`) are **reference only**:
study them for techniques and look, never copy their code (`architecture.md` §9).

## 2. Architecture in one screen (details: `docs/architecture.md`)

```text
apps     dalica · dalica-cli
L5       ui ─► ui-kit · mcp           (only L5 and apps may use egui/eframe/winit/wgpu/rfd/muda)
L4       engine                       Session, command registry, keymap, undo, jobs, settings
L3       store · scan · create
L2       library (pure) · thumbs
L1       image · meta · sidecar · edit · scenes · fetch · faces
L0       core · fs                    (only `fs` writes to disk)
```

- A crate depends only on lower layers (listed exceptions: `edit → image`, `meta → image`).
  `cargo xtask layers` enforces it; a new crate is registered in `xtask/src/layers.rs` and
  `architecture.md` §2 in the same commit.
- **Everything is a command** (`crates/engine/src/cmd/<area>.rs`): id, label, menu path, default
  keys with their context, params doc, effect, history, `enabled`, `run`. The UI, keyboard, CLI,
  control channel and MCP all call `Session::execute`. Use the id from
  `crates/ui/src/menu_catalog.rs` so the menu item goes live. Only pure view state lives in
  `UI_COMMANDS`. Every slider is a control spec shared by the UI and the API.
- **One implementation per output** (export, render, metadata write-back, save) used by the app,
  the CLI and MCP alike.
- **The UI is thin and data-driven:** state in the serialisable `crates/ui/src/state.rs`; colours,
  metrics and fonts from `ui-kit` `Tokens`; widgets from `ui-kit`; never hard-code a colour.
  Layout follows `docs/ui-layout.md`; the look follows `docs/design-system.md`.
- **No file system, decoding or dialogs on the UI thread** (the `fs` guard fails tests that try).

## 3. Golden rules

### Never lose a photo (outranks feature work; `docs/data-model.md`)

- **Originals are sacred.** Pixels change only on an explicit Save, which first moves the original
  into the folder's `.dalica-originals/`. Everything else is data in sidecars and metadata;
  metadata writes merge into existing fields and never touch pixels.
- **Every write goes through `fs`:** atomic (temp + fsync + rename) and verified. Deletes go to the
  OS trash. Never overwrite a file you did not just read.
- **Merge, never replace:** sidecars and XMP/IPTC keep unknown keys and other apps' fields verbatim.
  An unknown edit operation never drops the operations after it.
- **Legacy files are read-only.** One process per library. Damaged files are kept aside as
  `.corrupt-<time>`. Caches only list and delete files of their own naming shape.

### Never crash on bad input (`docs/standards/never-crash.md`)

- Non-test code never panics: no `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!`,
  `unimplemented!`; return the crate's error through `Result`. Every crate root has
  `#![forbid(unsafe_code)]` and the clippy `deny` list; `clippy.toml` allows them in tests only.
- Input-derived numbers are hostile (headers, EXIF/XMP, sidecars, file sizes, command params):
  `get()` not indexing, checked arithmetic, capped allocations, bounded recursion and walks.
- Every new command gets a graceful-failure test (empty, out-of-range, wrong-type params → `Err`);
  `panic_hunt` must stay green.

### Every change comes with tests

- New behaviour has a test that fails without it, at the lowest crate that fits. Every crash or
  data-safety fix lands with a regression test that failed before the fix.
- Commands follow the command test contract (`architecture.md` §3); pixel output uses fingerprint
  tests (`architecture.md` §11). Test photos are procedural (`scenes`) or public domain; never
  personal photos; never commit media.

### Never lower a quality metric

- Floors only rise: corpus floors, performance budgets, parity counts, workflow pass rates,
  acceptance checks of finished milestones.
- Never weaken a gate to make a commit pass: no `#[ignore]` or deleted tests, no relaxed lint, no
  `xtask` check loosened, no `DALICA_BLESS` fingerprint update without looking at the diff images
  and saying why in the commit body. If a gate is wrong, raise it on the task's issue.
- New or changed fingerprint baselines merge with their task (images on the PR, the why in the
  commit body); the maintainer approves them **in bulk at the next review** (the milestone's `M<n>.R`),
  and a rejected baseline becomes a fix task (`docs/task-board.md` §7).
- Never claim parity you have not measured. A ✅ parity row is not proof: if you find one wrong,
  downgrade it to 🟡 with a note.

### Non-negotiables

- **Clean-room:** match layout, workflow and behaviour; never copy the reference application's
  code, text, icons, images, sounds or data. Legacy formats are implemented from public prose and
  black-box observation, each reader with a provenance header. No GPL/LGPL/AGPL code. Branded
  feature names get descriptive names of our own (e.g. **Auto Fix**).
- **Assets:** original, public domain/CC0, CC-BY(-SA), OFL or permissively licensed, each with an
  `ATTRIBUTION.md` row and licence text in the same commit (`cargo xtask assets`; `docs/assets.md`).
- **Pure Rust:** no C/C++ build dependencies in the product, no webview/JS/Tauri; audit new
  dependencies (`cargo xtask deps`, `docs/security.md` §8).
- **No format or colour assumptions:** sources may be 8/16-bit, grey or CMYK, any ICC profile.
- **Resolution independence:** normalised coordinates and relative radii everywhere.
- **Nothing silently dropped:** every user-visible model field round-trips through the sidecar (a
  test fails when one is added without a mapping).
- **Performance is a feature:** no per-frame work proportional to the library; cache per revision;
  measure with `DALICA_PROFILE=1` and `cargo xtask bench`, and report before/after numbers.
- **Privacy:** no network without a user action, no telemetry; face data never leaves the machine.
- **Never send OS-level synthetic input** (xdotool, ydotool, osascript, AutoHotkey). Drive the app
  through the control channel or `dalica-cli snapshot`.

## 4. How work is organised: the task board (`docs/task-board.md`)

- **The plan** is the task blocks in `docs/roadmap.md` (`after`, `owner`, `files`, `scope`,
  `done when`). **The live state** is the GitHub board: one issue per task of an open milestone,
  all on [project 3](https://github.com/users/pkkid/projects/3), Status `Todo · Ready · In
  Progress · In Review · Blocked · Done`. **Done** means squash-merged into `main` with the task's
  ✅ ticked.
- **A milestone's tasks become issues when the previous milestone's review closes** (maintainer
  rule, 2026-10-10): until then its blocks are planning text in `roadmap.md`, and `tasks sync`
  creates nothing for them. To bring one task forward, its block gets `pulled: M<k>`: only that
  task gets an issue, during milestone k's run (`docs/task-board.md` §2–§3).
- Touch the board **only through `cargo xtask tasks`**; never edit labels, statuses or fields by hand.
- **Workers** loop:
  1. `cargo xtask tasks next --agent agent-<n>`: pick the first offered task.
  2. `cargo xtask tasks claim <id> --agent agent-<n>`, then `eval "$(cargo xtask tasks env agent-<n>)"`
     and work in the worktree it prints.
  3. Do the task: read its spec in `docs/`, stay inside its `files`, meet its *done when*.
  4. The checklist in §5.
  5. `cargo xtask tasks submit <id>`: the branch now belongs to the lead; start the next task.
  When `next` offers nothing, wait 5 minutes before asking again.
- **Decide, don't wait** (maintainer decision, 2026-10-10). Make technical decisions yourself:
  follow the spec; where it is silent, do what LightCraft or PhotoCraft did (study, never copy;
  `architecture.md` §9); otherwise pick the simplest option that keeps every rule in §3. Record
  each decision in the commit body (what, why, the alternative) so the check-in can list it for
  reversal. Only these need the maintainer, and even they never stop the run (maintainer rule,
  2026-10-10: a milestone runs through its whole backlog without human intervention): run
  `cargo xtask tasks block <id> "question"` (that task, and anything that depends on it, waits),
  take another task, and the question is asked at the milestone's review:
  - a licence exception, a C/C++ dependency or a new `C_BUILD_ALLOW` / `*-sys` entry;
  - anything that could lose or damage users' photos or data, or weaken §3's data rules;
  - loosening a quality gate, a floor or a budget;
  - dropping or descoping a feature or a parity row;
  - a *Reserved decision* in `roadmap.md`.
  Never leave bare `TODO`s; use `TODO(maintainer): …`.
- **The lead asks even less** (maintainer decision, 2026-10-10). Mid-run, the lead does not relay
  questions: blocked tasks' questions and its own hard forks (where a wrong guess means a large
  rewrite) are recorded for the review, which asks them all at once. It interrupts the maintainer
  mid-run only in two rare cases: a push or permission it has no go-ahead for, or nothing left to
  run (every remaining task blocked or waiting on a blocked one), when it prepares the review
  early. Everything else it decides (the most reversible option), and logs what has no commit
  body to live in (board, process, scheduling) with
  `cargo xtask tasks log <check-in id> "DEFERRED: question | options | chose | how to reverse" --agent agent-1`;
  the check-in lists these with the other decisions.
- **Can't finish** (wrong estimate, needs a file outside `files`): comment on the issue and
  `cargo xtask tasks release <id> --reason "…"`.
- **The lead** (`agent-1`) merges In Review tasks one at a time with `cargo xtask tasks merge
  <id>`, releases stale claims, writes the next milestone's task blocks while the current one runs,
  takes the `owner: lead` tasks, and prepares each review task (`/milestone-review`): `accept M<n>`,
  a published check-in page, *Where we stand* and `docs/status.md` on a `review/M<n>.R` branch, a
  pull request.
- **Review tasks are stops** (maintainer rule, 2026-10-10; `roadmap.md` → How milestones work).
  A milestone has exactly one review, at its end: `M<n>.R` (release gates in M6, M10 and M15
  included); there are no mid-milestone reviews (maintainer rule, 2026-10-10: "I do not want any
  checkins mid milestones"; the roadmap parser refuses an open `M<n>.A`). Every review is
  `owner: maintainer`, and nothing listed after an open one is claimed. When the run reaches one,
  the lead lets running tasks finish and merge, runs `/milestone-review` to prepare it (the
  check-in leads with the few **questions** only the maintainer can answer, asked there, and a
  **UI changelog** with before/after screenshots to check by eye), and stops (pushing the
  `review/M<n>.R` branch and opening its pull request are part of preparing it). Its last step
  is a push notification to the maintainer (maintainer rule, 2026-10-10): the `PushNotification`
  tool, which reaches their phone through Remote Control, with one line naming the review, how
  many questions wait and the pull request, e.g. `M3.R ready: 4 questions, PR #123`. **Only the
  maintainer merges a review's pull request or closes its issue;** no agent ever does, not even as
  board housekeeping, and `cargo xtask tasks` refuses to (`submit` and `merge` reject review ids;
  `sync` never closes a review's issue).
- **M0 and M1 (one agent, acting as lead and worker):** up to the M1.A checkpoint, work in roadmap
  order, one branch `task/M<n>.<k>-<slug>` per task, fast-forward `main` after `cargo xtask ci` is
  green, then `cargo xtask tasks sync` marks the task Done and closes its issue (In Progress is set
  by hand with `gh`). After M1.A the same agent uses the full loop (`tasks claim`, `submit`,
  `merge`, with pull requests), so the board's live paths are proven by one agent before eight use
  them; the maintainer gave the standing go-ahead for those `submit`/`merge` pushes at the M0 review
  (2026-10-09). Two more standing go-aheads (2026-10-10): **fixing a red `main`** (push a fix
  branch, open a PR, squash-merge it once its GitHub CI is green on all three OSes, then tell the
  maintainer) and **board housekeeping** (GitHub writes that only bring the board in line with
  `roadmap.md` and `task-board.md`: labels `tasks setup` defines, renaming milestones, closing
  empty ones). Any other push still needs the go-ahead. Parallel workers start at M2.

## 5. Before you finish a task

Run, in your worktree, with your agent environment loaded:

```sh
cargo fmt --all
cargo clippy -p <crates you touched> --all-targets -- -D warnings
cargo test -p <crates you touched>
cargo xtask ci                                               # fmt, clippy, tests, layers, deps, assets, names, parity, docs, roadmap parse, finished milestones' acceptance
cargo xtask accept M<n>                                      # the current milestone, once its acceptance test exists
cargo test -p dalica-engine --test panic_hunt -- --ignored   # if you added or changed commands
cargo xtask parity --write                                   # if parity rows changed; commit the regenerated summary
cargo xtask bench                                            # before and after touching a hot path; put both numbers in the PR (planned, M4)
cargo xtask test-corpus                                      # if you touched a decoder, metadata or sidecar reader/writer (planned, M5)
cargo xtask tasks log <id> "Landed: …  Open: …  Numbers: …"  # the dev-log entry (required by submit)
cargo xtask tasks submit <id> --body-file <evidence.md>   # the PR body: evidence per *done when*, numbers, screenshots
```

- **Portable paths:** CI runs on Windows too. Paths in messages use `/` (as git prints them); tests
  build expected paths with `Path::join`, never by hard-coding a separator. Tests that create files
  leave out names Windows forbids (`" * ? < > | :`, trailing dots or spaces) on Windows.
- **Watch `main`'s CI.** Local `cargo xtask ci` does not cover Windows or macOS: after a push to
  `main`, watch its run (`gh run watch <id> --exit-status`) before the next push (`tasks merge`
  already waits for it).
- **Nested cargo in tests is fine** (cargo releases the build lock while tests run); a test that
  copies the tree and builds it uses its own target directory (see `tests/acceptance/m0.rs`).

And:
- **Look at what you built.** After any user-visible change, take screenshots at 1400 × 900 and
  900 × 600 (`dalica-cli snapshot --script …`, or the control channel on your own port) and compare
  them with `docs/ui-layout.md` (and the reference screenshots in `notes/`, if you have them).
  Attach before/after images to the PR. Check `ui.inspect` → perf.
- **Update the docs in the same branch:** tick your task's ✅ in `roadmap.md`, update its parity
  row(s), and any spec section your work clarified. Do not edit `docs/status.md` (the lead does);
  put numbers in the PR body instead.
- **Commit messages:** one task id per commit subject (`M1.4: scenes: seeded JPEG and PNG
  generator`); the body says what was tested, numbers, and decisions made. End with the
  attribution line the environment requires. The lead squash-merges, so a branch may have several
  commits, but every commit must build.
- **Dev log (every task, no exceptions):** append a short entry to `log/devlog.md` in the **main
  checkout** (not your worktree's copy): what landed, what is still open, the numbers. Use
  `cargo xtask tasks log <id> "…"`, which writes this format (`docs/task-board.md` §8):

  ```markdown
  ## 2026-10-09 14:02 · agent-3 · M1.4 scenes
  Landed: …
  Open: …
  Numbers: …
  ```

  It is private (gitignored, never committed); it is how the next agent picks up after a crash.

## 6. Start or resume a session

1. Read this file, `docs/status.md`, and `docs/roadmap.md` → *Where we stand* and *Reserved
   decisions*. Run `cargo xtask tasks status`.
2. Resuming after a crash: check `git status`, `git worktree list`, each worktree's
   `git log origin/main..HEAD`, the end of `log/devlog.md`, and which tasks the board says you
   hold. Finish or release them before claiming anything new.
3. Read the spec for the area you touch (`docs/README.md` says which file) and, if you have them,
   the matching notes and screenshots.

## 7. Parallel agents (up to 8 on this machine)

- **Own your files.** Work only inside your task's `files`. Need another file? Ask on the issue
  first. Two tasks with overlapping `files` are never in progress together (`tasks next` and
  `claim` enforce it).
- **Shared registration files** (root `Cargo.toml`, `Cargo.lock`, `xtask/src/main.rs`,
  `tests/acceptance/Cargo.toml`) need not be in your `files` and never block a claim; keep edits
  there to the lines you add. The lead re-resolves `Cargo.lock` at merge instead of hand-merging it
  (`docs/task-board.md` §3).
- **Put new work in new modules.** New commands go in a new `crates/engine/src/cmd/<area>.rs`, new
  panels in a new `crates/ui/src/panels/<region>.rs`, rather than growing shared files. Shared
  files (`cmd/mod.rs`, `ui/src/menus.rs`, `ui/src/state.rs`, `menu_catalog.rs`) get small,
  surgical edits; re-read them right before editing.
- **Don't fix another agent's broken files.** If `main` or someone else's in-progress work breaks
  your build, wait and retry, or comment on their issue; never edit their files. If `main` itself
  is red, tell the lead (comment on the most recent merged task) and wait.
- **Your own resources** (from `cargo xtask tasks env agent-<n>`): worktree
  `../dalica.worktrees/agent-<n>`, `CARGO_TARGET_DIR=<main checkout>/target/agent-<n>` (keep it
  warm between tasks), `CARGO_BUILD_JOBS=3`, control port `18000 + 10·n` (never 7990), scratch
  directory `<scratchpad>/agent-<n>/`. Never `rm -rf` shared paths. Never share a
  `CARGO_TARGET_DIR` between two checkouts: cargo can reuse a binary or test built in the other one.
- **Keep every `Cargo.toml` valid at all times:** the `crates/*` glob means one broken manifest
  breaks everyone. Write new or rewritten manifests to a temp file outside `crates/`, then move
  them into place.
- **Only the lead merges**, one branch at a time (`tasks merge`: rebase, `cargo xtask ci`,
  squash-merge); a review task's pull request only the maintainer merges (§4). Two individually green branches can still break each other. The local gate
  decides a merge; GitHub's Linux/Windows/macOS run checks `main` after it, and `tasks merge`
  refuses while the latest finished run on `main` is red: fixing it comes first.

## 8. Contributor credits (About window)

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

Shell gotcha: `mv`/`cp` may be aliased to interactive mode on maintainer machines; use `/bin/mv -f`
and `/bin/cp -f` in scripts.
