# Parity tracker

A living checklist of every behaviour Dalica must match: what we have and what is missing. Agents
and humans pick work from **[Top gaps](#top-gaps)**; whoever lands a feature updates its row in the
same commit.

- **Ids** are stable and never renumbered. Feature rows use area prefixes (`LIB`, `ORG`, `IMP`,
  `PPL`, `GEO`, `SRCH`, `FILE`, `EDIT`, `FX`, `VIEW`, `OUT`, `DATA`, `PREF`, `SYS`); menu rows use
  `MENU-<command id>`; shortcut rows use `KEY-<context>-<key>`. Where useful, a note points to the
  matching section of the private reference notes (`notes/`), e.g. "ref 02 §6.5".
- **Tier:** **P0** core · **P1** important parity · **P2** later / niche · **OOS** out of scope.
- **Status:** ✅ done (usable end to end) · 🟡 partial (note says what is missing) · ⬜ missing ·
  🚫 out of scope.
- **Evidence:** `cmd:<id>` (registered command), file paths, test names. `cargo xtask parity` checks
  that every cited `cmd:` id and path exists and refreshes the summary between the markers.
- A ✅ row is not proof of parity. Behaviour is measured by the workflow acceptance tests
  (`process.md` §5); if you find a ✅ row wrong, downgrade it to 🟡 with a note.
- Feature names and notes are in our own words.

## Summary

<!-- parity:summary -->
| Section | ✅ | 🟡 | ⬜ | 🚫 |
|---|---:|---:|---:|---:|
| A. Library view | 7 | 20 | 5 | 1 |
| B. Organizing | 0 | 4 | 38 | 0 |
| C. Editing | 0 | 0 | 21 | 4 |
| D. Viewing | 0 | 1 | 5 | 2 |
| E. Output | 0 | 0 | 2 | 9 |
| F. Data | 1 | 15 | 3 | 0 |
| G. Preferences and system | 0 | 4 | 8 | 0 |
| H. Menus | 0 | 3 | 4 | 0 |
| **All** | 8 | 47 | 86 | 16 |
<!-- /parity:summary -->

## Top gaps

1. Everything in M0–M5 (`roadmap.md`): the workspace, the window regions, scanning, the light
   table and selection.

## A. Library view (LIB)

| Id | Feature | Tier | Status | Evidence / notes |
|---|---|---|---|---|
| LIB-REGIONS | Window regions and sizes per `ui-layout.md` §2 | P0 | 🟡 | every region at its §2 size, registered as `region:<name>` and reported by `ui.inspect` → `regions`, resizable tree through the splitter (200 … W − 480, `ui.set` refuses widths outside): `crates/ui/src/panels/layout.rs`, `crates/ui/tests/layout.rs`, `crates/ui/tests/tree_status_pane.rs`; the toolbar's controls at their §3 sizes (disabled until their commands): `crates/ui/src/panels/toolbar.rs`, `crates/ui/tests/toolbar.rs`; the bottom area's controls at their §6 sizes (disabled until their commands or the tray): `crates/ui/src/panels/bottom.rs`, `crates/ui/tests/bottom.rs`; the tree pane frame, status line and side-pane frame (M3.8): `crates/ui/src/panels/folders.rs`, `crates/ui/src/panels/status.rs`, `crates/ui/src/panels/side_pane.rs`; the §11 responsiveness rules (M3.10): every control at 1400 × 900, 1100 × 700 and 900 × 600, pane open and closed, in full screen; the tree shrinks before the light table drops below 480, then the pane narrows and closes; narrow windows drop only the title, the filter caption and search width: `crates/ui/tests/layout.rs`; side-pane contents and persistence (M3.6) missing; ref 01 §2.1, 02 §2 |
| LIB-TREE-GROUPS | Tree groups Albums/People/Projects/Folders/Other/Hidden, collapsible, counts | P0 | 🟡 | every group of the tree model with its header (30, disclosure, "Folders (n)", `tree:group:<group>`), each shown only when non-empty; a header's click runs `cmd:view.groupCollapse`, which hides its rows and its light-table sections; collapsed groups after the last open one dock at the bottom of the pane while the list fits: `crates/ui/src/panels/folders.rs`, `crates/ui/src/panels/folders/model.rs`, `crates/ui/tests/tree.rs`; Albums and People stay empty until albums (M9) and people (M13) exist; search's magnifier glyph (M9) missing; ref 02 §6.2–6.3 |
| LIB-TREE-FLAT | Flat folder view with year dividers (default) | P0 | ✅ | the model (`crates/library/src/tree.rs`, `crates/library/tests/tree.rs`, M4.4); the pane: one 24-high row per folder (icon, name elided in the middle, count in `text_dim`), year dividers (18, the year in 11 pt with a rule) only when sorted by date across years, only the rows on screen laid out (no more rows visited a frame at 5 000 folders than at 50): `crates/ui/src/panels/folders.rs`, `crates/ui/tests/tree.rs` |
| LIB-TREE-TREE | Tree view, simplified tree | P1 | 🟡 | tree view: the disk hierarchy under each watched root, 16 per level with disclosure triangles (`tree:disclosure:<FolderId>`, and `tree:dir:<hash>` / `tree:disclosure:dir:<hash>` for directories that are no library folder), parents with their own count, ←/→ fold and unfold with the pane focused, folded rows keep their sections: `crates/library/src/tree.rs` (M4.4), `crates/ui/src/panels/folders.rs`, `crates/ui/tests/tree.rs`; the toolbar's Flat and Tree buttons read the view state; the Folders header counts library folders only, as flat mode's groups do together (M5.13); Simplified Tree missing; ref 02 §6.1 |
| LIB-TREE-SORT | Folder sort: date, recent changes, size, name, reverse | P0 | ✅ | `cmd:view.folderSort` from the view-options popup, whose check marks read the view state (`crates/ui/src/panels/toolbar.rs`); rows and light-table sections in the same order for every sort and its reverse in both modes, checked against an order worked out from the photos: `crates/library/src/tree.rs` (M4.4), `crates/ui/tests/tree.rs` |
| LIB-TREE-CLICK | Click scrolls light table to section and selects folder as a unit | P0 | ✅ | a row's click selects the folder as a unit (the selection model of `cmd:select.section`) and eases its section's header to the top in 0.2 s (`cmd:section.scrollTo`; again = the top of that section); with the pane focused ↑/↓ do the same for the previous or next folder and never move the light table's photo selection: `crates/ui/src/panels/folders.rs`, `crates/ui/src/keys.rs`, `crates/ui/tests/tree.rs`, `crates/ui/tests/light_table.rs`; ref 02 §6.5 |
| LIB-TREE-DND | Drop photos on folder (move/copy), album (add), person | P0 | ⬜ | |
| LIB-SPLIT | Resizable tree with persisted width | P0 | ✅ | `splitter:tree` drags the tree within 200 … W − 480 (W − pane − 480 beside an open side pane) (by id: `crates/ui/tests/tree_status_pane.rs`, `crates/ui/tests/layout.rs`); the width survives a restart through `settings.json` (`crates/ui/src/app.rs` test `the_view_state_survives_a_restart`) |
| LIB-LT-SECTIONS | Continuous light table of sections in tree order | P0 | 🟡 | one continuous view of the folders' sections in the tree's order (sort, reverse, collapsed groups), our own viewport with the position kept by section, laid out by prefix sums and binary search (cells visited per frame equal at 10 k and 100 k photos): `crates/ui/src/panels/light_table.rs`, `crates/ui/src/panels/light_table/layout.rs`, `crates/ui/tests/light_table.rs`; album and person sections missing (M9, M13); ref 02 §7.1 |
| LIB-LT-HEADER | Section header: icon, title, date/place, buttons, description, edited count + Save | P0 | 🟡 | the 96-high block: folder icon 32 (tooltip = full path), title, long date with weekday (" · place" when set), the 26 × 22 buttons Play, Collage │ Select Starred │ Save, Export (live only when their command is registered; Select Starred also needs a starred photo, Save an unsaved edit), the "Add a description" placeholder, the accent bar on the selected section, right-click = the section menu; Select Starred selects the section's starred photos (`select.starred`): `crates/ui/src/panels/light_table/header.rs`, `crates/ui/tests/section_header.rs`; the other buttons' commands, Locate on Disk, Properties on double-click, places, descriptions and their inline editing (M6) and the edited count (M7) arrive with their milestones; ref 02 §7.2 |
| LIB-LT-STICKY | Sticky section headers | P1 | ✅ | the header of the section scrolled through is pinned at the top (32 high, its first line: icon, title, edited count and Save) and pushed up by the next; the pinned line owns the header's ids: `crates/ui/src/panels/light_table/layout.rs`, `crates/ui/src/panels/light_table/header.rs`, `crates/ui/tests/light_table.rs`, `crates/ui/tests/section_header.rs` |
| LIB-LT-GRID | Justified square-cell grid, aspect-preserving thumbnails, shadow | P0 | ✅ | square cells 64–320 in justified columns (gap ≥ 12, margins 24, rows 22 apart, 24 before the next header), photos letterboxed: `crates/ui/src/panels/light_table/layout.rs`, `crates/ui/src/panels/light_table/cell.rs`, `crates/ui/tests/light_table.rs`; the photo's drop shadow (`black α90`, 2 down, blur 4) under every image: `crates/ui-kit/src/widgets/badges.rs`, `crates/ui-kit/tests/badges.rs` |
| LIB-LT-CAPTION | Thumbnail captions (None/Filename/Caption/Tags/Resolution/Date) | P1 | 🟡 | `cmd:view.caption`; one 10.5 pt `text_dim` line under the cell, middle-elided: Filename, Date (taken, else the file's), Resolution: `crates/ui/src/panels/light_table/cell.rs`, `crates/ui/tests/cells.rs`; Caption and Tags draw an empty line until the model has them (M6) |
| LIB-LT-BADGES | Badges: edited, geotag, held, star; video sprockets + duration | P0 | 🟡 | one painter for the row (edited · geotag · held · star, star rightmost, dropped from the left when narrow), the sprocket strip and the duration pill, every combination at 64/144/320 fingerprinted: `crates/ui-kit/src/widgets/badges.rs`, `crates/ui-kit/tests/badges.rs`; fed star and held from the model: `crates/ui/src/panels/light_table/cell.rs`, `crates/ui/tests/cells.rs`, `crates/ui/tests/library_window.rs`; edited, geotag and video data arrive with their milestones (M6, M8, M12); ref 02 §7.4 |
| LIB-LT-SCROLLBAR | Section-jump scrollbar (⇑ ▲ ▼ ⇓) with the speed-control thumb and position marker | P0 | 🟡 | ⇑ ▲ track ▼ ⇓ acting on press with auto-repeat, eased steps that add up, the speed thumb (dead zone, `top · u^exponent` from `settings.json`, stop and 150 ms spring-back on release), the position marker, and the keys PgUp/PgDn, Ctrl+↑/↓, Ctrl+Home/End (`cmd:scroll.pageUp` `cmd:scroll.pageDown` `cmd:scroll.prevSection` `cmd:scroll.nextSection` `cmd:scroll.top` `cmd:scroll.bottom`): `crates/ui/src/panels/light_table/scrollbar.rs`, `crates/ui/src/commands/scroll.rs`, `crates/ui/tests/scrollbar.rs`; the curve and top speed are a starting point to tune by feel at the M4 review (`tests/acceptance/m4.md`); ref 02 §7.1 |
| LIB-LT-INSTANT | Thumbnails never blank while scrolling: colour sketch, packed size ladder, upscaled fallback | P0 | 🟡 | the colour sketch (encode, decode, ≤ 26 bytes): `crates/thumbs/src/sketch.rs`, `crates/thumbs/tests/sketch.rs` (M4.1); the sketch in the index (M4.2); the packed cache (M4.3); the size ladder, scan-time 128/256 entries and sketches, best-available answers (covering, smaller scaled up, sketch, not scanned) and direction-aware prefetch: `crates/thumbs/src/ladder.rs`, `crates/engine/src/thumbs.rs`, `crates/thumbs/tests/ladder.rs`, `crates/engine/tests/thumbs.rs` (M4.8); drawing them on screen (covering size, smaller scaled up, the sketch as a mesh the same frame, the placeholder only for a photo never scanned; textures within ±1 screen and 600, capped uploads per frame, speed-gated requests, `placeholderCells` and friends in `ui.inspect`): `crates/ui/src/panels/thumbnails.rs`, `crates/ui/tests/never_blank.rs` (M4.12: a scanned 10 k library at the thumb's top speed draws no placeholder on any frame); at 100 k, the speed thumb held at its top speed through an index-only library draws every cell's sketch on every frame, no placeholder, within the frame budget in release: `tests/acceptance/m4_thumb.rs` (M4.18; a scanned library with files is measured at 10 k, as synthetic photo files stop at 20 k); ref 05 db3 tiers |
| LIB-LT-HIDDEN | Hidden pictures shown translucent when enabled; small pictures toggle | P1 | 🟡 | small pictures (< 64 px on both sides) are left out unless View ▸ Small Pictures (`cmd:view.smallPictures`): `crates/ui/src/panels/light_table.rs`, `crates/ui/tests/light_table.rs`, `crates/ui/tests/cells.rs`; the 40 % rule for hidden photos with `cmd:view.hiddenPictures`: `crates/ui/src/panels/light_table/cell.rs` (`opacity`), `crates/ui/tests/cells.rs`; the photos' hidden flag missing (M6) |
| LIB-SEL | Selection: click, Ctrl, Shift, rubber band, scoped to one section | P0 | 🟡 | click, Ctrl-click (toggles; in another section replaces), Shift-click (range from the anchor), right-click on an unselected photo selects it first, a click on empty space selects nothing, the rubber band from empty space (Ctrl adds) touching only the section it started in; a drag from a thumbnail starts no band (`cmd:select.set`, `cmd:select.section`): `crates/ui/src/selection.rs`, `crates/ui/src/panels/light_table/input.rs`, `crates/ui/tests/selection.rs`; held photos stay in the tray when the selection moves (LIB-TRAY-HOLD); a band held at the view's top or bottom edge scrolls it (faster the farther past the edge) and keeps selecting, never past its section's ends (`crates/ui/tests/band_autoscroll.rs`); dragging photos missing; ref 02 §7.5 |
| LIB-SEL-KEYS | Keyboard navigation and selection keys | P0 | 🟡 | arrows move (Shift extends from the anchor), Home / End go to the section's first / last photo, all stop at the section's ends and scroll what they reach into view (`cmd:select.left` … `cmd:select.extendLast`); Ctrl+A / Ctrl+I / Ctrl+D and Select Starred per section (`cmd:select.all`, `cmd:select.invert`, `cmd:select.none`, `cmd:select.starred`): `crates/ui/src/commands/select.rs`, `crates/ui/tests/selection.rs`; Ctrl+Home / End, Ctrl+↑ / ↓ and PgUp / PgDn scroll (`cmd:scroll.pageUp` … `cmd:scroll.bottom`, M4.11); Ctrl+H holds (`cmd:tray.hold`, M4.16); Enter (M7) missing |
| LIB-HOVER | Tooltip on hover | P1 | 🟡 | thumbnails: after 600 ms of rest the file name over date taken, pixel size and file size, or why the photo cannot be shown: `crates/ui/src/panels/light_table/cell.rs`, `crates/ui/tests/cells.rs`; Ctrl+Alt full-screen preview missing |
| LIB-PEEK | Ctrl+Alt full-screen peek with wheel stepping | P1 | ⬜ | |
| LIB-LOUPE | Loupe magnifier over thumbnails | P2 | ⬜ | |
| LIB-THUMBSIZE | Thumbnail-size slider, Ctrl+1/Ctrl+2, Ctrl+wheel | P0 | ✅ | the bottom bar's slider (64–320) and Ctrl+1 / Ctrl+2 (`cmd:view.thumbsSmall`, `cmd:view.thumbsNormal`) set the light table's cell size: `crates/ui/src/panels/bottom.rs`, `crates/ui/tests/bottom.rs`; Ctrl+wheel over the light table changes it, keeping the section at the top in view: `crates/ui/tests/light_table.rs`; the size survives a restart: `crates/ui/tests/view_state.rs` |
| LIB-STATUS | Status line texts (none / one / many / edit view) | P0 | 🟡 | from the Photo Tray's contents (held photos and the selection): "No selection", one photo (name, date and time, pixels, size), several photos or a section selected as a unit (count, date range or one day, size on disk), checked against the literal §6.1 strings at 1400 × 900 and 900 × 600; a long line drops the edit view's folder prefix, then middle-elides the file name, then cuts its end; transient messages for 4 s of the frame clock; `ui.state` → `status` the text as drawn: `crates/ui/src/status_text.rs`, `crates/ui/src/panels/status.rs`, `crates/ui/tests/status_line.rs`, `crates/ui/tests/tree_status_pane.rs`; the tags field ("Tags: lisbon (40), tram (6)") and the edit view's "Folder › … (12 of 48)" are tested formats, shown once photos carry tags (M6) and the edit view exists (M7); ref 02 §8.1 |
| LIB-TRAY | Photo Tray: token for folder, mini thumbnails, own selection | P0 | 🟡 | the tray mirrors the selection plus the held photos (held first, each once); a section selected as a unit is one token (cover and "Folder selected — 48 photos"); square centre-cropped mini thumbnails shrinking with the count (54 … 18, up to three rows, the rest counted as "+N"), centred in the box (`crates/ui/tests/tray_keys.rs`); its own selection by click, Ctrl-click and rubber band (`cmd:tray.select`); the tray menu (View and Edit · Hold · Remove from Tray · rotations · Locate on Disk · Properties, greyed until their commands): `crates/ui/src/tray.rs`, `crates/ui/src/panels/bottom/tray.rs`, `crates/ui/tests/tray.rs`; `ui.menu.tree {context: "tray"}` answers the tray menu's rows (`crates/ui/tests/dialogs.rs`, M5.14); dragging photos onto and out of the tray and the album token (M9) missing; ref 02 §8.2 |
| LIB-TRAY-HOLD | Hold / Clear / Add to Album ▾ | P0 | 🟡 | Hold (Ctrl+H, the held badge; `cmd:tray.hold`) keeps photos while the selection moves; Clear removes the tray-selected photos, or all of them, asking "Clear all photos from the tray?" only when photos are held, through the dialog host (`dialog:clearTray`; `cmd:tray.clear`, `needs_confirm` over `engine.execute`, `confirm: true` to go ahead; `crates/ui/tests/dialogs.rs`, M5.14): `crates/ui/src/commands/tray.rs`, `crates/ui/tests/tray.rs`; Add to Album ▾ is drawn, disabled until albums (M9) |
| LIB-STARROT | Star and rotate buttons acting on the tray | P0 | 🟡 | the star button stars every photo in the tray in one undo step, or unstars them when all are starred (`cmd:photo.star.toggle`); the rotate buttons act on the tray once their commands exist; their shortcuts and menu items act on the same photos (`crates/ui/src/keys.rs` `TRAY_COMMANDS`): `crates/ui/src/panels/bottom.rs`, `crates/ui/tests/tray.rs`, `crates/ui/tests/tray_keys.rs`; rotation missing (M6) |
| LIB-ACTIONS | Action buttons (Export, Collage, Slideshow) with overflow | P0 | 🟡 | the buttons, and their More… menu below 1100 wide: `crates/ui/src/panels/bottom.rs`, `crates/ui/tests/bottom.rs`; disabled until export, collage and slideshow exist |
| LIB-CONFBUTTONS | Configure Buttons dialog | OOS | 🚫 | dropped 2026-10-09 (dated feature); the button set is fixed |
| LIB-PANES | Side pane container with People/Places/Tags/Properties toggles | P1 | 🟡 | the frame: 30 pt header with « (large width), title, × by id; pushes the light table; a narrowing window shrinks the tree to 200 first, then takes the large width away, then closes it: `crates/ui/src/panels/side_pane.rs`, `crates/ui/tests/tree_status_pane.rs`, `crates/ui/tests/layout.rs`; the bottom-bar toggles (M3.9), the slide animation and the pane contents (M6, M13) missing |
| LIB-PROPS | Properties pane | P1 | ⬜ | |
| LIB-ROOMS | Room tab strip (Import, Collage) | P1 | ⬜ | ref 01 §2.3 |
| LIB-EMPTY | Empty states | P0 | 🟡 | no watched folders: "No pictures yet" with Add a Folder… (the folder dialog) and Import… (disabled until import): `crates/ui/src/panels/light_table.rs`, `crates/ui/tests/light_table.rs`; search with no results missing (M9) |

## B. Organizing (ORG, IMP, PPL, GEO, SRCH, FILE)

| Id | Feature | Tier | Status | Evidence / notes |
|---|---|---|---|---|
| ORG-FIRSTRUN | First-run panel and progressive first scan | P0 | 🟡 | ref 04 §1; engine: scans are jobs whose folders land a batch per frame with their thumbnails queued while the walk goes on, with progress and cancel (`crates/engine/src/scanning.rs`, `crates/engine/tests/progressive_scan.rs`, `crates/engine/tests/jobs.rs`), and `Session::is_new_library` answers the panel's question; missing: the panel (M5.18), the activity indicator and status text (M5.19) |
| ORG-FOLDERMGR | Folder Manager (Scan Always / Once / Remove, faces toggle, watched list) | P0 | 🟡 | engine and model (M5.12): modes and the faces flag per folder, inherited unless overridden, Remove leaves the library without touching a file and undoes with its mode, Scan Once frozen after its first scan, `folders.json` v2 reading v1, the drive and Remove Folder confirmations over automation: `cmd:folders.set` `cmd:folder.remove` `cmd:folders.browse` `cmd:folders.list`, `crates/scan/src/rules.rs`, `crates/engine/src/folders.rs`, `crates/scan/tests/rules.rs`, `crates/engine/tests/folders.rs`; the dialog a stub on the dialog host (M5.14): Tools ▸ Folder Manager… (`cmd:folders.manager`) opens `dialog:folderManager` listing the watched folders, Help / Cancel / OK, OK applies nothing yet (`crates/ui/src/dialogs/folder_manager.rs`, `crates/ui/tests/dialogs.rs`); the tree, modes, faces toggle and File ▸ Add Folder… opening it (M5.18) missing; confirmations answer `disabled` until the dialog host routes `needs_confirm`; ref 04 §2 |
| ORG-WATCH | Watching for changes, offline volumes | P0 | 🟡 | `dalica-scan` side (M5.11): dirty folders from `notify` 8 events, debounced, coalesced and capped, with a pure-Rust polling fallback per root (network shares, watch failures, lost roots); incremental verification of the dirty folders (`watch::rescan`, `diff_rescan`) equal to the full diff restricted to them; folder renames and moves keep folder and photo ids; nothing removed under a root that cannot be walked; a full-verification `Schedule`: `crates/scan/src/watch.rs`, `crates/scan/tests/watch.rs`, `crates/scan/tests/incremental.rs`. Offline volumes in the engine (M5.13): roots checked off the owner thread (every 10 s and after scans), folders under an offline root keep their photos, rows and thumbnails, edits are journalled at once and their sidecar writes wait (no folder created, `pendingWrites` counts them, `pending-writes.json` keeps them across a restart) and land when the root returns: `crates/engine/src/offline.rs`, `crates/engine/tests/offline.rs`, `crates/store/tests/offline.rs`, `crates/library/tests/tree.rs`; wiring the watcher into the session (M5.17) and the dimmed rows with their "offline" marker (M5.19) missing |
| ORG-ALBUM-NEW | New album + Album Properties | P0 | ⬜ | |
| ORG-ALBUM-EDIT | Add/remove/reorder/cover/delete album | P0 | ⬜ | |
| ORG-ALBUM-AUTO | Starred Photos, Recently Updated, Search results albums | P0 | ⬜ | |
| ORG-STAR | Star toggle everywhere | P0 | 🟡 | library view only: Ctrl+8 / F4 and the bottom-bar star button on the tray's photos (the held ones and the selection) in one undo step (`cmd:photo.star.toggle`, `crates/ui/tests/library_window.rs`, `crates/ui/tests/tray.rs`, `crates/ui/tests/tray_keys.rs`); menus, edit view missing |
| ORG-TAGS | Tags pane: add, list with partial counts, remove | P1 | ⬜ | |
| ORG-QUICKTAGS | Quick tags + configure dialog | P2 | ⬜ | |
| ORG-CAPTION | Captions (edit view field, library inline) | P0 | ⬜ | |
| ORG-HIDE | Hide/unhide photos and folders | P1 | ⬜ | |
| ORG-FOLDERPROPS | Folder Properties dialog | P0 | ⬜ | |
| ORG-SORT-PHOTOS | Photo order in sections, manual order persisted | P1 | ⬜ | |
| ORG-ADJDATE | Adjust Date and Time dialog | P1 | ⬜ | |
| IMP-ROOM | Import room: source, tray, preview, destination strip | P1 | ⬜ | ref 04 §3 |
| IMP-DEVICES | Memory card and removable-drive detection | P1 | ⬜ | |
| IMP-MTP | Cameras/phones over MTP/PTP | P2 | ⬜ | OS integration (last milestone) |
| IMP-DUP | Exclude duplicates | P1 | ⬜ | |
| IMP-NAMING | Folder name options incl. date taken per day | P1 | ⬜ | |
| IMP-AFTER | After-copying card options (leave, delete copied files) with confirmation; no "delete everything" | P1 | ⬜ | |
| PPL-DETECT | Face detection + recognition (local models) | P1 | ⬜ | |
| PPL-UNNAMED | Unnamed view with clusters, naming, ignore | P1 | ⬜ | ref 04 §6.2 |
| PPL-SUGGEST | Suggestions with confirm/reject | P1 | ⬜ | |
| PPL-PANE | People pane, add person manually | P1 | ⬜ | |
| PPL-MANAGER | People Manager | P2 | ⬜ | |
| PPL-XMP | Write people names to photos (XMP regions) | P2 | ⬜ | |
| GEO-PANE | Places pane with map, drag to geotag, search | P1 | ⬜ | ref 04 §7 |
| GEO-WRITE | EXIF GPS write/clear | P1 | ⬜ | |
| SRCH-LIVE | Live search with operators and autocomplete | P0 | ⬜ | ref 04 §8 |
| SRCH-FILTERS | Filter toggles + age slider | P0 | ⬜ | |
| SRCH-BANNER | Search mode banner + Show All | P0 | ⬜ | |
| SRCH-SAVE | Save search as album, show tag as album | P2 | ⬜ | |
| SRCH-COLOR | Colour search | P2 | ⬜ | |
| SRCH-DUP | Find duplicates | P2 | ⬜ | |
| FILE-MOVE | Move/copy photos with collision dialog | P0 | ⬜ | ref 04 §9 |
| FILE-NEWFOLDER | Move to new folder, split folder | P1 | ⬜ | |
| FILE-RENAME | Rename / batch rename dialog | P0 | ⬜ | |
| FILE-DELETE | Delete to trash with confirmations | P0 | ⬜ | |
| FILE-FOLDEROPS | Move / rename / delete folder | P1 | ⬜ | |
| FILE-COPYPATH | Copy Path | P1 | ⬜ | |
| FILE-LOCATE | Locate on disk (file selected), Open With ▸, Open in External Editor | P2 | ⬜ | OS integration (last milestone) |
| FILE-DRAGOUT | Drag photos out to other apps; files and image on the system clipboard | P2 | ⬜ | OS integration (last milestone) |

## C. Editing (EDIT, FX)

| Id | Feature | Tier | Status | Evidence / notes |
|---|---|---|---|---|
| EDIT-VIEW | Edit view layout, top bar, filmstrip, caption bar, status | P0 | ⬜ | ref 03 §3 |
| EDIT-ZOOM | Zoom slider mapping, Fit/1:1, click-hold zoom, pan | P0 | ⬜ | ref 03 §3.5 |
| EDIT-COMPARE | A │ B and A │ A compare with dialogs | P2 | ⬜ | ref 03 §4 |
| EDIT-TABS | Five icon tabs | P0 | ⬜ | |
| EDIT-UNDO | Undo/Redo with action names, persistent, linear | P0 | ⬜ | |
| EDIT-HIST | Histogram & camera info box | P1 | ⬜ | |
| EDIT-CROP | Crop panel: ratios, custom ratios, suggestions, rotate, preview | P0 | ⬜ | ref 03 §6.1 |
| EDIT-STRAIGHTEN | Straighten with grid and floating bar | P0 | ⬜ | |
| EDIT-REDEYE | Red eye auto + manual | OOS | 🚫 | dropped 2026-10-09 (dated feature); legacy `redeye` operations kept as `legacy:` entries |
| EDIT-AUTOFIX | Auto Fix, Auto Contrast, Auto Color | P0 | ⬜ | |
| EDIT-RETOUCH | Retouch patches | P1 | ⬜ | |
| EDIT-TEXT | Text tool with rotate/scale handle, colours, opacity | OOS | 🚫 | dropped 2026-10-09 (dated feature) |
| EDIT-FILL | Fill Light slider | P0 | ⬜ | |
| EDIT-TUNING | Tuning tab incl. auto wands and neutral picker | P0 | ⬜ | ref 03 §7 |
| EDIT-SAVE | Save / Undo Save / Revert / Save As / Save a Copy | P0 | ⬜ | data-model §6 |
| EDIT-RECIPE-COPY | Copy/Paste All Edits | P1 | ⬜ | |
| EDIT-BATCH | Batch Edit submenu | P1 | ⬜ | |
| EDIT-FLIP | Flip H/V | P1 | ⬜ | |
| FX-TILES | Effect tile grid with live previews, 1-click badge | P1 | ⬜ | ref 03 §8 |
| FX-SHIFT | Shift variants | OOS | 🚫 | dropped 2026-10-09 (dated feature) |
| FX-T3 | Tab 3 effects (12) | P1 | ⬜ | ref 03 §9 |
| FX-T4 | Tab 4 effects (7) | P2 | ⬜ | ref 03 §10 |
| FX-T5 | Tab 5 effects (8) | P2 | ⬜ | ref 03 §10 |
| EDIT-VIDEO | Video trim, frame save, export clip | P2 | ⬜ | ref 06 §3 |
| FX-NOVELTY | Novelty effects (Toy Camera, Sixties, Heat Map, Night Vision, Cross Process, Posterize, Pencil Sketch, Neon, Comic Book, Instant Print) | OOS | 🚫 | dropped 2026-10-09 (dated feature) |

## D. Viewing (VIEW)

| Id | Feature | Tier | Status | Evidence / notes |
|---|---|---|---|---|
| VIEW-SLIDESHOW | Slideshow with control bar, transitions, display time, captions | P1 | ⬜ | ref 06 §2 |
| VIEW-MUSIC | Slideshow music, loop | P2 | ⬜ | |
| VIEW-VIEWER | Standalone photo viewer | P1 | ⬜ | ref 06 §1 |
| VIEW-VIEWER-ASSOC | Default-viewer registration | P2 | ⬜ | OS integration (last milestone) |
| VIEW-TIMELINE | Timeline | OOS | 🚫 | dropped 2026-10-09 (dated feature) |
| VIEW-VIDEO | Video playback | P2 | ⬜ | |
| VIEW-FULLSCREEN | F11 full screen | P1 | 🟡 | F11 and View → Full Screen toggle the view state's `fullScreen`; the window asks the OS to follow it once per change and adopts a full screen the OS ends or starts by itself (`FullscreenSync`, tested without a window on every OS): `apps/dalica/src/window.rs`; in full screen every region and control keeps its place for the screen's size: `crates/ui/src/menus.rs`, `crates/ui/tests/layout.rs`; not yet seen on a live window (the agent's window drew no frames), so the manual check of M3.11 confirms it |
| VIEW-SCREENSAVER | Screensaver mode, Set as Desktop | OOS | 🚫 | dropped 2026-10-09 (dated feature) |

## E. Output (OUT)

| Id | Feature | Tier | Status | Evidence / notes |
|---|---|---|---|---|
| OUT-EXPORT | Export dialog: size, quality, watermark, numbering, videos | P0 | ⬜ | ref 07 §4.1 |
| OUT-EMAIL | Email via default app / save attachments | OOS | 🚫 | dropped 2026-10-09 (dated feature); share by exporting |
| OUT-PRINT | Print room: layouts, fit, borders/text, review, copies | OOS | 🚫 | dropped 2026-10-09 (dated feature) |
| OUT-CONTACT | Printed contact sheet | OOS | 🚫 | dropped 2026-10-09 (dated feature); the collage room keeps its Contact Sheet style |
| OUT-COLLAGE | Collage room: 6 styles, settings, clips, canvas, output | P1 | ⬜ | ref 07 §1 |
| OUT-MOVIE | Movie maker, face movie | OOS | 🚫 | dropped 2026-10-09 (dated feature) |
| OUT-WEB | Export as web page | OOS | 🚫 | dropped 2026-10-09 (dated feature) |
| OUT-GIFT | Gift folder | OOS | 🚫 | dropped 2026-10-09 (dated feature) |
| OUT-POSTER | Poster | OOS | 🚫 | dropped 2026-10-09 (dated feature) |
| OUT-BACKUP | Backup sets | OOS | 🚫 | dropped 2026-10-09 (dated feature) |
| OUT-UPLOAD | Upload / sync to a web service, blogging, print ordering | OOS | 🚫 | online services |

## F. Data (DATA)

| Id | Feature | Tier | Status | Evidence / notes |
|---|---|---|---|---|
| DATA-SIDECAR | `.dalica.json` read/write per `data-model.md` §3 | P0 | 🟡 | `star` only (v1); unknown keys kept, debounced atomic writes, damaged files set aside: `crates/sidecar/src/lib.rs`, `crates/store/tests/sidecars.rs`; other fields arrive with their features |
| DATA-INDEX | Library index journal + snapshot, lock, rebuild | P0 | ✅ | `cmd:library.rebuild`; `crates/store/src/journal.rs`, `crates/fs/src/lock.rs`; measured by `tests/acceptance/m1_headless.rs` |
| DATA-META | XMP/IPTC/EXIF write-back with merge | P0 | ⬜ | |
| DATA-ORIGINALS | `.dalica-originals` backups | P0 | ⬜ | |
| DATA-LEGACY | Legacy sidecars read as a fallback when a folder has no `.dalica.json`; first change writes `.dalica.json`; legacy files never written | P1 | ⬜ | ref 05 §3; `data-model.md` §9 |
| DATA-TESTS | The eleven data-safety regression tests (`data-model.md` §10) | P0 | 🟡 | §10.1 for stars, §10.5, §10.6, §10.7: `crates/store/tests/sidecars.rs`, `crates/fs/tests/lock.rs`, `crates/fs/tests/walk.rs`; the rest arrive with their features |
| DATA-FMT-JPEG | JPEG: baseline, progressive, CMYK/YCCK, APP2 profiles, EXIF orientation and thumbnail | P0 | 🟡 | `crates/image/src/formats/jpeg.rs`, `crates/image/tests/decode.rs`, `crates/image/tests/color.rs`; 12-bit and DCT-scaled decodes not measured; arithmetic coding answers "Damaged file" |
| DATA-FMT-PNG | PNG: 1–16-bit, palette, transparency, interlaced, `iCCP` | P0 | 🟡 | `crates/image/src/formats/png.rs`, `crates/image/tests/color.rs`; 8/16-bit RGB and grey+alpha and `iCCP` measured; palette, low bit depths and interlacing go through `png` but are not measured until the PngSuite corpus (M5.15) |
| DATA-FMT-COLOR | Colour management: ICC v2/v4 to sRGB, untagged = sRGB, unusable profile → sRGB, naive CMYK | P0 | 🟡 | `crates/image/src/color.rs`, `crates/image/tests/color.rs` (Display P3, Adobe RGB, ProPhoto; v2 and v4); View → Use Color Management (`view.colorManagement`) arrives with M5.16 |
| DATA-FMT-TIFF | TIFF: 8/16-bit, float, grey/RGB/CMYK/palette, LZW/Deflate/PackBits/JPEG, first page | P0 | 🟡 | `crates/image/src/formats/tiff.rs`, `crates/image/tests/tiff.rs`: every `scenes` depth, colour model, compression and byte order, JPEG strips, tiles, planar, 1–4-bit, alpha, ICC, orientation 1–8 and multi-page measured on synthetic files; the thumbnail-IFD fast path is `formats::tiff::thumbnail`, not yet called by `decode_thumbnail`; real-world files wait for the corpus (M5.15) |
| DATA-FMT-WEBP | WebP: lossy, lossless, alpha, first frame | P1 | 🟡 | `crates/image/src/formats/webp.rs`, `crates/image/tests/webp_gif_bmp.rs`; lossless (alpha, `EXIF` orientation 1–8 and thumbnail, `ICCP`) and an animation's first frame measured exactly; lossy only from a hand-built flat VP8 frame (with and without `ALPH`), real lossy photos wait for the corpus (M5.15) |
| DATA-FMT-GIF | GIF: first frame | P1 | 🟡 | `crates/image/src/formats/gif.rs`, `crates/image/tests/webp_gif_bmp.rs`; one and several frames and a frame smaller than its screen measured exactly; interlaced frames and transparent colour indices go through `gif` but are not measured until the corpus (M5.15) |
| DATA-FMT-BMP | BMP | P1 | 🟡 | `crates/image/src/formats/bmp.rs`, `crates/image/tests/webp_gif_bmp.rs`; 24-bit, 8-bit grey and palette, and a top-down 32-bit `BITMAPV5HEADER` with an alpha mask and an embedded profile measured exactly; RLE, 1/4/16-bit and OS/2 headers go through `image` but are not measured; JPEG/PNG-compressed and CMYK bitmaps answer "This kind of BMP isn't supported" |
| DATA-FMT-JXL | JPEG XL | P2 | 🟡 | `crates/image/src/formats/jxl.rs` through `jxl-oxide`, `crates/image/tests/jxl.rs`: lossless grey and RGB at 8 and 16 bits, alpha, bare codestreams and the box container, the eight orientations, an enumerated Display P3 encoding through the colour stage, the allocation limit; lossy (XYB) files, embedded ICC profiles and animations go through the decoder but are not measured until the corpus (M5.15); CMYK answers "This kind of JPEG XL file isn't supported" |
| DATA-FMT-PSD | PSD/PSB merged composite | P1 | 🟡 | our own reader, `crates/image/src/formats/psd.rs`, `crates/image/tests/psd.rs`: 8/16/32-bit grey, RGB, CMYK and indexed, bitmap and duotone, raw and PackBits, PSD and PSB, the thumbnail (1036) and ICC (1039) resources; extra channels are not shown; ZIP composites, Lab and multichannel answer "This kind of PSD isn't supported"; files saved by Photoshop itself are not measured until the corpus (M5.15) |
| DATA-FMT-RAW-TIFF | TIFF-structured raws (DNG, CR2, NEF, ARW, PEF, ORF, RW2, SRW): embedded preview | P0 | 🟡 | recognised by content; the largest baseline/progressive JPEG preview found by our own bounded walk (IFD chain, SubIFDs, EXIF IFD, JPEG strips, RW2's JPEG tag, Nikon/Olympus/Pentax maker-note pointers), orientation from IFD0, Adobe RGB/ProPhoto-tagged previews converted, the generic RAW tile without a preview, uncompressed IFD1 strip thumbnails read and decoded: `crates/image/src/formats/raw/tiff_based.rs`, `crates/image/src/ifd.rs`, `crates/image/src/exif.rs`, `crates/image/tests/raw_tiff.rs` (synthetic files of each kind); real camera files not measured until the raw corpus (M5.15) |
| DATA-FMT-RAW-OTHER | CR3, RAF, CRW, MRW, X3F: embedded preview | P1 | 🟡 | largest embedded JPEG shown, the next smaller one as the thumbnail, orientation from the container: `crates/image/src/formats/raw/cr3.rs`, `crates/image/src/formats/raw/raf.rs`, `crates/image/src/formats/raw/crw.rs`, `crates/image/src/formats/raw/mrw.rs`, `crates/image/src/formats/raw/x3f.rs`, `crates/image/tests/raw_containers.rs` (synthetic containers around `scenes` JPEGs); real camera files are not measured until the corpus (M5.15) |
| DATA-FMT-HEIF | HEIF/HEIC behind the `heif` feature | P1 | 🟡 | heic-rs 0.1.1 behind the non-default `heif` feature (`crates/image/src/formats/heif.rs`): single pictures and grids, `irot`/`imir`/`clap`, 8–12-bit, alpha, ICC, EXIF; sequences, overlays and 4:2:2/4:4:4 answer "unsupported"; without the feature the tile says "HEIC support isn't included in this build" (`crates/image/tests/heif.rs`, synthetic lossless files); real iPhone files are not measured until the corpus (M5.15); whether release builds enable it is decided with the release pipeline (M6) |
| DATA-FMT-AVIF | AVIF recognised and listed as a format tile, never decoded | P2 | 🟡 | recognised by its major brand, or by an AV1 compatible brand or primary item behind a `mif1` major brand, and answers "AVIF files can't be displayed" (`crates/image/src/formats/heif.rs`, `crates/image/tests/heif.rs`); the tile drawing itself is not measured here |

## G. Preferences (PREF) and system (SYS)

| Id | Feature | Tier | Status | Evidence / notes |
|---|---|---|---|---|
| PREF-DIALOG | Options dialog with tabs, OK/Cancel/Apply | P1 | ⬜ | ref 08 §5 |
| PREF-GENERAL / FILETYPES / SLIDESHOW / NETWORK / PEOPLE / VIEWER / EXPORT | each tab's options | P1 | ⬜ | `menus-and-shortcuts.md` §5 |
| PREF-KEYMAP | Keyboard contexts and the shortcut editor (search, record, conflicts, reset) | P1 | ⬜ | `menus-and-shortcuts.md` §2.0 |
| SYS-UPDATES | Check for updates | — | ⬜ | deferred, not planned yet (2026-10-09) |
| SYS-COMMANDS | Command registry, menu catalog, UI_COMMANDS | P0 | 🟡 | the registry (`crates/engine/src/registry.rs`); the catalog of every item of `menus-and-shortcuts.md` §1 (`crates/ui/src/menu_catalog.rs`) and the menu model, `UI_COMMANDS` and menus bounded by the window (`crates/ui/src/menus.rs`), measured by `crates/ui/tests/menus.rs`; the in-window menu row (`crates/ui/src/titlebar.rs`) and the macOS menu bar generated from the same model (`apps/dalica/src/native_menu.rs`, the same items checked on every OS); missing: check marks fed by the view state (M4; the thumbnail-size radio group already answers) |
| SYS-WINDOW | Title bar, window frame and window state | P0 | 🟡 | the menu row as title bar on Windows and Linux (app mark, menus, centred title, 46 × 24 caption buttons, drag, double-click maximize, resize edges), the system-title-bar setting, the macOS strip and menu bar: `crates/ui/src/titlebar.rs`, `apps/dalica/src/frame.rs`, `apps/dalica/src/native_menu.rs`; window position, size and maximized state and the view state (tree width, side pane) restored from `settings.json` (`crates/engine/src/settings.rs`, `crates/ui/src/app.rs` test `the_view_state_survives_a_restart`); File ▸ Exit is the UI command `app.quit` (Ctrl+Q; it closes the window by the Close button's path: `crates/ui/src/commands/view.rs`, `crates/ui/tests/view_state.rs`); missing: the Options checkbox for the setting (PREF-GENERAL), the macOS build checked only by CI |
| SYS-CONTROL | Control channel + widget registry + screenshots | P0 | 🟡 | `crates/ui/src/control/` (every M2 method as pure handlers over the app and the headless driver: `engine.execute`, `engine.commands`, `ui.state`/`ui.set`, `ui.inspect`, `ui.widgets`, click/drag/hover/pointer/key/type/scroll/resize, `ui.screenshot` with §5.7 write roots, `ui.settle`/`ui.wait`, the file-picker stub); the transport in `apps/dalica/src/control/` (`--control [PORT]` on 127.0.0.1 only, the 0600 token file, `auth` in constant time, 1 MiB lines, 16 connections, 30 s idle, 60 s deadline, `app.quit`; requests answered from the window's logic tick or `--headless`), measured by `apps/dalica/tests/control_headless.rs`, its security rules (wrong token, no `auth`, oversized, non-JSON and HTTP lines, the 17th connection, token file modes, loopback only) by `apps/dalica/tests/control_security.rs`; `ui.menu.tree` / `ui.menu.invoke` in `crates/ui/src/control/menu.rs` (`crates/ui/tests/menus.rs`); the dialog host and `ui.dialog.confirm` / `ui.dialog.cancel` (dialogs as data in the view state, Esc / Enter, the window under a dialog takes no input; `ui.inspect` → `dialog`, `popup`, `tooltip`, `animations`, `scrollViews`, `jobs`) in `crates/ui/src/dialogs/` (`crates/ui/tests/dialogs.rs`), `jobs.list` / `jobs.wait` / `jobs.cancel` over the job registry, never blocking a frame (`crates/ui/tests/jobs_control.rs`), `ui.screenshot {region: <name>}` (M5.14); missing: `batch`, `library.inspect`, the long commands' replies waiting for their job (§5.3), the headless capture of a window (`ui.screenshot {headless: true}` in the window) |
| SYS-HEADLESS | Headless snapshots | P0 | 🟡 | `crates/ui/src/headless.rs` (click, drag, hover, scroll, raw pointer, keys, text, settle, wait) and `crates/ui/src/perf.rs` (perf counters, `DALICA_PROFILE=1`); `dalica-cli snapshot`; `dalica --headless --control` (the same handlers over TCP, no window); missing: the control-protocol script format in `snapshot` |
| SYS-MCP | MCP server | P1 | ⬜ | |
| SYS-I18N | Localisation | P2 | ⬜ | |
| SYS-A11Y | Accessibility (AccessKit) | P2 | ⬜ | |
| SYS-PACKAGING | Packages for Linux, Windows, macOS | P1 | ⬜ | |

## H. Menus (MENU)

One row per item in [`menus-and-shortcuts.md`](menus-and-shortcuts.md) §1. A menu item is ✅ when
its command exists, is wired to the menu catalog, and its behaviour row above is ✅.

| Menu | Ids | Status |
|---|---|---|
| File | `album.new` `folders.manager` `library.openFile` `import.open` `photo.openExternal` `photo.moveToNewFolder` `photo.rename` `photo.save` `photo.revert` `photo.saveAs` `photo.saveCopy` `output.export` `photo.locate` `photo.delete` `app.quit` | 🟡 |
| Edit | `edit.undo` `edit.redo` `photo.cut` `photo.copy` `photo.paste` `edit.copyRecipe` `edit.pasteRecipe` `select.all` `select.starred` `select.invert` `select.none` | 🟡 |
| View | `view.library` `view.thumbsSmall` `view.thumbsNormal` `view.edit` `pane.properties` `pane.tags` `pane.people` `pane.places` `pane.toggle` `edit.panelToggle` `view.slideshow` `view.smallPictures` `view.hiddenPictures` `view.colorManagement` `view.caption` `view.folderView` `window.fullscreen` | 🟡 |
| Folder / Album | `section.properties` `section.slideshow` `section.refreshThumbs` `section.sort` `folder.hide` `folder.unhide` `folder.locate` `folder.remove` `folder.move` `folder.delete` `album.delete` | ⬜ |
| Picture | `view.edit` `edit.undoAll` `photo.hide` `photo.unhide` `faces.reset` `pane.properties` `photo.star.toggle` `photo.caption.edit` `photo.rotate.cw` `photo.rotate.ccw` `edit.flipH` `edit.flipV` | ⬜ |
| Create | `create.collage` | ⬜ |
| Tools | `folders.manager` `people.manager` `viewer.settings` `photo.adjustDate` `geo.clear` `geo.copy` `geo.paste` `search.duplicates` `search.color` `search.saveAsAlbum` `search.tagAsAlbum` `faces.writeXmp` `app.preferences` | ⬜ |

## I. Keyboard shortcuts (KEY)

Every shortcut in [`menus-and-shortcuts.md`](menus-and-shortcuts.md) §2 and
[`editing.md`](editing.md) §12 is a row here once the keymap exists (`cargo xtask parity` will list
the keymap's bindings and fail when a documented key has no binding). Until then: ⬜ for all.
