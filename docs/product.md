# Product

## What Dalica is

Dalica is a fast, local-first desktop **photo organizer with simple editing**, for people with large
personal photo collections on their own disks. It shows every photo in the folders you choose, in
one continuous, scrollable "light table"; lets you star, caption, tag, album, name faces, place on a
map, fix and play with photos in one click; and makes slideshows, collages and exports — without importing anything into a proprietary library, without an account, and without
ever changing a photo's pixels unless you explicitly save.

Dalica re-creates the layout, workflows and behaviour of a classic, much-loved desktop photo
organizer as faithfully as we can (the reference is documented privately; see `AGENTS.md`), with a
modern, muted dark look shared with the Craft apps, written in pure Rust, native on Linux, Windows
and macOS.

## Who it is for

- People with tens of thousands of photos in folders on local, external or network disks who want
  to browse them instantly, organize them lightly, and share them.
- People who liked the classic organizer's simplicity: one window, folders on the left, every photo
  on the right, one-click fixes, a tray to gather photos, big obvious buttons for what to do next.
- Not for: raw development workflows (that's LightCraft), pixel editing with layers (PhotoCraft),
  or cloud photo libraries.

## Principles

1. **Your folders are the library.** No import step for files already on disk; no hidden copies;
   moving or renaming in Dalica is moving or renaming on disk.
2. **Originals are sacred.** Edits are recipes; pixels change only on an explicit Save, with the
   original kept and restorable (`data-model.md`). Captions, tags, places and date changes are
   also written into the file's metadata (on by default, can be turned off); that never touches
   pixels and keeps every other application's metadata.
3. **Your work is portable.** Everything you do is stored in small readable files next to your
   photos and in standard metadata; the central index is a disposable cache.
4. **Instant.** Thumbnails appear as fast as the disk allows; the UI never waits for the disk,
   the network or a dialog. 100 k photos scroll smoothly.
5. **One-click first.** The common thing is one click (Auto Fix, star, rotate, Export); depth is
   one panel away, never a wizard.
6. **Familiar to the letter.** When in doubt, do what the reference organizer did, in the same
   place, with the same keys — unless that loses data, depends on a discontinued online service, or
   is one of the dated features listed as out of scope below.
7. **Private by default.** No account, no telemetry, no network unless the user asks for something
   that needs it (map tiles, model download), and face data never leaves the machine.
8. **Never crash, never lose a photo.**

## Scope

Tiers used in [`parity.md`](parity.md): **P0** core (needed for a first useful release), **P1**
important parity, **P2** later / niche, **OOS** out of scope.

| Area | Tier | Notes |
|---|---|---|
| Watched folders, scanning, Folder Manager | P0 | |
| Library view: tree, light table, sections, selection, tray, status line, bottom actions | P0 | |
| Stars, captions, tags, hide, rotate | P0 | |
| Search + filters | P0 | |
| Edit view: Basic fixes, Tuning, Undo/Redo, Save/Revert | P0 | |
| Export | P0 | |
| Albums (incl. automatic albums) | P0 | |
| File operations (move, copy, rename, delete to trash, new folder) | P0 | |
| Import from memory cards, removable drives and folders | P1 | cameras/phones over PTP/MTP: P2, OS integration (below) |
| Keyboard shortcut editor with contexts | P1 | `menus-and-shortcuts.md` §2.0 |
| Effects tabs (27 effects) | P1 | |
| Slideshow, full-screen preview, photo viewer | P1 | |
| People (faces) | P1 | local models only |
| Places (geotagging, map pane) | P1 | OpenStreetMap-compatible tiles, opt-in |
| Collage | P1 | |
| Video playback/trim | P2 | pure-Rust decoders in the process (reserved decision 2, decided by the maintainer 2026-10-19; the spike's record is `video-decoders.md`). As built for 1.0 (M15): MP4, M4V, MOV and MKV are listed with their facts, poster frames, a playing clip in the edit view with its trim, Save Frame as Picture and Export Clip, clips in slideshows, Export and collages; H.264 in every build; HEVC (most iPhone clips) only behind a build feature that the released apps do not turn on yet, so those clips are tiles ("This video can't be played", Open With) until the maintainer answers the patent and `unsafe` questions at M15.R; a clip's AAC sound and slideshow music (MP3, Ogg Vorbis, FLAC, WAV) through a pure-Rust output per OS (the PulseAudio protocol on Linux, WASAPI on Windows, CoreAudio on macOS); a folder's own music is not built |
| Languages | P2 | English and Thai (M15.35; the Thai catalog is a draft awaiting a fluent reader's review at M15.R); every string of the window goes through the catalogs, and Options ▸ General ▸ Language switches without a restart |
| Accessibility | P2 | AccessKit for every widget and region, keyboard-only use, scaling to 150 % with the Large font (M15.14, M15.29); checked with a real screen reader at M15.R |
| Legacy sidecar compatibility | P1 | reads the previous organizer's sidecars where no `.dalica.json` exists; never writes them |
| OS integration: talking to other apps (drag photos out to other apps, files on the clipboard, Open With ▸, Open in External Editor, Locate on Disk with the file selected, default-viewer registration, camera import over MTP/PTP) | P2 | the last milestone (maintainer decision, 2026-10-09). As built for 1.0 (M15): Locate on Disk, Open With ▸, Open in External Editor and files and pictures on the clipboard on all three systems (never from automation); dragging photos out on X11, Windows and macOS, not on Wayland (Copy puts the files on the clipboard instead); Dalica as the default viewer set on Linux, registered on Windows (the system's Default apps then chooses), declared on macOS (the user chooses in Finder); cameras and phones over PTP/MTP on Linux, while on Windows and macOS the system holds them and they import as a drive or card when they offer one; one Dalica per user, a second launch handing its files to the running one. Windows and macOS are compiled and tested by CI; the by-hand checks on each system are M15.R's |
| Check for updates | — | deferred, not planned yet (maintainer decision, 2026-10-09); updates come from the release page |
| Upload/sync to any web service, sign-in, blogging, online print ordering, online editors | OOS | the reference's online features are not reproduced |
| Email, printing (print room, contact sheets, posters), movie maker, timeline view, web page export, gift folder, screensaver, set as desktop, backup, configurable action buttons | OOS | dropped 2026-10-09 as dated; people share by exporting |
| Red Eye and Text tools, novelty effects and Shift variants, legacy video formats (AVI, WMV, MPG, 3GP, MTS/M2TS), "delete everything on the card" | OOS | dropped 2026-10-09 as dated |
| Password-protected hidden folders | OOS | decided 2026-10-09: no (false sense of security) |
| RAW development beyond the embedded preview | OOS | open the file in a raw developer such as LightCraft instead (Open With, OS integration) |

## Non-goals

- A catalog database that owns your photos.
- Multi-user or cloud sync.
- Plugins (until 1.0).
- A web build (possible later; L0–L4 stay wasm-clean after 1.0).
