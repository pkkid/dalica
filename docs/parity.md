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
| A. Library view | 0 | 2 | 30 | 1 |
| B. Organizing | 0 | 1 | 41 | 0 |
| C. Editing | 0 | 0 | 21 | 4 |
| D. Viewing | 0 | 0 | 6 | 2 |
| E. Output | 0 | 0 | 2 | 9 |
| F. Data | 1 | 2 | 3 | 0 |
| G. Preferences and system | 0 | 3 | 8 | 0 |
| H. Menus | 0 | 0 | 7 | 0 |
| **All** | 1 | 8 | 118 | 16 |
<!-- /parity:summary -->

## Top gaps

1. Everything in M0–M5 (`roadmap.md`): the workspace, the window regions, scanning, the light
   table and selection.

## A. Library view (LIB)

| Id | Feature | Tier | Status | Evidence / notes |
|---|---|---|---|---|
| LIB-REGIONS | Window regions and sizes per `ui-layout.md` §2 | P0 | 🟡 | every region at its §2 size, registered as `region:<name>`, resizable tree through the splitter: `crates/ui/src/panels/layout.rs`, `crates/ui/tests/layout.rs`; toolbar, side-pane and bottom-area contents, the §11 responsiveness rules and persistence missing; ref 01 §2.1, 02 §2 |
| LIB-TREE-GROUPS | Tree groups Albums/People/Projects/Folders/Other/Hidden, collapsible, counts | P0 | ⬜ | ref 02 §6.2–6.3 |
| LIB-TREE-FLAT | Flat folder view with year dividers (default) | P0 | ⬜ | |
| LIB-TREE-TREE | Tree view, simplified tree | P1 | ⬜ | |
| LIB-TREE-SORT | Folder sort: date, recent changes, size, name, reverse | P0 | ⬜ | |
| LIB-TREE-CLICK | Click scrolls light table to section and selects folder as a unit | P0 | ⬜ | ref 02 §6.5 |
| LIB-TREE-DND | Drop photos on folder (move/copy), album (add), person | P0 | ⬜ | |
| LIB-SPLIT | Resizable tree with persisted width | P0 | ⬜ | |
| LIB-LT-SECTIONS | Continuous light table of sections in tree order | P0 | ⬜ | ref 02 §7.1 |
| LIB-LT-HEADER | Section header: icon, title, date/place, buttons, description, edited count + Save | P0 | ⬜ | ref 02 §7.2 |
| LIB-LT-STICKY | Sticky section headers | P1 | ⬜ | |
| LIB-LT-GRID | Justified square-cell grid, aspect-preserving thumbnails, shadow | P0 | ⬜ | |
| LIB-LT-CAPTION | Thumbnail captions (None/Filename/Caption/Tags/Resolution/Date) | P1 | ⬜ | |
| LIB-LT-BADGES | Badges: edited, geotag, held, star; video sprockets + duration | P0 | 🟡 | star badge only: `crates/ui-kit/src/widgets.rs`, `crates/ui/tests/library_window.rs`; edited, geotag, held badges and video marks missing; ref 02 §7.4 |
| LIB-LT-SCROLLBAR | Section-jump scrollbar (⇑ ▲ ▼ ⇓) with the speed-control thumb and position marker | P0 | ⬜ | ref 02 §7.1 |
| LIB-LT-INSTANT | Thumbnails never blank while scrolling: colour sketch, packed size ladder, upscaled fallback | P0 | ⬜ | ref 05 db3 tiers |
| LIB-LT-HIDDEN | Hidden pictures shown translucent when enabled; small pictures toggle | P1 | ⬜ | |
| LIB-SEL | Selection: click, Ctrl, Shift, rubber band, scoped to one section | P0 | ⬜ | ref 02 §7.5 |
| LIB-SEL-KEYS | Keyboard navigation and selection keys | P0 | ⬜ | |
| LIB-HOVER | Tooltip on hover | P1 | ⬜ | |
| LIB-PEEK | Ctrl+Alt full-screen peek with wheel stepping | P1 | ⬜ | |
| LIB-LOUPE | Loupe magnifier over thumbnails | P2 | ⬜ | |
| LIB-THUMBSIZE | Thumbnail-size slider, Ctrl+1/Ctrl+2, Ctrl+wheel | P0 | ⬜ | |
| LIB-STATUS | Status line texts (none / one / many / edit view) | P0 | ⬜ | ref 02 §8.1 |
| LIB-TRAY | Photo Tray: token for folder, mini thumbnails, own selection | P0 | ⬜ | ref 02 §8.2 |
| LIB-TRAY-HOLD | Hold / Clear / Add to Album ▾ | P0 | ⬜ | |
| LIB-STARROT | Star and rotate buttons acting on the tray | P0 | ⬜ | |
| LIB-ACTIONS | Action buttons (Export, Collage, Slideshow) with overflow | P0 | ⬜ | |
| LIB-CONFBUTTONS | Configure Buttons dialog | OOS | 🚫 | dropped 2026-10-09 (dated feature); the button set is fixed |
| LIB-PANES | Side pane container with People/Places/Tags/Properties toggles | P1 | ⬜ | |
| LIB-PROPS | Properties pane | P1 | ⬜ | |
| LIB-ROOMS | Room tab strip (Import, Collage) | P1 | ⬜ | ref 01 §2.3 |
| LIB-EMPTY | Empty states | P0 | ⬜ | |

## B. Organizing (ORG, IMP, PPL, GEO, SRCH, FILE)

| Id | Feature | Tier | Status | Evidence / notes |
|---|---|---|---|---|
| ORG-FIRSTRUN | First-run panel and progressive first scan | P0 | ⬜ | ref 04 §1 |
| ORG-FOLDERMGR | Folder Manager (Scan Always / Once / Remove, faces toggle, watched list) | P0 | ⬜ | ref 04 §2 |
| ORG-WATCH | Watching for changes, offline volumes | P0 | ⬜ | |
| ORG-ALBUM-NEW | New album + Album Properties | P0 | ⬜ | |
| ORG-ALBUM-EDIT | Add/remove/reorder/cover/delete album | P0 | ⬜ | |
| ORG-ALBUM-AUTO | Starred Photos, Recently Updated, Search results albums | P0 | ⬜ | |
| ORG-STAR | Star toggle everywhere | P0 | 🟡 | library view only: Ctrl+8 / F4 and the bottom-bar star button (`cmd:photo.star.toggle`, `crates/ui/tests/library_window.rs`); menus, tray, edit view missing |
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
| VIEW-FULLSCREEN | F11 full screen | P1 | ⬜ | |
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

## G. Preferences (PREF) and system (SYS)

| Id | Feature | Tier | Status | Evidence / notes |
|---|---|---|---|---|
| PREF-DIALOG | Options dialog with tabs, OK/Cancel/Apply | P1 | ⬜ | ref 08 §5 |
| PREF-GENERAL / FILETYPES / SLIDESHOW / NETWORK / PEOPLE / VIEWER / EXPORT | each tab's options | P1 | ⬜ | `menus-and-shortcuts.md` §5 |
| PREF-KEYMAP | Keyboard contexts and the shortcut editor (search, record, conflicts, reset) | P1 | ⬜ | `menus-and-shortcuts.md` §2.0 |
| SYS-UPDATES | Check for updates | — | ⬜ | deferred, not planned yet (2026-10-09) |
| SYS-COMMANDS | Command registry, menu catalog, UI_COMMANDS | P0 | 🟡 | the registry (`crates/engine/src/registry.rs`); the catalog of every item of `menus-and-shortcuts.md` §1 (`crates/ui/src/menu_catalog.rs`) and the menu model, `UI_COMMANDS` and menus bounded by the window (`crates/ui/src/menus.rs`), measured by `crates/ui/tests/menus.rs`; missing: the catalog's menus in the window's menu row and the native macOS menu (M3.6), UI commands in the keymap (M3.4), check marks fed by the view state (M4) |
| SYS-CONTROL | Control channel + widget registry + screenshots | P0 | 🟡 | `crates/ui/src/control/` (every M2 method as pure handlers over the app and the headless driver: `engine.execute`, `engine.commands`, `ui.state`/`ui.set`, `ui.inspect`, `ui.widgets`, click/drag/hover/pointer/key/type/scroll/resize, `ui.screenshot` with §5.7 write roots, `ui.settle`/`ui.wait`, the file-picker stub); the transport in `apps/dalica/src/control/` (`--control [PORT]` on 127.0.0.1 only, the 0600 token file, `auth` in constant time, 1 MiB lines, 16 connections, 30 s idle, 60 s deadline, `app.quit`; requests answered from the window's logic tick or `--headless`), measured by `apps/dalica/tests/control_headless.rs`, its security rules (wrong token, no `auth`, oversized, non-JSON and HTTP lines, the 17th connection, token file modes, loopback only) by `apps/dalica/tests/control_security.rs`; `ui.menu.tree` / `ui.menu.invoke` in `crates/ui/src/control/menu.rs` (`crates/ui/tests/menus.rs`); missing: `batch`, `library.inspect`, `jobs.*`, the headless capture of a window (`ui.screenshot {headless: true}` in the window) |
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
| File | `album.new` `folders.manager` `library.openFile` `import.open` `photo.openExternal` `photo.moveToNewFolder` `photo.rename` `photo.save` `photo.revert` `photo.saveAs` `photo.saveCopy` `output.export` `photo.locate` `photo.delete` `app.quit` | ⬜ |
| Edit | `edit.undo` `edit.redo` `photo.cut` `photo.copy` `photo.paste` `edit.copyRecipe` `edit.pasteRecipe` `select.all` `select.starred` `select.invert` `select.none` | ⬜ |
| View | `view.library` `view.thumbsSmall` `view.thumbsNormal` `view.edit` `pane.properties` `pane.tags` `pane.people` `pane.places` `pane.toggle` `edit.panelToggle` `view.slideshow` `view.smallPictures` `view.hiddenPictures` `view.colorManagement` `view.caption` `view.folderView` `window.fullscreen` | ⬜ |
| Folder / Album | `section.properties` `section.slideshow` `section.refreshThumbs` `section.sort` `folder.hide` `folder.unhide` `folder.locate` `folder.remove` `folder.move` `folder.delete` `album.delete` | ⬜ |
| Picture | `view.edit` `edit.undoAll` `photo.hide` `photo.unhide` `faces.reset` `pane.properties` `photo.star.toggle` `photo.caption.edit` `photo.rotate.cw` `photo.rotate.ccw` `edit.flipH` `edit.flipV` | ⬜ |
| Create | `create.collage` | ⬜ |
| Tools | `folders.manager` `people.manager` `viewer.settings` `photo.adjustDate` `geo.clear` `geo.copy` `geo.paste` `search.duplicates` `search.color` `search.saveAsAlbum` `search.tagAsAlbum` `faces.writeXmp` `app.preferences` | ⬜ |

## I. Keyboard shortcuts (KEY)

Every shortcut in [`menus-and-shortcuts.md`](menus-and-shortcuts.md) §2 and
[`editing.md`](editing.md) §12 is a row here once the keymap exists (`cargo xtask parity` will list
the keymap's bindings and fail when a documented key has no binding). Until then: ⬜ for all.
