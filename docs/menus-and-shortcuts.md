# Menus, shortcuts, context menus and preferences

The complete command surface of Dalica. This file is the source for `crates/ui/src/menu_catalog.rs`
(menu tree, order, separators, default shortcuts, command ids) and for the parity rows `MENU-*` and
`KEY-*` in [`parity.md`](parity.md). Labels are Dalica's; command ids are stable and never renamed.

Conventions: `Ctrl` means Cmd on macOS unless a row says otherwise; `—` is a separator; `▸` is a
submenu; *(cond)* states when an item is enabled. On macOS an application menu **Dalica** holds About,
Preferences… (Cmd+,; the Options dialog), Hide, Quit; File → Exit and Tools → Options… move there.

## 1. Menu bar

`File  Edit  View  Folder  Picture  Create  Tools  Help` — the fourth menu is titled **Folder** when
the current section is a folder and **Album** when it is an album or person (its items change
accordingly; it is identical to the section's context menu).

### 1.1 File

| Item | Shortcut | Command id | Enabled / notes |
|---|---|---|---|
| New Album… | Ctrl+N | `album.new` | always; adds the tray contents |
| — | | | |
| Add Folder… | | `folders.manager` | opens Folder Manager |
| Open File… | Ctrl+O | `library.openFile` | show a single file (adds its folder as Scan Once) |
| Import From… | Ctrl+M | `import.open` | |
| — | | | |
| Open in External Editor | Ctrl+Shift+O | `photo.openExternal` | selection |
| — | | | |
| Move to New Folder… | | `photo.moveToNewFolder` | selection in one folder |
| Rename… | F2 | `photo.rename` | selection |
| — | | | |
| Save | Ctrl+S | `photo.save` | focused/selected photos with unsaved edits |
| Revert… | | `photo.revert` | saved photos with an original backup |
| — | | | |
| Save As… | | `photo.saveAs` | one photo with edits |
| Save a Copy | | `photo.saveCopy` | photos with edits |
| Export Pictures to Folder… | Ctrl+Shift+S | `output.export` | tray not empty |
| — | | | |
| Locate on Disk | Ctrl+Enter | `photo.locate` | |
| Delete from Disk… | Delete (Ctrl+Delete in albums) | `photo.delete` | |
| — | | | |
| Exit | Ctrl+Q | `app.quit` | |

### 1.2 Edit

Reference order; Undo/Redo are a Dalica addition placed first.

| Item | Shortcut | Command id | Notes |
|---|---|---|---|
| Undo *<last action>* | Ctrl+Z | `edit.undo` | label names the action ("Undo Star") |
| Redo *<action>* | Ctrl+Y (macOS Cmd+Shift+Z) | `edit.redo` | |
| — | | | |
| Cut | Ctrl+X | `photo.cut` | for moving files between folders |
| Copy | Ctrl+C | `photo.copy` | files to the clipboard (paths + image for other apps) |
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
| Library View | | `view.library` | disabled when already in the library |
| — | | | |
| Small Thumbnails | Ctrl+1 | `view.thumbsSmall` | radio group of three |
| Normal Thumbnails | Ctrl+2 | `view.thumbsNormal` | |
| Edit View | Ctrl+3 | `view.edit` | "Select a photo to edit." toast without a selection |
| — | | | |
| Properties | Alt+Enter | `pane.properties` | checkable; Ctrl+0 toggles the side pane (`pane.toggle`) |
| Tags | Ctrl+T | `pane.tags` | checkable |
| People | | `pane.people` | checkable |
| Places | | `pane.places` | checkable |
| — | | | |
| Show Edit Controls | Ctrl+9 | `edit.panelToggle` | checkable; edit view only |
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
| Locate on Disk | Ctrl+Enter | `folder.locate` | disabled for albums |
| Remove from Dalica… | | `folder.remove` | disabled for albums |
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

Help Contents (F1) · Keyboard Shortcuts (Ctrl+/) — Release Notes · Report a Problem… · Show Log
Folder · Rebuild Library… (`library.rebuild`) — Check for Updates — About Dalica (version, credits, models).

### 1.9 Commands without a menu item

Used by panes, inline editors, the CLI, the control channel and MCP (`control-protocol.md`). Query
commands change nothing and record no undo step.

| Command id | Purpose |
|---|---|
| `photo.caption.set` | set captions (`{ids, caption}`); the inline caption editors call it |
| `photo.star.set` | set or clear stars explicitly (`{ids, on}`) |
| `photo.tags.add` / `photo.tags.remove` | Tags pane and quick tags |
| `photo.geo.set` | Places pane drop |
| `photo.inspect` | query: everything known about photos (`{ids}`) |
| `library.query` | query: search with the search-box grammar, filters and paging |
| `library.rebuild` | delete and rebuild the index (confirmation; Help → Rebuild Library…) |
| `edit.apply` | append an edit operation to recipes (`{ids, op, params}`) |
| `edit.recipe.set` | replace recipes (used by Paste All Edits and agents) |
| `section.scrollTo` | scroll the light table to a section (tree click) |
| `tray.hold` / `tray.clear` | tray buttons |

## 2. Keyboard shortcuts by context

### 2.1 Global

| Key | Action |
|---|---|
| Ctrl+N / Ctrl+O / Ctrl+M | new album / open file / import |
| Ctrl+1 / Ctrl+2 | small / normal thumbnails (library) |
| Ctrl+3 / Ctrl+4 | edit view / slideshow |
| Ctrl+0 / Ctrl+9 | side pane / edit controls |
| Ctrl+T | Tags pane |
| Ctrl+F, F3 | search |
| Ctrl+S / Ctrl+Shift+S | save / export |
| Ctrl+Shift+O | open in external editor |
| Ctrl+Enter | locate on disk |
| Alt+Enter | properties |
| F2 | rename |
| F5 | refresh the current folder |
| F11 | full screen |
| F1 | help |
| Ctrl+Z / Ctrl+Y | undo / redo |
| Ctrl+Q | quit |

### 2.2 Library

| Key | Action |
|---|---|
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

Tabs: **General · File Types · Slideshow · Network · People · Photo Viewer · Export**. Buttons [OK] [Cancel] [Apply] (Apply disabled until something changes; Cancel discards
changes since the last Apply).

### General

| Group | Option | Default |
|---|---|---|
| Interface | Use animations (smooth transitions when rotating, applying edits, previewing) | on |
| | Show tooltips | on |
| | Use system title bar (Windows/Linux; applies at next launch) | off |
| | Single click on the photo returns to the library | off |
| | UI scale (Auto, 75–300 %) / UI font size (Small, Medium, Large) | Auto / Small |
| | Language | System |
| Files | Detect duplicates while importing | on |
| | Delete from disk without asking | off |
| | Remove from albums without asking | off |
| | Write captions, tags and places into photo files | on |
| | Write stars as ratings (XMP) | off |
| | Update file modification time on metadata changes | off |
| | Write XMP sidecars for RAW files | off |
| | Read photo data left by the previous organizer (`data-model.md` §9) | on |
| Import | Save imported pictures in | ~/Pictures |
| Updates | Check for updates: Ask / Never | Ask |

### File Types

"Show JPEG files and:" ☑ PNG ☑ GIF ☑ BMP ☑ TIFF ☑ WebP ☑ HEIC/HEIF ☑ AVIF ☑ JPEG XL ☑ PSD ☑ RAW formats
(list…) ☑ Videos (MP4, MOV, M4V, MKV). Small-picture threshold
(64 px).

### Slideshow

Loop slideshow (off) · Play music during slideshows (off) + folder [Browse…] · Display time (3 s) ·
Transition (Dissolve) · Show captions (on) · Play video sound (off).

### Network

Allow map tiles (off until the Places pane asks) · Map tile server (URL template, default
OpenStreetMap with attribution) · Geocoding service · Proxy (system / manual) · Log level.

### People

Find faces in my photos (on) · Suggest names (on) · Suggestion confidence (85) · Grouping
confidence (85) · Store people names in photos (off) · Face model: status + [Download…] / [Remove].

### Photo Viewer

As [`viewing.md`](viewing.md) §3.3.

### Export

Default export location · Default size and quality · Remove location from exported photos (off) ·
Default watermark text.
