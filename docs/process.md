# How we build Dalica

Dalica is built mostly by AI coding agents (Claude Code, occasionally Codex) steered by a human
maintainer, following the process that let the Craft apps (PhotoCraft, LightCraft) reach usable
parity with their reference applications in days. This document is the process; `AGENTS.md` is the
short rule sheet agents load every session; `development.md` is the mechanics (build, test, tools).

## 1. The loop

```text
reference research (private notes/)  ─┐
                                      ├─► spec in docs/ (our words) ─► parity row + task id
honest assessment (docs/roadmap.md) ──┘                                     │
                                                                            ▼
             commit (one task id, green)  ◄─ cargo xtask ci ◄─ look at it (screenshot) ◄─ implement as commands + tests
                    │
                    └─► update parity row, tick the task, dev log, submit → lead merges
```

1. **Research privately.** Everything learned about the reference application (screenshots, menu
   trees, shortcuts, dialog contents, sidecar formats, observed behaviour) lives in the gitignored
   `notes/` directory with stable ids. It is never committed, quoted into code, or copied into
   docs verbatim.
2. **Specify publicly, in our own words.** `docs/` describes how *Dalica* behaves: layout,
   workflows, data model, menus. Specs are precise enough to implement and test without opening the
   notes. Where a behaviour is uncertain, the spec says so and names the decision as open.
3. **Track parity.** Every feature, menu item and shortcut has a row in [`parity.md`](parity.md)
   with a stable id, a tier (P0/P1/P2/OOS) and a status. Work is picked from *Top gaps*.
4. **Implement as commands** (see `architecture.md` §3) with tests, in the lowest crate that fits.
5. **Look at it.** Drive the app through the control channel or a headless snapshot script, take a
   screenshot, and compare with the reference screenshots in `notes/`. UI work is not done until
   someone has looked at the pixels.
6. **Gate and submit.** `cargo xtask ci` green; one task id per commit; tick the task and update
   its parity row; dev-log entry; `cargo xtask tasks submit`. The lead merges, keeps
   `docs/status.md`, and updates the roadmap's *Where we stand* at milestone reviews.
7. **Test fully once, at the review** (maintainer decision, 2026-10-15). During a milestone a
   task's gate tests only the packages it changed, quick, and a merge runs no tests. The full list
   (every test at full strength, every milestone's acceptance, Windows and macOS, budgets, corpus,
   fuzzing) runs once, at the milestone's review; what fails there becomes fix tasks merged before
   the review closes (`development.md` §3).

## 2. Private reference notes (`notes/`)

`notes/` is gitignored and local to the maintainer's machine. It holds:

| Path | Contents |
|---|---|
| `notes/README.md` | index of the notes, conventions, id scheme |
| `notes/<ref>/01-…` to `08-…` | numbered research documents: versions and visual reference, library view, edit view, organizing, data and files, viewing, create and share, menus/shortcuts/options |
| `notes/<ref>/screenshots/` | reference screenshots with an `INDEX-*.md` per topic (source URL, version, description) |
| `notes/craft-apps/` | studies of LightCraft and PhotoCraft (architecture, UI, process) |

Rules:

- Never commit anything from `notes/`, and never paste its text, screenshots or the reference
  product's name into code, comments, commit messages, UI strings, tests, or `docs/`.
- Parity ids and specs may *point* at a notes section ("see notes, library view §3") so an agent
  with the notes can check details; contributors without them work from `docs/` alone.
- When an agent learns something new about the reference behaviour, it adds it to the right notes
  file (with source and confidence tag) and updates the public spec in our own words.

## 3. Clean-room rules

- Match **layout, workflow and behaviour**; never look at or use the reference application's
  **code**, and never copy its **assets, text or data**. No icons,
  bitmaps, sounds, fonts, sample photos or help-text from the reference application or its vendor,
  not even as placeholders.
- Observation of the reference application, if done at all, happens in a disposable VM or
  container, on synthetic or CC0 photos only, never against a real personal photo library.
- Formats (legacy sidecars, databases) are implemented from public prose descriptions and our own
  black-box observation. Every reader of an undocumented format starts with a provenance header:
  sources consulted, what was observed, and the statement that no decoder source was read.
- Never copy GPL/LGPL/AGPL code (other photo managers and their importers). Permissive code
  (MIT/Apache/BSD/ISC) may be reused with attribution in `NOTICE`.
- Branded or trademarked phrases from the reference product are replaced with descriptive names of
  our own (e.g. the one-click automatic fix is **Auto Fix**). Generic labels ("Crop", "Retouch",
  "Straighten", "Folder Manager") are fine.
- Every asset in the repository is original, public domain/CC0, CC-BY(-SA), OFL, or permissively
  licensed, with a row in `ATTRIBUTION.md` added in the same commit (`cargo xtask assets` enforces
  it). Demo photos are procedural or CC0, never personal.
- `cargo xtask names` fails if the reference product's name appears in any tracked file other than
  `AGENTS.md` (the single allowed mention is the reminder about the notes folder).

## 4. Working with AI agents

### Models and tools

- **Lead / implementation:** Claude Code with the strongest available model (currently Claude Opus
  5.5; Fable 5.1 for long autonomous runs if available). Large-context sessions for cross-crate work.
- **Second opinion:** Codex (via the Claude Code Codex plugin) for independent diagnosis or review
  of risky changes.
- **Cheap parallel work:** Sonnet 5.5 subagents for mechanical tasks (translations, test
  scaffolding, doc regeneration).
- Commits carry a `Co-Authored-By:` trailer naming the model; the About window credits models from
  these trailers (M6).

### Sessions

- An agent session starts by reading `AGENTS.md`, `docs/status.md`, the task board
  (`cargo xtask tasks status`) and the task's spec in `docs/`.
- The canonical prompt for a worker is short: *"You are agent-<n>. Read AGENTS.md. Take tasks from
  the board with `cargo xtask tasks next` / `claim`, follow the operating protocol, submit each
  one, and keep going until `next` has nothing for you."* The lead's prompt names it the
  integrator (`docs/task-board.md` §6).
- **Operating protocol:** agents don't stop to ask unless the decision is in the *Reserved
  decisions* list in `roadmap.md`. They record decisions they make in the commit body, and
  maintainer decisions are recorded inline with a date ("maintainer decision, 2026-10-09").
  Code never contains bare `TODO`s; use `TODO(maintainer): …` for questions only a human can answer.
- **Crash and context-loss resilience:** commit small and often (a crash loses minutes, not hours),
  only green states, and write a dev-log entry after every task (`cargo xtask tasks log`). A
  resuming agent checks `git status`, `git worktree list`, each worktree's `git log main..HEAD`
  and the board (`tasks status`: what it had claimed) before starting.

### Parallel agents

- Up to 8 agents on the maintainer's machine: one lead (integrator) and workers, coordinated by
  the GitHub task board ([`task-board.md`](task-board.md)). Workers pick tasks themselves with
  `cargo xtask tasks next` / `claim`; local locks stop two agents from claiming the same task or
  overlapping files.
- Each agent: its own git worktree, `CARGO_TARGET_DIR`, control port and scratch directory
  (`cargo xtask tasks env <agent>`). Never `rm -rf` shared paths; keep every `Cargo.toml` valid at
  all times (write to a temp file, then move it into place).
- New commands go in new modules (`engine/src/cmd/<area>.rs`) rather than growing shared files.
- The lead merges **one branch at a time** (`cargo xtask tasks merge`: rebase,
  `cargo xtask ci --merge`, squash-merge), then the next. Two individually green branches can still
  break each other: the merge gate proves everything still builds, and the review's full run finds
  the rest. GitHub runs CI on `main` only when the lead dispatches it; a red run there is fixed
  before the next merge.
- When running many sessions, use a notification hook (Claude Code `Stop` / permission-request
  hooks) so the maintainer hears when an agent needs input.

### Code review

- No AI review bot is required. Every PR gets: a green local gate, a self-review by the authoring agent
  (`/code-review` at high effort before opening the PR), before/after screenshots for UI changes,
  and a maintainer review (human, optionally assisted by a Claude Code or Codex review pass).
- Risky areas (anything that writes to the user's photo folders, sidecar/metadata writers, delete
  and move commands) require a second, independent agent review focused on data safety.
- Community PRs may be integrated in batches on an integration branch, each PR merged with its
  authorship preserved, with a short security checklist per batch: new dependencies, `build.rs`
  or CI changes, `unsafe`, hidden Unicode, network access, file writes outside the library.

## 5. Measuring parity honestly

The Craft apps learned that a "menu items wired" count reached 100 % long before the apps behaved
like their references. Dalica tracks three different numbers and never confuses them:

1. **Wiring:** share of parity rows ✅ (generated by `cargo xtask parity`).
2. **Behaviour:** pass rate of **workflow acceptance tests** — scripted end-to-end tasks run over
   MCP against the real engine (e.g. *add a folder of 200 CC0 photos → star 5 → create an album →
   caption 3 → crop and Auto Fix one → export the album at 1600 px → check files and metadata*).
   This is the headline number.
3. **Feel:** a dated, prose **honest assessment** in `roadmap.md` (*Where we stand*), by dimension
   and by kind of user, updated only with measured facts. Agents read it before picking work.

A ✅ row is not proof of parity: an agent that finds a ✅ feature wrong or incomplete downgrades it
to 🟡 with a note. Floors (tests that only allow numbers to rise) protect measured numbers.

## 6. Visual verification

- Every UI change is verified by screenshot at 1400 × 900 (and at 900 × 600 when it changes
  layout), compared with the reference screenshots in the notes and the specs in `ui-layout.md` / `design-system.md`.
- Use the control channel (`ui.clickWidget`, `ui.key`, `ui.menu.invoke`, `ui.screenshot`) or
  `dalica-cli snapshot --script`. **Never** use OS-level synthetic input (xdotool, osascript,
  AutoHotkey): it reaches whatever window is focused, including the user's terminal.
- PRs that change the UI include before/after screenshots.

## 7. Releases and community

- Ship early (0.1 when the library view and folder scanning are real), with an honest status
  banner in the README rather than parity percentages.
- A `release` branch builds signed, checksummed **draft** releases for Linux (AppImage, Flatpak,
  deb/rpm), Windows (MSI) and macOS (DMG) — copy the Craft release workflow when we get there.
- Feedback becomes GitHub issues with "Done when" acceptance criteria, ideally including a
  headless UI test.
