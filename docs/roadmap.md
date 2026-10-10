# Roadmap

Milestones for Dalica, with task ids used in commit subjects (`M1.3: …`). Agents pick the next
unchecked task of the current milestone (or a *Top gaps* row in [`parity.md`](parity.md)), keep
`docs/status.md` current, and tick tasks here in the same commit that lands them.

Status legend: ✅ done · 🚧 in progress · ⬜ not started.

## Where we stand

*Honest assessment, 2026-10-09. Agents: read this before picking work. Update it only with
measured facts, dated.*

Nothing is built yet. The investigation stage is done: the reference organizer's layout, workflows,
data formats, menus and shortcuts are documented privately in `notes/`, and the public specs in
`docs/` describe Dalica. On 2026-10-09 the scope was trimmed: dated features (email, printing,
movie maker, timeline, web page, gift folder, screensaver, set as desktop, backup, Red Eye, Text,
novelty effects) were dropped, and the folder sidecar became `.dalica.json` with read-only
fallback to the legacy sidecar (`data-model.md` §3, §9). Next: M0.

## Where we're going

1. M0 — a skeleton that already looks right and can be driven and screenshotted by agents.
2. M1 — a real library on real folders: scanning, the light table, selection, tray, stars.
3. M2 — the edit view with Basic fixes and Tuning, Save/Revert, Export.
Everything else follows the order below unless a *Reserved decision* changes it.

## Milestones

| # | Milestone | Scope | Status |
|---|---|---|---|
| M0 | Skeleton + agent eyes | workspace, xtask gates, theme/widgets/icons, empty window with all regions, commands, control channel, headless snapshots | ⬜ |
| M1 | Library core | folders, scanning/watching, thumbnails, light table, tree, selection, tray, status line, stars, rotate, hide, sidecar v1 | ⬜ |
| M2 | Edit view + output | edit view, Basic fixes, Tuning, undo/redo, Save/Undo Save/Revert, Export, captions | ⬜ |
| M3 | Organize | albums, tags pane, search + filters, file operations, Folder Manager UI, Properties pane, batch edit | ⬜ |
| M4 | Import + MCP | import room (cameras/cards/folders), duplicates, MCP server, workflow acceptance tests | ⬜ |
| M5 | Effects | the 27 effects, effect tiles, compare modes | ⬜ |
| M6 | Viewing | slideshow, full-screen preview, standalone photo viewer, video thumbnails | ⬜ |
| M7 | People + Places | face detection/recognition, People group/pane, Places pane with map, geotag writing | ⬜ |
| M8 | Create | collage room, colour search, duplicates finder | ⬜ |
| M9 | Video | video playback/trim/frame save | ⬜ |
| M10 | 1.0 polish | legacy sidecar reading, preferences complete, localisation, accessibility, packaging, release pipeline | ⬜ |

### M0 — Skeleton + agent eyes (Done when: `cargo xtask ci` is green on a clean clone; `dalica-cli snapshot --demo` renders a window whose regions match `ui-layout.md` §2 at 1400 × 900 and 900 × 600; an agent can list commands, invoke one and take a screenshot over the control channel)

- ⬜ M0.1 Workspace: `Cargo.toml`, `rust-toolchain.toml`, `clippy.toml`, `rustfmt.toml`, crate stubs per `architecture.md` §2 with the never-crash attributes, `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`, `ATTRIBUTION.md`.
- ⬜ M0.2 xtask: `ci`, `layers`, `assets`, `names`, `parity`, `stats`.
- ⬜ M0.3 CI workflow running `cargo xtask ci` on Linux, Windows and macOS for every PR.
- ⬜ M0.4 Theme, widgets, icons vendored from LightCraft with attribution; Inter fonts; token tests (contrast).
- ⬜ M0.5 App shell: eframe window, persisted window state, menu bar from a menu catalog (`menus-and-shortcuts.md`), all regions of `ui-layout.md` drawn with placeholders.
- ⬜ M0.6 Engine: `CommandSpec` registry, `Session::execute`, undo stack, `catch_unwind` guard, `UI_COMMANDS`.
- ⬜ M0.7 Control channel (`--control <port>`), widget registry, `ui.screenshot`.
- ⬜ M0.8 Headless renderer + `dalica-cli snapshot`; procedural demo library (`dalica-scenes`).
- ⬜ M0.9 `panic_hunt` test over all commands.

### M1 — Library core (Done when: adding a folder of 10 k CC0 JPEGs shows first thumbnails in < 1 s and all within the scan budget; the light table scrolls 100 k synthetic photos at ≤ 8 ms/frame; clicking a tree row scrolls to its section and selects it; star/rotate/hide round-trip through the sidecar and survive an index rebuild)

- ⬜ M1.1 `image`: sniff, decode, embedded thumbnails, orientation, ICC → sRGB.
- ⬜ M1.2 `sidecar`: `.dalica.json` reader/writer with the §3 rules and property tests.
- ⬜ M1.3 `library`: model, ops with inverses, journal + snapshot, lock file, rebuild.
- ⬜ M1.4 `scan`: watched folders, walks, `notify` watching, offline volumes.
- ⬜ M1.5 `thumbs`: job pool, LRU, disk cache.
- ⬜ M1.6 Library tree (flat/tree modes, groups, year dividers, sorting).
- ⬜ M1.7 Light table: sections, headers, justified grid, badges, sticky headers, section scrollbar.
- ⬜ M1.8 Selection model, keyboard navigation, tray (Hold/Clear), status line.
- ⬜ M1.9 Star, rotate, hide; first-run panel; Folder Manager (minimal).

### M2 — Edit view + output

- ⬜ M2.1 Edit view layout, filmstrip, zoom/pan, caption bar, histogram box.
- ⬜ M2.2 `edit` renderer + recipe model; thumbnails show edits.
- ⬜ M2.3 Crop (with ratios and suggestions), Straighten, Auto Fix/Contrast/Color, Fill Light.
- ⬜ M2.4 Tuning tab.
- ⬜ M2.5 Retouch.
- ⬜ M2.6 Undo/Redo labels, persistence; Save, Undo Save, Revert, Save a Copy.
- ⬜ M2.7 Export dialog and background export.
- ⬜ M2.8 Captions, metadata write-back (XMP/IPTC) with merge tests.

### M3 – M10

Task lists are written when the previous milestone is half done, from the parity rows of their
areas, each with a *Done when* line.

## Reserved decisions (the maintainer's, not the agents')

Record the decision here with a date when made.

1. Reuse model: depend on LightCraft crates at a pinned git revision, or vendor them? (Proposed:
   vendor UI files, depend on `meta`/`codecs`/`raw` pinned.)
2. Video: pure-Rust decoding vs optional external process (M9).
3. Default for writing stars as XMP ratings (proposed: off) and captions/tags into files
   (proposed: on).
4. Map tiles provider and whether maps are on by default.
5. Face-recognition models and their licences (M7).
6. Password-protected hidden folders. **Decided 2026-10-09: no.**
7. Licence (proposed: MIT OR Apache-2.0, like the Craft apps).
8. Public repository and community process (Discord/GitHub Discussions), release cadence.
9. TLS stack for the few network features (map tiles, model downloads): a pure-Rust provider vs
   accepting `ring` (LightCraft pulls `ring` in through its TLS stack), or delegating downloads to
   the OS. See `security.md`.
10. Application id (reverse-DNS, used by packaging, desktop files and the settings path).
