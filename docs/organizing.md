# Organizing: folders, import, albums, stars, tags, people, places, search

Behaviour spec for everything that organizes photos. Layout of the regions mentioned here is in
[`ui-layout.md`](ui-layout.md); storage is in [`data-model.md`](data-model.md); menu items and
shortcuts in [`menus-and-shortcuts.md`](menus-and-shortcuts.md). Parity ids are in
[`parity.md`](parity.md).

## 0. Mental model (applies everywhere)

- Dalica **indexes; it does not import into a library.** It shows the real files in the folders the
  user chose. Scanning never copies or moves anything.
- **Folders** are real directories. **Albums** are virtual, ordered lists of references; a photo
  lives in exactly one folder but can be in any number of albums. Removing from an album never
  touches the file; deleting a file removes it from every album.
- A folder appears in the library only if it contains at least one displayable item (directly or in
  a subfolder in tree view). Empty folders disappear.
- Almost every batch action works on **the selection, or the Photo Tray** when items are held in it
  (see `ui-layout.md` → Photo Tray). The tray is how the user gathers photos from several folders.
- Every change is undoable (Edit → Undo) unless the dialog says it cannot be.

## 1. First run

1. Before the main window is usable, a modal first-run panel (≈ 640 × 460) appears if no library
   exists:
   - Heading: **"Dalica is ready to look for pictures on your computer"**.
   - Radio (default): **"Only look in my Pictures, Documents and Desktop folders"** — hint
     "Choose this if you keep your photos in these folders."
   - Radio: **"Choose folders…"** — hint "Pick the folders you want Dalica to watch. You can change
     this at any time in the Folder Manager." Choosing it opens the Folder Manager after Continue.
   - Footer: "Looking for pictures never moves or copies files. You can change which folders are
     shown at any time with Tools → Folder Manager."
   - Single button **Continue**.
   - Built (M5.18): the panel is offered by the window when it opens a library that has never had
     a folder (`Session::is_new_library`); headless and snapshot sessions never show it, and the
     demo library is never new. Under the first choice it lists the places it will add (with their
     icons), read on the engine's workers (`folders.browse`); Continue adds the existing Pictures,
     Documents and Desktop folders (never the home folder itself, never a folder inside another)
     as Scan Always with one `folders.set`, one undo step. When none of them exists, Continue opens
     the Folder Manager, as "Choose folders…" does. Ids: `dialog:firstRun`,
     `choice:dialog.start/places`, `choice:dialog.start/choose`, `button:dialog.continue`; the
     draft is `{choice}`.
2. The main window opens immediately; folders **appear in the library tree as they are found** and
   their thumbnails fill in progressively. The activity indicator at the right end of the toolbar
   (`ui-layout.md` §3) and a status line message ("Scanning: 1,204 pictures found") show background work. The user can browse,
   star, and open Folder Manager while scanning continues.
3. After scanning, face detection runs as a second, lower-priority background pass (if enabled);
   progress shows in the People group header ("Finding faces… 312 of 4,980").
4. When folders with the legacy organizer's sidecars are found during the first scan, their stars,
   captions, edits, albums and people show up right away (no import step) and a non-modal notice
   says once: **"Found photo data from your previous photo program in %d folders. Dalica shows it
   and saves any changes in its own files; the old files are never changed."** [OK] (see
   `data-model.md` §9).

## 2. Folder Manager (Tools → Folder Manager…, also File → Add Folder…)

Both entries open the same dialog. When opened with a folder selected in the library, that folder
is pre-selected in its tree.

Layout: resizable modal, initial ≈ 640 × 480, split 50/50.

- **Left: "Folders"** tree. Roots: Pictures, Documents, Desktop (with their icons), Home, then each
  mounted volume and network mount. Each row: disclosure triangle, **state icon**, folder icon,
  name. Lazily loaded on expand (on a worker; a spinner row while loading).
  - State icons: **Scan Always** (circular arrows, `accent`), **Scan Once** (check mark,
    `text_label`), **Remove from Dalica** (×, `danger`). Every row shows its effective state;
    subfolders inherit unless explicitly set (inherited icons are drawn at 50 % opacity).
- **Right:**
  - Text: "For each folder, choose whether Dalica looks for pictures in it. Watched folders are
    checked for new pictures automatically."
  - Group **"For the selected folder:"** with three radio buttons, each with its icon:
    **Scan Always**, **Scan Once**, **Remove from Dalica**. Disabled until a tree row is selected.
  - Separator, then toggle **"Find faces in this folder"** (on/off; applies to subfolders).
  - **"Watched folders"** list: folders explicitly set to Scan Always, shown with friendly names
    ("Pictures", "Desktop") or full paths. Clicking one reveals and selects it in the tree.
- Buttons bottom-right: **OK** (default), **Cancel**, **Help**. Changes apply only on OK. Esc =
  Cancel.

Semantics: see `data-model.md` §7. Confirmations:
- Setting a volume root or the home folder to Scan Always asks: "Watching a whole drive can slow
  down your computer and fill the library with non-photo images. It's usually better to choose a
  few folders. Watch the whole drive anyway?" [Watch Drive] [Cancel]. Cancel reverts the radio.
- OK after turning face finding off for folders that have named faces: "Remove the face
  rectangles found in these folders? Names you've given stay with the photos that are still
  scanned." [Remove Faces] [Keep Faces].
- The folder context-menu command **Remove from Dalica…** (different entry point, same effect as
  the radio) confirms: title "Remove Folder", text "Remove the folder "%s" and its subfolders from
  Dalica? The files stay on your disk." [Remove Folder] [Cancel].

How it is built (M5.18; `crates/ui/src/dialogs/folder_manager.rs`, its tests
`crates/ui/tests/folder_manager.rs`):

- The dialog is ≈ 640 × 480, fixed (it fits the smallest window, 900 × 600); a resizable frame
  waits for the dialog widget to grow one. Its draft (the dialog's fields: the selected folder,
  the expanded folders, the changes by folder, the drives confirmed) is plain data; nothing reaches
  the library before OK, and OK with no change runs nothing.
- Roots are the standard places that exist, then the volumes (a volume that is also a place is
  listed once). A folder's subfolders are read only when it is expanded, through `folders.browse`
  on the engine's workers: the frame never waits, a spinner row ("Loading…") stands in until the
  answer arrives, and each folder is read from disk again the first time it is listed after the
  dialog opens. A folder whose listing comes back empty loses its triangle. Double-clicking a row
  expands or collapses it.
- A row's state is its effective mode with the draft applied: the nearest folder at or above it
  that sets one, else Remove. Clicking a radio sets the selected folder's own mode, also when it
  only inherited that mode; a change back to what the library already sets is dropped from the
  draft.
- The Watch Drive question opens over the Folder Manager when Scan Always is chosen for a volume
  root (a path with no parent, or a volume the roots listed) or the home folder that is not
  watched yet. Watch Drive sets the radio and marks the folder confirmed, so OK sends
  `"confirm": true`; Cancel or Esc leaves the radio as it was. The same question guards
  `folders.set` run from anywhere in the app while the Folder Manager knows the drives.
- Remove from Dalica… (`folder.remove`) asks Remove Folder through the dialog host, naming the
  folder; over automation the question is `needs_confirm` with the generic text "Remove this
  folder and its subfolders from Dalica? The files stay on your disk."
- The Remove Faces question waits for faces (M13). Help stays disabled until there is help.
- A dialog under another one keeps its buttons as parts of its own id (`dialog:folderManager/ok`),
  so `button:dialog.<action>` always names the top dialog's.
- Ids: the tree's rows `choice:dialog.folder/<root>` (a root) and
  `choice:dialog.folder/<root>/<folder>`, where each part is sixteen hex digits of the FNV-1a hash
  of the path; their triangles `button:dialog.expand/…` with the same parts; a spinner row
  `choice:dialog.folder/…/loading` (painted, under the folder being read); the radios
  `choice:dialog.mode/always`, `/once`, `/remove`; the faces toggle `check:dialog.faces`; the
  watched folders `button:dialog.reveal/<folder>`; the questions `dialog:watchDrive` and
  `dialog:removeFolder` with `button:dialog.watchDrive` and `button:dialog.removeFolder`.
- The state icons and the roots' icons are drawn in code (`FolderIcon` in `dalica-ui-kit`):
  circular arrows, check mark and cross; a framed picture, a page, a monitor, a house, a drive and
  a network drive.

## 3. Import (Import button, File → Import From…, Ctrl+M)

Import opens as a **second tab** next to the "Library" tab at the top of the content area (tab
"Import" with a close ×). It is not a modal dialog: the user can switch back to the Library tab while
files load.

Layout, top to bottom:

1. **"Import from:"** dropdown (top-left): removable volumes ("Removable drive (E:)" / volume name), **"Folder…"** (opens a folder picker),
   recent sources. Cameras and phones over MTP/PTP are added in the OS-integration milestone
   (`product.md` → Scope).
2. **Import tray** (left/main area, the full width minus the preview): a status line ("Loading 112
   of 340 files…", "340 files", "No pictures found"), a checkbox **"Exclude duplicates"**
   (tooltip "Skip pictures that are already in your library"; default from Options →
   General → "Detect duplicates while importing", on). Thumbnails appear **one by one as they
   load**, grouped by capture time: a group header per gap of more than 2 hours, with a clock icon
   that selects the whole group. Duplicates and excluded items show a `danger` ⊘ overlay and are
   dimmed.
3. **Preview** (right, ≈ 35 % width): the selected item large, header "filename · date time ·
   size"; buttons below: star toggle, exclude/include toggle (also the **X** key), previous/next,
   rotate left/right. Context menu on thumbnails: Exclude, Include, Exclude All, Include All.
4. **Destination strip** (bottom):
   - **"Import to:"** dropdown — default from Options → General → "Save imported pictures in"
     (default: the user's Pictures folder), plus recent destinations and **Choose…**.
   - **"Folder name:"** combo — free text, or **"Date taken (YYYY-MM-DD)"** (one subfolder per
     capture date), **"Today's date"**, plus recently used names. An existing folder name imports
     into that folder (an improvement over the reference, which could not).
   - **"After copying:"** (only for removable/camera sources): **"Leave files on the card"**
     (default) or **"Delete copied files from the card"**.
5. Hint line with a light-bulb icon, state dependent: "Choose a folder name to continue" → "Click
   Import All or Import Selected" / "Each capture date gets its own folder" / "Pictures will go
   into a folder named with today's date".
6. Buttons bottom-right: **Import All** (primary), **Import Selected (n)**, **Cancel**. Import
   buttons are disabled until a folder name is set.

Import flow: copy each file to the destination (verify by size + hash, keep the original
modification time, never overwrite: collisions get `-001`), then add the folder to the watched set
if it isn't covered already. Progress in the import tab ("Copying 45 of 340 at 38 MB/s") and in the
status line; Cancel stops after the current file (already-copied files stay). Then:
- "Delete copied files from the card" asks: "Remove the imported files from the card? This cannot
  be undone." [Remove Imported Files] [Keep Files].
- Finish: the Import tab closes, the library selects and scrolls to the new folder (it appears in
  **Recently Updated**, see §4.1), and a toast "Imported 340 pictures into 2026-06-04".

## 4. Albums

### 4.1 Kinds of albums (left tree, "Albums" group)

| Kind | Icon | Contents |
|---|---|---|
| User album | book | user-chosen photos, manual order |
| **Starred Photos** | book with star (automatic) | every starred photo, by date |
| **Recently Updated** | book with clock (automatic) | photos added or changed in the last 30 days (setting) |
| **Search results for "x"** | temporary, highlighted | the current search; disappears when the search is cleared |
| **Exported**, **Collages**, **Video Frames** | project folders | real folders under `Pictures/Dalica/<name>` that Dalica creates; listed in a "Projects" group in flat view |

### 4.2 Create an album

- File → **New Album…** (Ctrl+N), the **New Album** button in the toolbar, the tray's
  **Add to Album ▾** → **New Album…**, or a thumbnail's context menu → Add to Album → New Album….
- Opens **Album Properties**: **Name** (default "New Album", selected), **Date** (+ checkbox
  **"Use the date of the newest photo"**, on), **Place** (optional), **Description** (optional),
  [OK] [Cancel]. On OK the album is created with the current selection/tray contents (an empty album
  is allowed) and selected in the tree.

### 4.3 Add, remove, order

- Add: drag thumbnails onto an album in the tree (dragging onto a *folder* moves files — the drop
  target highlight and the drag pill say "Add to album" vs "Move to folder"); tray **Add to Album ▾**
  → album list; context menu → Add to Album ▸.
- Inside an album, Delete and the context menu read **Remove from Album**; confirmation (unless
  Options → "Remove from albums without asking"): title "Remove from Album", "Remove the %d
  selected pictures from this album? The files are not deleted." [Remove] [Cancel].
  Ctrl+Delete deletes the file from disk (with the delete confirmation).
- Order: drag thumbnails within an album; order is **persisted** (one position key per member in its
  folder sidecar, `data-model.md` §3.2, mirrored in `albums.json`). Album context menu → Sort Album By ▸ Date / Name / Size / Reverse Order rewrites the
  manual order. Album cover: context menu on a photo in the album → **Set as Album Cover**
  (default: first photo).
- Delete album: album context menu → **Delete Album…** — "Delete the album "%s"? The pictures in it
  are not deleted." [Delete Album] [Cancel].
- Album Properties: double-click the album name or its header, or context menu → Album Properties….

## 5. Stars, tags, captions

### 5.1 Stars

- A photo is either starred or not (no 1–5 ratings).
- Toggle: the **star button** in the bottom bar (library and edit view), **Ctrl+8** everywhere,
  **Space** in the edit view, or Picture → Star (full list in `menus-and-shortcuts.md`).
- Starred thumbnails show the star badge (bottom-right). The **Starred Photos** album and the
  **Show only starred** filter use it. Edit → **Select Starred** / the folder header's star button
  select the starred photos of that folder.

### 5.2 Tags

- **Tags pane** (right side pane; toolbar/pane button with a tag icon, View → Tags, Ctrl+T), header
  "Tags" with collapse « and close ×:
  - **"Add a tag:"** field + **+** button (Enter adds; commas separate several tags). Autocompletes
    existing tags.
  - **"Tags in the selection:"** list; a tag present on only some selected photos shows a count
    "beach (3 of 8)"; hovering shows **×** to remove it from all selected photos; context menu: Add
    to All Selected, Search for This Tag, Remove from Selected.
  - **"Quick tags"**: a grid of 10 buttons (rows of 2/3/2/3), unassigned ones show "+"; a gear
    opens **Configure Quick Tags** (checkbox "Use the top two buttons for recently used tags", on;
    10 text fields; [Fill Empty with Common Tags]; [OK] [Cancel]). Clicking a quick tag toggles it
    on the selection. Keyboard: **Ctrl+Alt+1 … Ctrl+Alt+0** apply quick tags 1–10 (toggling them
    too). With the recent-tags option on, buttons 1 and 2 show the two tags most recently added
    through "Add a tag:" (or Add to All Selected) that are not on buttons 3–10, empty until there
    are such tags; clicking quick tags and toggling do not reorder them, so the buttons stay put.
- Tag text: commas separate tags typed into "Add a tag:"; each is trimmed, and a tag that differs
  from one in use only in case takes the spelling in use. A tag is at most 256 bytes; longer ones
  are refused, not cut. Rename Tag onto an existing tag merges the two (one undo step, asking
  first).
- Tags apply to the selection (any folders); show them under thumbnails with View → Thumbnail
  Caption → Tags.
- Tags are written into JPEG/TIFF/PNG metadata (`data-model.md` §5).

### 5.3 Captions

- Edit view: a caption field under the photo with placeholder **"Add a caption"**; click, type, Enter
  or click away commits; the × at the right clears it; a toggle at the left shows/hides captions
  over photos in slideshows and the viewer.
- Library: View → Thumbnail Caption → Caption shows captions under thumbnails; Picture → Edit
  Caption edits the caption of the focused photo inline under its thumbnail.
- Captions are searchable, shown in slideshows and the viewer, and written to file metadata.

## 6. People (face recognition)

Face detection and recognition run locally (pure-Rust models downloaded by the user on first use
with consent; see [`faces.md`](faces.md) §3). Nothing leaves the machine.

### 6.1 Settings

Options → People: **Find faces in my photos** (on), **Suggest names** (on) with
**Suggestion confidence** and **Grouping confidence** sliders (50–95 in steps of 5, default 85),
**Store people names in photos** (off; writes XMP regions). Per-folder exclusion in the Folder
Manager.

### 6.2 The People group and Unnamed

- The left tree has a **People** group between Albums and Folders: **Unnamed (n)** first, then one
  entry per person with a count; a `caution` "?" badge marks people with pending suggestions.
- Selecting **Unnamed** shows a face grid on the light table: header "Unnamed people" with
  [Expand Groups] and [Show Ignored Faces]; faces cropped and grouped in clusters of similar faces;
  each cluster has an **"Add a name"** field under it and an × (ignore) at its top-right.
- Typing a name and Enter names the whole cluster (autocomplete over existing people). A new name
  asks **New Person**: Name (prefilled), Nickname (optional), [OK] [Cancel].
- Ignore: × or Delete moves faces to Ignored (confirmation "Ignore these faces?" [Ignore]
  [Cancel] + "Don't ask again").
- Faces can be multi-selected (click, Ctrl/Shift-click, rubber band) and dragged onto a person in
  the tree.

### 6.3 Person albums and suggestions

- Each person behaves as an album of the photos containing them (virtual).
- Person header buttons: **Zoom to faces** / **Show whole photos** toggle, **Set as Person
  Thumbnail**, Play slideshow, Collage.
- Suggestions: a strip "Suggestions:" with [Confirm All], [Confirm], [Reject], [Find More], toggle
  "Show only suggestions", hint "Click ✓ to confirm a match or × to reject it."; each suggested face
  has ✓ and × buttons.
- Wrong face: select it → Delete / **Remove from Person** (back to Unnamed). Context menu on a face:
  Move to New Person…, Move to Person ▸, Remove from Person, Set as Person Thumbnail. Context menu on
  a person: Rename…, Merge Into ▸, Delete Person (faces return to Unnamed; confirmation).

### 6.4 People pane (right side pane)

- Toggle with the people button in the side-pane switcher or View → People. Sections: "People in
  these photos" (named, with counts), "Unnamed faces" (faces with Add a name / ×), suggestions shown
  as "Name?" with ✓/×.
- Hovering a face in the pane highlights its rectangle on the photo (edit view) and vice versa.
- **Add a Person Manually** (edit view only): draws an adjustable rectangle on the photo with the
  instruction "Drag the box to fit the face, then type a name." → name field.
- Selecting several people in the pane (Ctrl-click) filters the light table to photos containing
  all of them, with the filter banner ("Showing photos with Ana and Rui" + [Show All]).

## 7. Places (geotagging)

- **Places pane** (right side pane; pin button, View → Places): header "Places"; status "No places
  in the selection" / "9 places in the selection"; a map (OpenStreetMap-compatible tiles fetched
  only when the pane is open, with an attribution line; a preference can disable network maps,
  leaving a coordinates-only pane); zoom +/−; a pin tool; **"Search for a place:"** field at the
  bottom (geocoding through a configurable service; offline: coordinates only).
- Geotag: drag photos from the light table onto the map, or search a place and click the pin →
  bubble **"Put %d photos here?"** with a thumbnail stack, [OK] [Cancel]. Moving already-tagged
  photos asks "Move %d photos here?". A marker's bubble: "%d photos here", [Show These Photos],
  [Remove Location].
- Confirmations when changing the location of more than 20 photos, or removing it from more than 5.
- The map shows only the photos of the current folder/album/selection.
- Geotagged thumbnails show the pin badge; the **Show only geotagged** filter uses it.
- Folder/album **Place** (Properties) is a free-text field, searchable, distinct from geotags.

## 8. Search and filters

### 8.1 Search box (toolbar, right)

- **Instant**: results narrow on every keystroke (debounced 120 ms, computed on a worker for big
  libraries). Matches words (prefix match on tokens split at space, dot, underscore, dash) in file
  names, captions, tags, people names, folder and album names, places, descriptions, dates (month
  names, years, `2026-06`), camera make/model, and dimensions. Several words = AND. Operators:
  `-word` (NOT), `tag:word`, `person:name`, `camera:x`, `color:green` (a later milestone, decided at
  the M9 review; dominant colour).
- Autocomplete dropdown lists matching tags, folders, albums and people with counts.
- Results: only matching folders/albums/photos are shown; a **filter banner** across the top of the
  light table: [**Show All**] + "Showing 33 pictures in 4 folders (0.01 s)"; a temporary album
  **Search results for "x"** at the top of Albums. Clear with the × in the box, Esc in the box, or
  Show All.
- Tools → **Save Search as Album…** creates a user album from the results.

### 8.2 Filter buttons (toolbar, left of the search box)

Toggle buttons, combinable with each other and with text: **Starred**, **Faces** (photos with
faces), **Videos**, **Geotagged**. Then the **age slider**: one handle, leftmost = everything;
moving right shows only newer items, label "Pictures up to 3 months old" (days/weeks/months/years
on a logarithmic scale; the end is always today). Any active filter shows the filter banner.

### 8.3 Duplicates

Tools → **Find Duplicates**: a search mode showing only groups of identical files (same size and
content hash; later: plus visually near-identical) side by side, with the banner "12 duplicate groups
(31 files)".

## 9. File operations

All act on real files; all are commands with undo where possible (`data-model.md` §8).

| Operation | How | Dialog |
|---|---|---|
| Move photos | drag onto a folder in the tree; Edit → Cut / Paste | first time: "Move these files on disk? The files will be moved, not copied." [Move] [Cancel] + "Don't ask again"; name clash: "The folder already has files with the same names." [Rename Duplicates] [Skip Duplicates] [Cancel] |
| Copy photos | drag with Ctrl held (drag pill shows +); Edit → Copy / Paste | "Copy %d files to %s?" [Copy] [Cancel] |
| Move to new folder | context menu → **Move to New Folder…** | Folder Properties (Name "New Folder", Date + "Use the date of the oldest photo", Place, Description) — created next to the source folder |
| Split folder | context menu on a photo → **Split Folder Here…** | Folder Properties; moves that photo and every photo after it |
| Move folder | folder context menu → **Move Folder…** | folder picker |
| Rename folder | double-click folder name / Folder Properties | renames on disk |
| Rename photos | File → **Rename…** (F2) | **Rename Files**: "%d files selected. Enter a new name:" field, example line "Example: Lisbon-001.jpg", checkboxes **Add the date taken to the name**, **Add the image size to the name**; [Rename] [Cancel]. Several files get `-001`, `-002` … in display order |
| Delete photos | Delete (in folders) / Ctrl+Delete (anywhere) / File → **Delete from Disk…** | title "Delete", "Move the selected picture to the trash?" / "Move the %d selected pictures to the trash?" [Move to Trash] [Cancel] (no trash available: "Permanently delete…? This cannot be undone." [Delete]); skipped if Options → "Delete without asking" |
| Delete folder | folder context menu → **Delete Folder…** | "Move the folder "%s" and everything in it (including non-picture files) to the trash?" [Move to Trash] [Cancel] |
| Hide / Unhide | Picture → Hide (context menu) | none; hidden items show only with View → Hidden Pictures (drawn at 40 % opacity) |
| Hide folder | folder context menu → Hide Folder | folder moves to a **Hidden Folders** group visible with View → Hidden Pictures (no password protection) |
| Locate on Disk | context menu, Ctrl+Enter, clicking the folder icon in a folder header | opens the system file manager with the item selected *(OS integration)* |
| Open with | context menu → Open With ▸ (system handlers), Open in External Editor (Ctrl+Shift+O) | *(OS integration)* |
| Copy Path | context menu | |
| Refresh thumbnails | folder context menu | regenerates the folder's thumbnails |
| Adjust date and time | Tools → **Adjust Date and Time…** | **Adjust Picture Date** (title "… — %d pictures" for several): thumbnail, "Current date:" (read-only), "New date:" date + time pickers; with several: radios **"Shift all dates by the same amount"** (default) / **"Set all pictures to the same date and time"**; [OK] [Cancel] |
| Batch edit | Picture → Batch Edit ▸ | see `editing.md` → Batch edit |

### 9.1 Photo context menu (light table thumbnail), in order

**View and Edit** (bold, Enter) · Add to Album ▸ · — · Rotate Clockwise · Rotate Counterclockwise ·
— · Undo All Edits · — · Hide · — · Move to New Folder… · Split Folder Here… · — · Open in External
Editor · Open With ▸ · — · Save · Revert… · — · Locate on Disk · Delete from Disk… · Copy Path · — ·
Find Faces Again · — · Properties (Alt+Enter). In an album: Delete from Disk becomes **Remove from
Album**, and **Show in Folder** is added (selects the photo in its folder).

### 9.2 Folder context menu (tree row, folder header, empty light-table area)

Folder Properties… · — · Select All · Select None · Invert Selection · — · Refresh Thumbnails ·
Sort Folder By ▸ (Date, Name, Size, Reverse Order) · — · Hide Folder · — · Locate on Disk ·
Remove from Dalica… · — · Move Folder… · Delete Folder… · — · Save Edited Pictures. Album
variant: Album Properties…, Delete Album…, Select All/None/Invert, Refresh Thumbnails, Sort Album
By ▸.

## 10. Sorting and folder metadata

- **Library tree view mode** (View → Folder View ▸ or toolbar buttons): **Flat** (default: every
  folder as one row under "Folders", plus groups such as Projects and Hidden Folders) or **Tree**
  (disk hierarchy under each watched root; groups are only Albums, People, Folders).
- **Folder sort** (View → Folder View ▸): **By Date** (default; newest first; a folder's date is
  its oldest photo's capture date unless overridden), **By Name**, **By Size**, **By Recent
  Changes**, **Reverse Order**. People: by name or by count.
- Groups ("Albums (12)", "People (40)", "Folders (310)") collapse with a disclosure triangle; the
  light table only shows sections of expanded groups.
- **How the folder sorts read** (the tree model, `crates/library/src/tree.rs`): By Size is the
  total bytes of the photos a folder shows, largest first; By Recent Changes is its newest photo's
  modification time; By Name ignores case. Folders without any date stay first under the header in
  both directions. In tree mode a directory sorts by its whole subtree (the oldest photo below it,
  the newest change, the total size), shows only its own count, and folders under no watched root
  are tops of their own. A group's header count is its folder rows, including those folded away
  under a collapsed tree node; collapsing a tree node hides rows only, while collapsing a group
  also drops its light-table sections.
- **Other** (flat mode) holds folders where more than half of the photos are small pictures; with
  View → Small Pictures off it shows only their other photos, and a folder with nothing left to
  show disappears like an empty one.
- **Photos within a folder:** default by capture date, oldest first (falling back to file name);
  Sort Folder By ▸ Date / Name / Size / Reverse; dragging thumbnails sets a persistent manual
  order (stored in the folder sidecar).
- **Folder Properties** (double-click the folder name or header, or the context menu): **Name**
  (renames on disk), **Date** + **"Use the date of the oldest photo"**, **Place** (optional),
  **Description** (optional), **Music for slideshows** (path + Browse…), [OK] [Cancel].
  All fields are searchable; the description shows in the folder header. The date is typed as
  `YYYY-MM-DD` (the long date shows beside it; OK is greyed while the text does not read as a
  day); while the box is ticked the field shows the oldest photo's date. Name and Music stay
  greyed until renaming (M9) and slideshows (M11) arrive.

## 11. Open questions (reserved for the maintainer)

1. Whether `color:` search ships in M9 or later.
