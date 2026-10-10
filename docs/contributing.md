# Contributing

Adapted from PhotoCraft `docs/contributing.md` and `book/src/getting-started/contributing.md`
(revision 59684266) and the contributor sections of LightCraft `AGENTS.md` (revision 88ea1a8);
MIT OR Apache-2.0.

Humans and AI agents follow the same rules. Read [`AGENTS.md`](../AGENTS.md) first, then
[`process.md`](process.md) (how we work) and [`development.md`](development.md) (build, test,
xtask). This page is the short list a pull request is checked against.

> Status: there is no code yet. The rules below apply from the first commit of milestone M0; the
> paths they name are the planned ones from [`architecture.md`](architecture.md).

## Rules

- **Language:** Rust only. No C/C++ dependencies in the product, no JavaScript, TypeScript,
  webview or Tauri.
- **Licence:** contributions are MIT OR Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`;
  the licence itself is a reserved decision until the maintainer confirms it, see
  [`roadmap.md`](roadmap.md)). Code reused from LightCraft or PhotoCraft is listed in `NOTICE` with
  its source revision, and vendored files keep a header naming their origin.
- **Clean-room:** match the reference organizer's layout, workflow and behaviour; never copy its
  code, text, icons, images, sounds, sample photos, help text or data, not even as placeholders.
  Never copy GPL/LGPL/AGPL code. Never write the reference product's name in the repository
  (`cargo xtask names` fails). Details: [`process.md`](process.md) §3.
- **Assets:** only original work, public domain/CC0, CC-BY(-SA), OFL or permissively licensed
  material, with its licence file next to it and a row in [`ATTRIBUTION.md`](../ATTRIBUTION.md) in
  the same commit. Fonts: Inter is the only font file in the repository; everything else comes
  through the optional `CRAFT_FONTS_DIR` build input. When in doubt about a licence, don't use it.
  Details: [`assets.md`](assets.md).
- **Never lose a photo:** originals change only on an explicit Save; every write is atomic and
  merges rather than replaces; deletes go to the OS trash. Full rules in
  [`data-model.md`](data-model.md) and `AGENTS.md` §2.
- **Never crash:** non-test code must not panic: no `unwrap`/`expect`/`panic!`/`unreachable!`/
  `todo!`/`unimplemented!` and no `unsafe`. Errors go through `Result` and `?`, input-derived
  indices and sizes are checked, and every crash fix ships with a regression test. See
  [`standards/never-crash.md`](standards/never-crash.md).
- **Commands, not handlers:** new features are engine commands with tests, and the UI calls them
  (checklist below).
- **Layering:** `cargo xtask layers` must pass. Register new crates in `xtask/src/layers.rs`.
- **Tests:** required for every change. Readers and writers need round-trip and malformed-input
  tests; anything that writes to a photo folder needs a test proving the original is untouched.
  Test photos are procedural (`dalica-scenes`) or public domain/CC0 from the pinned corpus; never
  commit media and never use personal photos.
- **Style:** `cargo fmt`, and `cargo clippy --all-targets -- -D warnings`. Match surrounding code.
  Comments explain *why*. No bare `TODO`; use `TODO(maintainer): …` for questions only a human can
  answer.
- **UI:** use `theme::Tokens` and `widgets::*`; never hard-code a colour. User-facing text is an
  English literal wrapped for translation (`tl!("…")`, see [`localization.md`](localization.md)).
  Verify visually through the control channel or a headless `dalica-cli snapshot` before
  submitting.
- **Privacy:** no network access without a user action, no telemetry, never personal details in
  requests (generic User-Agent `Dalica-dev`).
- **Commits:** small, focused, green, with the task id in the subject (below).

## Adding a command

1. **Find the id.** Search `crates/ui/src/menu_catalog.rs` for the menu item. Using its id makes
   the menu item live with no further UI work. Commands without a menu entry use a descriptive id in
   the same style (`photo.star.toggle`, `folder.description.set`) and an empty menu path.
2. **Spec and parity row.** Find or add the feature's row in [`parity.md`](parity.md) and its spec
   in `docs/` ([`menus-and-shortcuts.md`](menus-and-shortcuts.md) for the menu path and shortcut).
   Write the spec first if it is missing.
3. **Algorithm** goes in the lowest crate that fits (`edit` for pixels, `library` for model changes,
   `sidecar`/`meta` for persistence), with unit tests. Pixel code never assumes 8-bit or sRGB input
   and is deterministic (seeded randomness).
4. **Command** goes in an engine module (`crates/engine/src/cmd/<area>.rs`; prefer a new module to
   growing a shared file), registered in `cmd/mod.rs`. Fill in:
   - `id`, `label` (English, also the translation key), `menu` path and default `shortcut`,
   - a params doc string such as `{"ids":[photo id],"stars":0..5=1}` (this is what agents read
     through the CLI, the control channel and MCP),
   - an `enabled` predicate (greys the menu item out),
   - `run`, which returns a JSON result and records exactly **one** undo step (coalesce drags and
     typing sessions).
5. **Respect the context:** the current selection versus the Photo Tray, hidden and offline photos,
   read-only legacy files, and the never-lose-a-photo rules for anything that touches disk.
6. **Tests** in the module: behaviour, undo/redo, disabled states, bad params (must return `Err`,
   never panic), and a headless UI test if it has UI. Run
   `cargo test -p dalica-engine --test panic_hunt -- --ignored`.
7. **Dialog** (if it has parameters): compose existing widgets, register automation ids so agents
   can drive it, and add every new user-facing string to the translation catalogs' source set.
8. **Finish:** `cargo xtask ci`, `cargo xtask parity --write`, update the parity row and
   `docs/status.md`, and commit with the task id.

## Pull requests

- **One task id per commit.** The subject starts with the roadmap or parity id
  (`M1.3: journal + snapshot persistence`); the body says what was tested, any numbers (benchmarks,
  test counts) and the decisions you made. Only commit green states.
- **Attribution trailers.** Commits made with an AI model end with a `Co-Authored-By:` trailer
  naming the model, as the agent environment requires; About ▸ Models is generated from these
  trailers ([`contributors.md`](contributors.md)). Keep trailers intact when squashing or
  rebasing.
- **Before/after screenshots** for every change that touches the UI, taken at 1400 × 900 and, where
  the layout changes, 900 × 600, through the control channel or `dalica-cli snapshot`. Use the
  procedural demo library or public-domain images, never personal photos.
- **Self-review** with `/code-review` at high effort (or an equivalent review pass) before opening
  the PR. Changes to anything that writes to the user's photo folders, sidecar or metadata writers,
  and delete or move commands also get a second, independent review focused on data safety.
- **Credit yourself if you want to:** add your own entry to `contributors/people.toml` in your PR
  ([`contributors.md`](contributors.md)). Agents ask their human once; they never add anyone else.
- **Security-sensitive changes** (parsers, file I/O, sidecar and metadata writers, the control
  channel, MCP, command dispatch): say whether the change adds a control, documents a limitation or
  proposes later hardening, and add a regression test for every fixed defect. Never put live
  exploit payloads, secrets, personal files or unlicensed samples in issues, commits or fixtures.

## How community PRs are integrated

The maintainer may integrate community pull requests in **batches** on an integration branch:

1. Each PR is merged onto the integration branch with its authorship (and its `Co-Authored-By`
   trailers) preserved, one PR at a time, with `cargo xtask ci` run between merges.
2. Each batch gets a short security checklist: new dependencies (licence, `*-sys` crates, C code),
   `build.rs` or CI changes, `unsafe`, hidden or bidirectional Unicode, new network access, and
   file writes outside the library and the app's own data directories.
3. Conflicts and follow-up fixes are separate commits on the integration branch, so each
   contributor's work stays recognisable in the history.
4. The integration branch is merged into `main` when green; each PR is then closed with a link to
   the merge commit.

Feedback and bug reports become GitHub issues with a *Done when* line, ideally with a headless UI
test that reproduces them ([`process.md`](process.md) §7).
