# Menus, shortcuts, context menus and preferences

The complete command surface of Dalica. This file is the source for `crates/ui/src/menu_catalog.rs`
(menu tree, order, separators, default shortcuts, command ids) and for the parity rows `MENU-*` and
`KEY-*` in [`parity.md`](parity.md). Labels are Dalica's; command ids are stable and never renamed.

Conventions: `Ctrl` means Cmd on macOS unless a row says otherwise; `—` is a separator; `▸` is a
submenu; *(cond)* states when an item is enabled. On macOS an application menu **Dalica** holds About,
Preferences… (Cmd+,; the Options dialog), Hide, Quit; File → Exit and Tools → Options… move there.

Every shortcut in this file is a **default**: the user can change any of them (§2.0). Items marked
*(OS integration)* talk to other applications or the operating system; they are listed so the menu
is complete but stay disabled until the last milestone (`product.md` → Scope).

## 1. Menu bar

`File  Edit  View  Folder  Picture  Create  Tools  Help` — the fourth menu is titled **Folder** when
the current section is a folder and **Album** when it is an album or person (its items change
accordingly; it is identical to the section's context menu).

### 1.1 File

| Item | Shortcut | Command id | Enabled / notes |
|---|---|---|---|
| New Album… | Ctrl+N | `album.new` | always; adds the tray contents |
| — | | | |
| Add Folder… | | `folders.manager` | opens the Folder Manager, as Tools ▸ Folder Manager… does, with the library's selected folder pre-selected (`organizing.md` §2); `folders.add {path}` adds a folder without it (agents, the CLI) |
| Open File… | Ctrl+O | `library.openFile` | show a single file (adds its folder as Scan Once) |
| Import From… | Ctrl+M | `import.open` | |
| — | | | |
| Open in External Editor | Ctrl+Shift+O | `photo.openExternal` | selection *(OS integration)* |
| — | | | |
| Move to New Folder… | | `photo.moveToNewFolder` | selection in one folder |
| Rename… | F2 | `photo.rename` | selection |
| — | | | |
| Save | Ctrl+S | `photo.save` | focused/selected photos with unsaved edits |
| Revert… | | `photo.revert` | saved photos with an original backup |
| — | | | |
| Save As… | | `photo.saveAs` | one photo with edits; file dialog for name, folder and format (`data-model.md` §6) |
| Save a Copy | | `photo.saveCopy` | photos with edits |
| Export Pictures to Folder… | Ctrl+Shift+S | `output.export` | tray not empty |
| — | | | |
| Locate on Disk | Ctrl+Enter | `photo.locate` | *(OS integration)* |
| Delete from Disk… | Delete (Ctrl+Delete in albums) | `photo.delete` | |
| — | | | |
| Exit | Ctrl+Q | `app.quit` | |

### 1.2 Edit

Reference order; Undo/Redo are a Dalica addition placed first.

| Item | Shortcut | Command id | Notes |
|---|---|---|---|
| Undo *<last action>* | Ctrl+Z | `edit.undo` | label names the action ("Undo Star"); library view: the library undo stack; edit view: the open photo's history (`architecture.md` §4) |
| Redo *<action>* | Ctrl+Y (macOS Cmd+Shift+Z) | `edit.redo` | same two histories as Undo |
| — | | | |
| Cut | Ctrl+X | `photo.cut` | for moving files between folders inside Dalica |
| Copy | Ctrl+C | `photo.copy` | for copying files between folders inside Dalica; putting paths and the image on the system clipboard for other apps is *(OS integration)* |
| Paste | Ctrl+V | `photo.paste` | into the current folder (move after Cut, copy after Copy) |
| — | | | |
| Copy All Edits | | `edit.copyRecipe` | one photo with edits |
| Paste All Edits | | `edit.pasteRecipe` | replaces recipes of the selection |
| — | | | |
| Select All | Ctrl+A | `select.all` | current section |
| Select Starred | | `select.starred` | current section |
| Invert Selection | Ctrl+I | `select.invert` | |
| Select None | Ctrl+D | `select.none` | |

### 1.3 View

| Item | Shortcut | Command id | Notes |
|---|---|---|---|
| Library View | Ctrl+1 *(edit view context)* | `view.library` | disabled when already in the library; in the edit view Ctrl+1 means Back to Library (§2.0) |
| — | | | |
| Small Thumbnails | Ctrl+1 *(library context)* | `view.thumbsSmall` | radio group of two |
| Normal Thumbnails | Ctrl+2 *(library context)* | `view.thumbsNormal` | |
| Edit View | Ctrl+3 | `view.edit` | "Select a photo to edit." toast without a selection; in the edit view Ctrl+3 keeps the photo shown, its zoom and its open tool |
| — | | | |
| Properties | Alt+Enter | `pane.properties` | checkable; Ctrl+0 toggles the side pane (`pane.toggle`; when closed it opens Properties) |
| Tags | Ctrl+T | `pane.tags` | checkable |
| People | | `pane.people` | checkable |
| Places | | `pane.places` | checkable |
| — | | | |
| Show Edit Controls | Ctrl+9 | `edit.panelToggle` | checkable; in the library it sets whether the edit panel shows when the edit view next opens |
| — | | | |
| Slideshow | Ctrl+4 | `view.slideshow` | |
| — | | | |
| Small Pictures | | `view.smallPictures` | checkable |
| Hidden Pictures | | `view.hiddenPictures` | checkable |
| — | | | |
| Use Color Management | | `view.colorManagement` | checkable (display ICC profile) |
| — | | | |
| Thumbnail Caption ▸ | | `view.caption` | None · Filename · Caption · Tags · Resolution · Date (radio; Date is a Dalica addition) |
| Folder View ▸ | | `view.folderView` | Flat Folder View · Tree View (radio) — Simplified Tree View (check) |
| — | | | |
| Full Screen | F11 | `window.fullscreen` | |

The toolbar's ▾ **view options** popup (also the tree's right-click menu on empty space): Sort by
Date · Sort by Recent Changes · Sort by Size · Sort by Name · Reverse Sort — Sort People by Name ·
Sort People by Count · Sort People by Top 10 — Shortcuts ▸ (Home, Pictures, Documents, Desktop) —
Show Thumbnails in Tree · Simplified Tree View.

### 1.4 Folder / Album

The same list as the section context menu (`organizing.md` §9.2).

| Item | Shortcut | Command id | Notes |
|---|---|---|---|
| Edit Description… | | `section.properties` | Folder/Album Properties |
| View Slideshow | Ctrl+4 | `section.slideshow` | |
| — | | | |
| Refresh Thumbnails | | `section.refreshThumbs` | |
| Sort By ▸ | | `section.sort` | Date · Name · Size · Reverse Order |
| — | | | |
| Hide | | `folder.hide` | |
| Unhide | | `folder.unhide` | disabled unless hidden |
| — | | | |
| Locate on Disk | Ctrl+Enter | `folder.locate` | disabled for albums *(OS integration)* |
| Remove from Dalica… | | `folder.remove` | disabled for albums and while no folder is selected; asks "Remove Folder" first, naming the folder (`organizing.md` §2) |
| — | | | |
| Move… | | `folder.move` | disabled for albums |
| Delete… | | `folder.delete` / `album.delete` | |

### 1.5 Picture

Reference items in reference order, then a group of Dalica additions (the reference kept these
only in context menus and buttons).

| Item | Shortcut | Command id |
|---|---|---|
| View and Edit | Ctrl+3 (Enter in context menus) | `view.edit` |
| Batch Edit ▸ | | (submenu, `editing.md` §9) |
| — | | |
| Undo All Edits… | | `edit.undoAll` |
| — | | |
| Hide | | `photo.hide` |
| Unhide | | `photo.unhide` |
| — | | |
| Find Faces Again | | `faces.reset` |
| — | | |
| Properties | Alt+Enter (macOS Cmd+I) | `pane.properties` |
| — | | |
| Star / Unstar | Ctrl+8 (F4) | `photo.star.toggle` |
| Edit Caption | | `photo.caption.edit` |
| Rotate Clockwise / Counterclockwise | Ctrl+R / Ctrl+Shift+R | `photo.rotate.cw` / `photo.rotate.ccw` |
| Flip Horizontal / Vertical | Ctrl+Shift+H / Ctrl+Shift+V | `edit.flipH` / `edit.flipV` |

### 1.6 Create

| Item | Command id |
|---|---|
| Picture Collage… | `create.collage` |

### 1.7 Tools

| Item | Shortcut | Command id |
|---|---|---|
| Folder Manager… | | `folders.manager` |
| People Manager… | | `people.manager` |
| — | | |
| Configure Photo Viewer… | | `viewer.settings` |
| — | | |
| Adjust Date and Time… | | `photo.adjustDate` |
| — | | |
| Geotag ▸ Clear Location… · Copy Location · Paste Location | | `geo.clear` · `geo.copy` · `geo.paste` |
| Experimental ▸ | | Show Duplicate Files (Ctrl+F6, `search.duplicates`) · Search for ▸ Red, Orange, Yellow, Green, Blue, Purple, Black and White (`search.color`) · Save Search Results… (`search.saveAsAlbum`) · Show Tag as Album… (`search.tagAsAlbum`) · Write People Names to Photos… (`faces.writeXmp`) |
| — | | |
| Options… | Ctrl+, | `app.preferences` (macOS: Dalica → Preferences…) |

### 1.8 Help

Help Contents (F1, `help.contents`) · Keyboard Shortcuts… (Ctrl+/, `app.shortcuts`; opens Options →
Keyboard Shortcuts, §2.0) — Release Notes (`help.releaseNotes`) · Report a Problem…
(`help.reportProblem`) · Show Log Folder (`help.logFolder`) · Rebuild Library… (`library.rebuild`) —
About Dalica (`app.about`; version, credits, models). The macOS application menu's Hide Dalica is
`app.hide`. There is no Check for Updates item
for now (deferred, `product.md` → Scope).

**Rows that share a command.** Submenu rows that differ only by a parameter run one command with
that parameter, and their widget id ends in the variant (`menuitem:view.caption/date`): Thumbnail
Caption ▸ `view.caption {caption: none | filename | caption | tags | resolution | date}`; Folder
View ▸ `view.folderView {mode: flat | tree}`, and Simplified Tree View is `view.simplifiedTree`;
Sort By ▸ `section.sort {by: date | name | size}` and Reverse Order `section.sort {reverse: true}`
(their marks show the current folder's photo sort; none in an album, M10.25);
Search for ▸ `search.color {color: red | orange | yellow | green | blue | purple | bw}`. Batch Edit ▸
runs `photo.rename`, `photo.rotate.cw` and `photo.rotate.ccw` (widget part `batch`, since the
Picture menu lists the rotations too) and `edit.apply {op: autocontrast | autocolor | autofix |
sepia | sharpen | warmify | grain | bw}`.

### 1.9 Commands without a menu item

Used by panes, inline editors, the CLI, the control channel and MCP (`control-protocol.md`). Query
commands change nothing and record no undo step.

| Command id | Purpose |
|---|---|
| `photo.caption.set` | set captions (`{ids, caption}`); the inline caption editors call it |
| `app.setShortcut` | bind or unbind a key (`{command, context, key, slot?, replace?}`; without `slot` the key replaces the command's keys in that context and `key: null` removes them; with `slot` (0 or 1, a command having up to two keys per context, §2.0) only that key is set or removed and the other kept; keys as written or as the screen shows them: `ctrl-8`, `⇧⌘S`). A key another command has where both can be active is refused with the editor's sentence ("Ctrl+1 is already Small Thumbnails in Library."), unless `replace`, which takes it from that command (`took` lists whom); keys set back to the defaults are no longer an override. The shortcut editor calls it |
| `app.resetShortcuts` | drop user overrides (`{command?}`: one command, or all when omitted) |
| `keymap.list` | query: one row per command and context with its keys in effect, its default keys, whether it differs from the default and whether it can be edited (`{context?, search?, withoutKeys?}`); the shortcut editor's list. A command without keys is listed once with no context (with `withoutKeys`, under every Context filter). The search matches labels, menu paths and keys ignoring case; a search that reads as a key matches that key (`ctrl 1`, `⌘1`: every Ctrl+1 binding), or with no modifier typed that key under any (`F4`, never F11 for `F1`) |
| `options.list` | query: the options catalogue's rows (§5) with their values in effect and whether they can be changed yet (`{tab?}`: `general`, `fileTypes`, `slideshow`, `viewer` or `export`) |
| `options.set` | change options (`{values}`: `{key: value}`, every key and value checked against the catalogue before anything changes, an unknown key, an option still to come or a bad value being `bad_params` naming it; one atomic write of `settings.json`, the session taking the values only once it is on disk, so a failed write (a read-only file too) is `disabled` with its reason and changes nothing; no undo step; answers `{changed, restart, rescan, job}`: the keys changed, those applying at the next launch, and the scan a File Types or threshold change started); the Options dialog's Apply and OK |
| `photo.star.set` | set or clear stars explicitly (`{ids, on}`) |
| `photo.tags.add` / `photo.tags.remove` | add or remove tags (`{ids, tags}`: text with commas separating tags, or a list of whole tags); the Tags pane. Tags added here are the recently used ones |
| `photo.tags.toggle` | remove a tag from all the photos when all hold it, else add it to all (`{ids, tag}`); the quick tag buttons |
| `tags.list` | query: every tag with its count, completion for a prefix, or the tags of some photos with partial counts (`{ids?, prefix?}`) |
| `tags.rename` | rename a tag on every photo, merging onto an existing tag (`{from, to, confirm}`; asks first) |
| `tags.quick.get` / `tags.quick.set` | the quick tags (`{slots, useRecent}`; `get` also gives what each button applies) |
| `tags.quick.apply` / `tags.quick.1` … `tags.quick.10` | toggle quick tag n on photos (`{n, ids}`; the numbered ones carry Ctrl+Alt+1 … Ctrl+Alt+0) |
| `photo.geo.set` | Places pane drop |
| `photo.inspect` | query: everything known about photos (`{ids}`) |
| `library.query` | query: search with the search-box grammar, filters and paging |
| `library.inspect` | query: the library's state for agents (`control-protocol.md` §6.4: counts, tree, sections, search, undo, unsaved edits, pending writes, problems; `{photos?, section?, offset?, limit?}`) |
| `library.rebuild` | delete and rebuild the index (confirmation; Help → Rebuild Library…) |
| `edit.apply` | Batch Edit: append one edit operation to each photo's recipe with the edit view's merge rules, as one library undo step (`{ids, op, params?}`; an operation this build cannot render is `disabled`, "Arrives with Effects"); Picture ▸ Batch Edit and MCP's `apply_edit` (M11.9) |
| `edit.copyRecipe` | Copy All Edits: keep one photo's recipe in the session, never saved (`{id}`; M11.9) |
| `edit.pasteRecipe` | Paste All Edits: replace recipes with the copied one, as one library step (`{ids}`), or as one step of the open photo's history in the edit view (`{id, history: true}`) (M11.9) |
| `edit.undoAll` | Undo All Edits: remove every unsaved edit (recipe and redo list) as one library step that can be undone (`{ids, confirm}`; `needs_confirm` without), or one history step in the edit view (`{id, history: true}`); saved photos keep their saved pixels (M11.9) |
| `edit.variant.open` / `edit.variant.close` | A│A's variant: a copy of the photo's recipe with its own undo, held in the session only (`{id}`), dropped or kept as one step of the photo's history (`{keep: "left" \| "right"}`); `photo.history.apply`, `.undo`, `.redo` and `.state` take `variant: true` (M11.10) |
| `import.sources` | query: the cards, drives and recent sources to import from, from the cached list, never waiting on the disk (M11.7) |
| `import.openSource` | open a source in the Import room (`{device}`: a card or drive from `import.sources`, or `{path}`: a folder, checked against automation's read roots); starts the listing and drops an older one (M11.7; the UI's `import.open`, Ctrl+M, opens the room) |
| `import.list` | query: the listing's status ("Loading 112 of 340 files…") and its candidates in capture order, paged (`{offset?, limit?}`, at most 1 000) (M11.7) |
| `import.exclude` / `import.excludeAll` / `import.mark` / `import.options` / `import.close` | the room's choices, no undo step: exclude or include candidates (`{ids, on}`, `{on}`), their star and rotation (`{ids, star?, rotate?}`), "Exclude duplicates" (`{excludeDuplicates}`); close the listing (M11.7) |
| `import.run` | import as a job (`{ids?, destination, folderName, after}`; `folderName` a text, `{dateTaken: true}` or `{today: true}`): copy, verify, enter the photos as one library undo step "Import" (M11.8) |
| `import.removeFromSource` | remove the last import's verified copies from the card (`{confirm}`; `needs_confirm` without); refused over the control channel and MCP (M11.8) |
| `slideshow.list` | query: the photos a slideshow plays and where it starts (`{section?, ids?, search?, start?}`; videos left out and counted) (M11.11) |
| `viewer.list` | query: the photos of the viewed file's folder in name order and the file's place among them (`{path}`) (M11.12) |
| `edit.recipe.set` | replace recipes, each one step of its photo's history (`{ids, recipe, label?, dropUnknown?}`; refuses to drop an operation it cannot read unless `dropUnknown`); used by Paste All Edits and agents |
| `section.scrollTo` | scroll the light table to a section (tree click) |
| `tray.hold` / `tray.clear` | tray buttons (Hold: Ctrl+H) |
| `tray.select` | the tray's own selection (`{ids, mode}`: click, Ctrl-click, rubber band in the tray) |

## 2. Keyboard shortcuts by context

### 2.0 Contexts and the shortcut editor

Maintainer decision, 2026-10-09: every shortcut is user-editable, and a key may mean different
things in different places.

**Contexts.** A binding is *(key, context) → command*. The contexts are **Global**, **Library**,
**Edit view**, **Slideshow**, **Photo viewer**, **Import**, **Collage** and **Text input**.
- Exactly one view context is active at a time (the library, the edit view, a running slideshow,
  the standalone viewer, or the active room). **Global** is always active underneath it.
  **Text input** is active on top while a text field has keyboard focus.
- A key press is resolved in the active view context → Global; the first binding found runs.
  While a text field has keyboard focus it is resolved Text input → Global instead: the view
  context's shortcuts are off while typing, so the field keeps the keys it needs (characters,
  arrows, Home/End, Backspace, Delete, Enter, Ctrl+A/C/V/X/Z/Y) and a view shortcut (Star's Ctrl+8,
  Select All, Undo) never acts on the photos behind it; Global keys (F11, Ctrl+Q) still work. A
  view command meant to work while typing (M8's Save, Ctrl+S, from the edit view's caption field)
  must be bound in Global or Text input.
- Two bindings **conflict** only when they use the same key in contexts that can be active at the
  same time: the same context, or Global plus any other. The same key in two view contexts (Ctrl+1
  is Small Thumbnails in Library and Back to Library in Edit view) is not a conflict.
- A shortcut whose command is disabled right now does nothing except show the reason on the status
  line ("Select a photo to edit."), never silently fall through to another binding.
- A command may have up to two keys per context (Star: Ctrl+8 and F4 in the Library, Ctrl+8 and
  Space in the edit view). Hold gestures (Ctrl+Alt full-screen preview, `\` show original) are
  listed but not editable for now.
- Keys are written platform-neutrally: `Ctrl` is the primary modifier (Cmd on macOS); defaults
  that differ by platform say so (Redo: Ctrl+Y, macOS Cmd+Shift+Z).

**Where defaults live.** Each `CommandSpec` declares its default bindings with their contexts
(`architecture.md` §3); `UI_COMMANDS` does the same, and the app adds them to the session's keymap,
so UI commands are conflict-checked and rebound through `app.setShortcut` like the engine's. A
default for some platforms only carries a qualifier: Redo declares `win,linux:Ctrl+Y` and
`mac:Ctrl+Shift+Z`; an unqualified default applies on all three (a key the user binds never has
one). The menus show whatever is in effect (the catalog's default only until a library is open).
A test fails if two default bindings conflict on any of the three platforms or a binding names an
unknown command.

**The editor:** Options → **Keyboard Shortcuts** (also Help → Keyboard Shortcuts…, Ctrl+/):
- a search field (matches command names, menu paths and keys: typing "ctrl+1" finds both Ctrl+1
  bindings), a **Context** filter (All, Global, Library, Edit view, …) and **Show commands without
  a shortcut**;
- one row per command and context: the menu path and label, the context, its key(s), and a ↺
  button when it differs from the default;
- click a key cell to record: "Press the new shortcut…"; Esc cancels, Backspace removes the key.
  A conflict shows inline: "Ctrl+1 is already Small Thumbnails in Library." [Replace] [Cancel];
  Replace removes it from the other command;
- **Reset All** restores every default (confirmation "Restore all default shortcuts?"
  [Reset All] [Cancel]);
- changes follow the dialog's Apply / OK / Cancel rules (`design-system.md` §11.3).

**Storage.** Only the user's overrides are stored, in `settings.json` under `"keymap"` as
`{command, context, key}` entries (`key: null` = removed). Entries naming a command this version
does not know are kept verbatim and ignored. The editor goes through `app.setShortcut` /
`app.resetShortcuts` (§1.9), so agents and tests can rebind keys too. Prior art: LightCraft
`crates/ui-egui/src/panels/keymap.rs` and PhotoCraft `shortcut_dispatch.rs`.

### 2.1 Global

| Key | Action |
|---|---|
| Ctrl+N / Ctrl+O / Ctrl+M | new album / open file / import |
| Ctrl+3 / Ctrl+4 | edit view / slideshow |
| Ctrl+0 / Ctrl+9 | side pane / edit controls |
| Ctrl+T | Tags pane |
| Ctrl+F, F3 | search |
| Ctrl+S / Ctrl+Shift+S | save / export |
| Ctrl+Shift+O | open in external editor *(OS integration)* |
| Ctrl+Enter | locate on disk *(OS integration)* |
| Alt+Enter | properties |
| F2 | rename |
| F5 | refresh the current folder |
| F11 | full screen |
| F1 | help |
| Ctrl+Z / Ctrl+Y | undo / redo (library stack in the library, the photo's history in the edit view) |
| Ctrl+Q | quit |

### 2.2 Library

| Key | Action |
|---|---|
| Ctrl+1 / Ctrl+2 | small / normal thumbnails |
| ← → ↑ ↓ | move the focus/selection within the section (Shift extends) |
| Home / End | first / last photo of the section (Shift extends) |
| Ctrl+Home / Ctrl+End | top / bottom of the library |
| PgUp / PgDn | scroll a page |
| Ctrl+↑ / Ctrl+↓ | previous / next section |
| Enter | view and edit |
| Ctrl+A / Ctrl+D / Ctrl+I | select all / none / invert (section) |
| Ctrl+H | hold in the tray |
| Ctrl+8 | star |
| Ctrl+R / Ctrl+Shift+R | rotate |
| Delete / Ctrl+Delete | delete from disk (folders) or remove from album (albums) / always delete from disk |
| Ctrl+Alt (hold, hover) | full-screen preview; wheel steps |
| Ctrl+Alt+1 … Ctrl+Alt+0 | apply quick tags 1–10 |
| Ctrl+wheel | thumbnail size |
| Esc | leave search mode (when active) |

### 2.3 Edit view

See [`editing.md`](editing.md) §12.

### 2.4 Slideshow, photo viewer, video, import

See [`viewing.md`](viewing.md) §1.4 and §3.2, [`editing.md`](editing.md) §11, and in import: **X**
toggles exclude on the selected item, Enter imports selected.

### 2.5 Hidden extras

- No easter eggs that ship third-party imagery.

## 3. Mouse conventions

| Where | Click | Double-click | Right-click | Drag | Wheel |
|---|---|---|---|---|---|
| Tree row | scroll to + select section | properties | context menu | — (drop target) | scroll tree |
| Section header | select section | title: properties | section menu | — | scroll |
| Thumbnail | select (Ctrl toggle, Shift range) | edit view | photo menu | move/copy/add/reorder/export | scroll (Ctrl: size) |
| Empty light-table space | clear selection | — | section menu | rubber band | scroll |
| Tray | select tray item | view and edit | tray menu | drag out | — |
| Edit-view photo | (tool-specific) | back to library | photo menu (+ Back to Library Esc) | pan / tool | zoom |

## 4. Context menus

- **Photo** (thumbnail, filmstrip, edit-view photo): `organizing.md` §9.1.
- **Folder / album** (tree row, section header, empty light-table space): `organizing.md` §9.2.
- **Tree group header**: Collapse/Expand · Sort ▸ (for Folders and People).
- **Tray**: `ui-layout.md` §6.2.
- **Face** (people views): Move to New Person… · Move to Person ▸ · Remove from Person · Set as Person
  Thumbnail · Ignore Face.
- **Tag** (Tags pane): Add to All Selected · Search for This Tag · Remove from Selected · Rename Tag…
  (renames everywhere, with confirmation).
- Disabled items are shown greyed, never hidden. Menus never run off the window (bounded and
  scrolling, as in LightCraft's `menu_level.rs`).

## 5. Options (Tools → Options…; macOS Dalica → Preferences…)

Tabs: **General · File Types · Slideshow · Network · People · Photo Viewer · Export · Keyboard
Shortcuts** (§2.0). Buttons [OK] [Cancel] [Apply] (Apply disabled until something changes; Cancel discards
changes since the last Apply).

Every option of the General, File Types, Slideshow, Photo Viewer and Export tabs is a row of the
engine's options catalogue
(`crates/engine/src/options.rs`), stored in `settings.json` under the key given here
(`interface.animations` is `{"interface": {"animations": …}}`; `data-model.md` §4). The dialog, the
`options.list {tab?}` and `options.set {values}` commands (§1.9) and MCP all read that catalogue: its
type, default, range or choices, group, label and hint, when a change applies (at once, after a
rescan, at the next launch), and whether its feature exists yet. An option whose feature arrives
later is shown disabled with its reason ("Arrives with sound support") and its setting is stored
only once its feature reads it. A test fails when a row of these five tables and the catalogue
disagree.

### General

| Group | Option | Key | Default |
|---|---|---|---|
| Interface | Use animations (smooth transitions when rotating, applying edits, previewing) | `interface.animations` | on |
| | Show tooltips | `interface.tooltips` | on |
| | Use system title bar (Windows/Linux; applies at next launch) | `interface.systemTitleBar` | off |
| | Single click on the photo returns to the library | `edit.singleClickLeaves` | off |
| | UI scale (Auto, 75–300 % in steps of 25) | `interface.uiScale` | Auto |
| | UI font size (Small, Medium, Large) | `interface.fontSize` | Small |
| | Language (arrives with translations; applies at next launch) | `interface.language` | System |
| Files | Detect duplicates while importing (the Import room's "Exclude duplicates" starts from it) | `import.detectDuplicates` | on |
| | Delete from disk without asking | `organize.deleteWithoutAsking` | off |
| | Remove from albums without asking | `organize.removeFromAlbumsWithoutAsking` | off |
| | Write captions, tags and places into photo files | `files.writeCaptionsTagsPlaces` | on |
| | Write stars as ratings (XMP) | `files.writeStarsAsRatings` | off |
| | Update file modification time on metadata changes | `files.updateModificationTime` | off |
| | Write XMP sidecars for RAW files | `files.writeRawXmpSidecars` | off |
| | Write date changes to files (`xmp-interop.md` §2) | `files.writeDates` | on |
| | Read photo data left by the previous organizer (`data-model.md` §9; arrives with it) | `files.readLegacyData` | on |
| Import | Save imported pictures in (the Import room's "Import to:" starts from it) | `import.folder` | ~/Pictures |

### File Types

"Show JPEG files and:" one checkbox per type (JPEG is always shown); a change rescans the watched
folders.

| Group | Option | Key | Default |
|---|---|---|---|
| Show JPEG files and: | PNG | `fileTypes.png` | on |
| | GIF | `fileTypes.gif` | on |
| | BMP | `fileTypes.bmp` | on |
| | TIFF | `fileTypes.tiff` | on |
| | WebP | `fileTypes.webp` | on |
| | HEIC/HEIF (disabled, "HEIC support isn't included in this build", in a build without it) | `fileTypes.heif` | on |
| | AVIF | `fileTypes.avif` | on |
| | JPEG XL | `fileTypes.jxl` | on |
| | PSD | `fileTypes.psd` | on |
| | RAW formats (list…) | `fileTypes.raw` | on |
| | Videos (MP4, MOV, M4V, MKV) | `fileTypes.video` | on |
| Small pictures | Small-picture threshold (16–1024 px) | `fileTypes.smallPictureThreshold` | 64 px |

### Slideshow

As [`viewing.md`](viewing.md) §1.5. The slideshow's bar changes the transition, the display time
and the captions too, and they persist here. Playing sound needs an audio library Dalica does not
include yet, so the music and video sound options are drawn disabled ("Arrives with sound
support") until it does.

| Group | Option | Key | Default |
|---|---|---|---|
| Slideshow | Loop slideshow | `slideshow.loop` | off |
| | Play music during slideshows (arrives with sound support) | `slideshow.music` | off |
| | Music folder [Browse…] (arrives with sound support) | `slideshow.musicFolder` | none |
| | Display time (1–60 s) | `slideshow.displayTime` | 3 s |
| | Transition (Cut, Pan and Zoom, Dissolve, Wipe, Circle, Push, Rectangle) | `slideshow.transition` | Dissolve |
| | Show captions | `slideshow.captions` | on |
| | Play video sound (arrives with sound support) | `slideshow.videoSound` | off |

### Network

Allow map tiles (off until the Places pane asks) · Map tile server (URL template, default
OpenStreetMap with attribution) · Geocoding service · Proxy (system / manual) · Log level.

### People

Find faces in my photos (on) · Suggest names (on) · Suggestion confidence (85) · Grouping
confidence (85) · Store people names in photos (off) · Face model: status + [Download…] / [Remove].

### Photo Viewer

As [`viewing.md`](viewing.md) §3.3, which the viewer's ⋯ → Settings… also opens. Choosing Dalica as
the default viewer needs the OS integration, so the File Types part is drawn disabled ("Arrives
with OS integration") until then.

| Group | Option | Key | Default |
|---|---|---|---|
| File Types | Use Dalica as the default viewer for these file types (arrives with OS integration) | `viewer.defaultViewer` | off |
| Interface | Mouse wheel (zooms, hold Ctrl to navigate; or moves through photos, hold Ctrl to zoom) | `viewer.wheel` | zooms |
| | Hide controls automatically | `viewer.hideControls` | on |
| | Translucent background | `viewer.translucent` | on |

### Export

The defaults the Export dialog starts from (`creating.md` §2).

| Group | Option | Key | Default |
|---|---|---|---|
| Location | Default export location | `export.location` | Pictures/Dalica/Exports |
| Size and quality | Resize to (off: use the original size) | `export.resize` | on |
| | Size (100–4000 px) | `export.size` | 1600 px |
| | Image quality (Automatic, Normal, Maximum, Minimum, Custom) | `export.quality` | Automatic |
| | Custom quality (1–100) | `export.customQuality` | 75 |
| Privacy | Remove location from exported photos | `export.removeLocation` | off |
| Watermark | Default watermark text | `export.watermarkText` | empty |

### Keyboard Shortcuts

The shortcut editor of §2.0.
