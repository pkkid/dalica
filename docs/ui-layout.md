# UI layout

The window structure and interaction model of Dalica. This is a **layout** spec: positions, sizes,
ordering and behaviour of regions. Colours, fonts and widget styling come from
[`design-system.md`](design-system.md) (dark, muted greys); never take colours from the reference
screenshots. Edit view details are in [`editing.md`](editing.md); viewer and slideshow in
[`viewing.md`](viewing.md); create tools in [`creating.md`](creating.md).

Sizes are in points at 100 % UI scale. "W" is the window width.

## 1. The core workflow (read this first)

1. The **light table** (the big thumbnail area) is **one continuous, scrolling list of every
   visible folder and album**, each shown as a *section*: a header block followed by its thumbnail
   grid, in the same order as the library tree on the left.
2. **Clicking a folder or album in the tree scrolls the light table so that section's header is at
   the top** (animated, 0.2 s). It does *not* filter the light table to that folder. Clicking it
   again returns to the top of that section.
3. Clicking a folder/album name also **selects the whole folder as a unit**: the Photo Tray shows a
   single token "Folder selected — 48 photos", the status line summarises it, and actions (Export,
   Collage, star, rotate…) apply to all its photos.
4. Photo selection in the light table is **scoped to one section at a time**: selecting in another
   folder replaces the selection. To gather photos from several folders, press **Hold** (Ctrl+H)
   first; held photos stay in the tray while you select elsewhere.
5. Bottom-bar action buttons always act on **the Photo Tray contents** (current selection + held
   items). Section-header buttons act on **that section**.
6. Double-click a thumbnail (or Enter, Ctrl+3) opens the **edit view**; Back to Library / Esc
   returns to the same scroll position with that photo selected.
7. Search and filters put the light table in **search mode**: a banner with **Show All** appears,
   only matching sections/photos remain, and the results also appear as a temporary album.

## 2. Window anatomy (library view)

```text
┌────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ File  Edit  View  Folder  Picture  Create  Tools  Help                                                 │ menu bar 32
├────────────────────────────────────────────────────────────────────────────────────────────────────────┤
│ [📷 Import][+📘][≣|⊢][▾]                  Filters                ┌─🔍───────────────────────────────┐ ◌   │ toolbar 40
│                                     [★][☺][🎞][📍] ○────────     │ Search                         ⓧ │     │
├───────────────────────────┬┬──────────────────────────────────────────────────────────────┬─┬───────────┤
│ ▾ Albums (3)              ││ ┌─────────────────────────────────────────────────────────┐   │⇑│ « People ×│ side pane
│   ★ Starred Photos (12)   ││ │[📁] Lisbon Weekend                         3 edited [Save]│  │▲│ (280,     │ (optional)
│   ◷ Recently Updated (40) ││ │     Thursday, June 4, 2026 · Lisbon, Portugal            │   │ │  shown    │
│   📘 Best of 2026 (40)    ││ │ [▶][▦] [★] [💾] [Export ▾]                                │   │ │  when a   │
│ ▾ People (8)              ││ │ Add a description                                        │   │ │  pane is  │
│   ☺ Unnamed (112)         ││ └─────────────────────────────────────────────────────────┘   │ │  toggled) │
│   ☺ Ana Costa (40)        ││   ┌────┐ ┌──┐ ┌────┐ ┌────┐ ┌──┐ ┌────┐                       │ │           │
│ ▾ Folders (174)           ││   │    │ │  │ │ 📍★│ │    │ │  │ │ 1:23│                       │ │           │
│   2026 ────────────────── ││   └────┘ └──┘ └────┘ └────┘ └──┘ └────┘                       │ │           │
│   📁 Lisbon Weekend (48) ◀││   (next section header …)                                     │▼│           │
│   📁 Garden (120)         ││                                                               │⇓│           │
├───────────────────────────┴┴──────────────────────────────────────────────────────────────┴─┴───────────┤
│              48 pictures     June 4, 2026 to June 6, 2026     212.4 MB on disk     Tags: lisbon (40)     │ status 20
├──────────────────────────────────────────────┬─────────────────────────────────────────────────────────┤
│ ┌──────────────────────────────────────┐ [📌]│ [☆][↺][↻]                    🔍──○───────  [☺|📍|🏷|ⓘ]   │ bottom
│ │ [▣] Folder selected — 48 photos      │ [⊘] │ ─────────────────────────────────────────────────────── │ area 96
│ └──────────────────────────────────────┘ [📘▾]│ [Export] ┆ [Collage] [Slideshow]                         │
└──────────────────────────────────────────────┴─────────────────────────────────────────────────────────┘
```

| Region | Size | Notes |
|---|---|---|
| Menu bar | 32 high (its last point a 1 pt `divider`) | in-window on Windows/Linux, where it is the title bar (`design-system.md` §11.1); native on macOS (generated from the same menu model) |
| Toolbar | 40 high | §3 |
| Tab strip (only when a second "room" is open) | 28 high | §8 |
| Library tree | default **240** wide, min 200, max W − 480; resizable; width persisted | §4 |
| Splitter | 8 wide, grip 8 × 48 centred | drag to resize the tree |
| Light table | the rest | §5 |
| Light-table scrollbar | 16 wide | ⇑ previous section · ▲ · track · ▼ · ⇓ next section (§5.6) |
| Side pane | **280** wide (toggle «: large = min(30 % W, 392)) | §7; pushes (narrows) the light table, never overlays it |
| Status line | 20 high | §6.1 |
| Bottom area | **96** high | §6 |

Window: default 1400 × 900, minimum 900 × 600; position, size, maximised state, tree width, side
pane open/closed and which pane, thumbnail size, view modes are persisted. **F11** = full screen.

## 3. Toolbar (library view)

Left → right:

| # | Control | Size | Tooltip | Action |
|---|---|---|---|---|
| 1 | **Import** (camera icon + "Import") | 110 × 28 | "Get photos from a camera, card or folder" | opens the Import tab (`organizing.md` §3); Ctrl+M |
| 2 | **New Album** (album + plus icon) | 30 × 28 | "Create a new album" | File → New Album (Ctrl+N) |
| 3 | **Flat** \| **Tree** segmented pair | 30 × 28 each | "Show folders as a flat list" / "Show the folder tree" | library tree mode (`organizing.md` §10) |
| 4 | **▾** view options | 22 × 28 | "View options" | popup: Sort by Date ✓ · Sort by Recent Changes · Sort by Size · Sort by Name · Reverse Order · — · Sort People by Name · Sort People by Count · — · Show Thumbnails in Tree · Simplified Tree |
| — | flexible space | | | |
| 5 | **Filters** group | | the caption "Filters" (11 pt `text_dim`) above the buttons is replaced by the hovered button's tooltip | toggles: **Starred** ("Show starred photos only"), **Faces** ("Show only photos with faces"), **Videos** ("Show videos only"), **Geotagged** ("Show only geotagged photos"); 26 × 26 each, joined |
| 6 | **Age slider** | 100 wide | "Show only recent photos" | `organizing.md` §8.2 |
| 7 | **Search** field | from 0.4·W to W − 64 | placeholder "Search" | live search (`organizing.md` §8.1); Ctrl+F / F3 focuses it |
| 8 | **Activity indicator** | 28 × 28 | current background job | spinner while scanning/exporting/finding faces; click → popover listing jobs with progress and Cancel |

The search field and filters move right as the tree is widened (they belong to the light table's
column): the filter group starts at least 8 right of the splitter, so with a wide tree or a narrow
window the search field starts right of the filters (12 after the age slider) rather than at 0.4·W,
and it never gets narrower than 160. The caption sits above the toggles (which then rest on the
toolbar's bottom rule); without it the toggles are centred like the other controls.

The **activity indicator** (`indicator:activity`) spins while a background job runs (today a scan
or a rebuild); at rest it draws nothing and takes no click (its tooltip says "No background work").
A click toggles the **activity popover** (`popover:activity`; `ui.state` → `activity`): the popup
frame (`chrome`, `button_border`, radius 6, popup shadow), 360 wide (less in a narrow window, 8 from its edges), its
right edge under the indicator's and 4 below it, 8 of padding. It lists the jobs of `jobs.list`,
running ones first (oldest first), then finished ones (newest first), at most 8 and a "N more" line
for the rest; each is a `progress_row` (`design-system.md` §6) of the job's label ("Scanning: 1,204
pictures found", "Scan finished: 4,980 pictures"), its bar (done of total; the sliding third while a
running job's total is unknown) and, while it runs, its × Cancel (`jobs.cancel`). Ids: the row
`job:<n>`, its Cancel `job:<n>/cancel`. With no job it reads "No background work". A press outside
the popover and the indicator, or Esc, closes it; it stays open when its last job ends, showing the
finished row.

## 4. Library tree (left pane)

- **Groups** (collapsible, each shown only when non-empty), in this order: **Albums**, **People**,
  **Projects** (flat mode only: Exported, Collages, Video Frames; the folder Create Collage writes
  into, `Pictures/Dalica/Collages`, is marked a Collages project and drawn with the project
  briefcase like an export folder),
  **Folders**, **Other** (flat mode only: folders of mostly tiny images), **Hidden Folders** (only
  with View → Hidden Pictures).
- **Group header**: 30 high, full width, disclosure triangle, SemiBold 13 title with count "Albums
  (3)". Collapsing a group hides its rows **and its sections in the light table**. During search the
  triangle is replaced by a magnifier glyph. Headers are sticky: a collapsed group's header stays
  docked at the bottom of the pane when the list above it is short (the collapsed groups after the
  last open one sit at the pane's bottom while the whole list fits, and follow the list once it is
  taller).
- **Rows**: 24 high; 16 pt icon (folder, album, automatic album, person face thumbnail 20 pt in
  28-high rows, project), name, count in `text_dim`; long names elide in the middle. The People
  group's rows are all 28 high: Unnamed (the viewfinder bust) first, then the people (their face, or
  the plain bust until it arrives), each person's `caution` "?" badge after the count while
  suggestions wait (`organizing.md` §6.2). Tree mode:
  16 pt indent per level with disclosure triangles, parents show their own count (often "(0)",
  dimmed). Flat mode sorted by date: **year dividers** (the year in `text_dim` 11 pt with a 1 pt
  rule to the right edge, 18 high), newest year first, only when folders span more than one year.
- **Selection**: selected row = `canvas` fill with `text` label (focused list: plus a 2 pt `accent`
  bar at the left edge). Hover = `hover × 0.6`.
- **Click** = scroll the light table to that section + select the folder as a unit (§1); on
  Unnamed, the Unnamed grid replaces the light table until another row is clicked.
  **Double-click** = Folder/Album Properties. **Right-click** = folder/album context menu (on a person: the person menu)
  (`organizing.md` §9.2). **Delete** on a folder = Delete Folder (confirmation).
- **Drop target**: dragging photos over a folder row highlights it with `drop_target` (drop = move;
  Ctrl = copy; drag pill text changes accordingly); over an album row = add to album; over a person
  = assign faces (from the Unnamed view). A row where a drop would change nothing (the photos' own
  folder, an album already holding them all) is no target. A drag held within 24 pt of the pane's
  top or bottom edge scrolls the rows, faster nearer the edge.
- Keyboard: ↑/↓ move the selection (scrolling the light table), ←/→ collapse/expand in tree mode.
  The keys are the tree's while it has the focus: a press in the pane gives it, a press anywhere
  else takes it back to the light table. ↑/↓ pass over rows without photos (a parent showing
  "(0)") and keep the selected row in view; ← on a row that is folded or has no children goes up
  to the nearest parent with photos. The wheel scrolls the pane; only the rows on screen are laid
  out, so its cost per frame does not grow with the library.
- **Scroll bar:** while the list is taller than the pane, an ordinary position scroll bar, 10 wide,
  sits at the pane's right edge (the rows end left of it): the track (`scroll:tree`, `inset`) and a
  thumb (`scroll:tree/thumb`) as tall as the share of the list on screen (at least 24) at the
  proportional place, a rounded pill in `track` (`thumb` hovered or held). Dragging the thumb moves
  the rows with it; a press in the track above or below it pages one pane height that way. A list
  that fits has no bar.
- **Offline folders:** the rows of folders under a watched root whose volume is offline
  (`data-model.md` §7) keep their place, count and section but are dimmed (icon, name and count in
  `text_disabled`) with an "offline" marker, 11 pt `caution`, at the row's right end (the name
  gives way, elided in the middle). They come back as they were when the root returns. Rows are marked once per tree build, never tested per frame.

## 5. Light table

### 5.1 Sections

Each visible folder/album is a section: **header block** + **thumbnail grid**. Sections follow the
tree order. 12 pt between the header block and the first row; 24 pt gutter between a grid's last
row and the next header. The scroll position is kept by content (the section at the top and the
offset below its top), so a resize, a new thumbnail size or a caption change keeps the same photos
in view; a scroll asked for from outside the view (a tree click, a scroll command) eases there over
0.2 s, ease-out.

### 5.2 Section header block

```text
[📁 32]  Lisbon Weekend                                   3 edited  [Save]
         Thursday, June 4, 2026 · Lisbon, Portugal
[▶] [▦] │ [★] │ [💾] [Export ▾]
Add a description
```

- Background `folder_header`; full light-table width; padding 12 left. The header of the
  currently selected section has a 2 pt `accent` bar at its left edge.
- **Icon** 32 pt (folder / album / automatic album / person). Tooltip on the folder icon = full
  path; clicking the icon = Locate on Disk.
- **Title** SemiBold 15 `text`; double-click = Properties. **Date** (+ " · place" when set) 12.5
  `text_dim`, long format ("Thursday, June 4, 2026").
- **Buttons** (26 × 22 icon buttons): **Play slideshow** ("Play a slideshow of this folder"),
  **Collage** ("Make a collage from this folder"), |
  **Select starred** ("Select the starred photos in this folder"; disabled when none), |
  **Save** ("Save edited photos to disk"; enabled when the section has unsaved edits), **Export**
  ("Export these photos to a folder"). Play/Collage/Export use the selection when the selection
  is inside this section, else the whole section (Collage passes `create.collage` one or the other:
  `ids`, else `section`; it opens the Collage room, `creating.md` §3).
- Top-right, only when the section has unsaved edits: "%d edited" (`text_dim`) and a **Save**
  text button (same as the 💾 button).
- **Description** line: the description (`text_label`) or the placeholder **"Add a description"**
  (`text_dim`); click to edit inline (Enter commits, Esc cancels).
- Right-click anywhere on the header = the folder/album context menu.
- **Damaged data file** (`data-model.md` §3): the header of a folder whose sidecar was damaged and
  kept aside carries a `caution` banner, right-aligned over the date and button rows (half the
  header's width, 300–600 pt, 42 high, clear of the description row): `filter_banner` fill, a 3 pt
  `caution` bar at its left, "This folder's data file was damaged and kept aside." (13 pt `text`)
  over "Nothing is written here until you reset it or restore the file." (13 pt `text_dim`), each
  cut short with "…", and a **Reset folder data** text button at its right (`banner:folderData/<FolderId>`,
  `button:folder.resetData/<FolderId>`). The button asks first: "Reset the data of the folder
  "%s"? Dalica writes a new data file from what it knows about these photos. The damaged file
  stays beside the photos, untouched." [Reset] [Cancel] (`dialog:resetFolderData`); Cancel changes
  nothing, Reset runs `folder.resetData` and the banner goes. Not on the pinned line; hidden while
  it would lie under it.
- **Sticky**: the header of the section being scrolled through stays pinned at the top of the
  light table (collapsed to its first line, 32 high) and is pushed up by the next header.
- Person sections: the person's face (32 pt) in the icon's place, their name, the newest photo's
  date, no description row; buttons **Play**, **Collage** (the person's photos, or the selection
  inside the section) | **Zoom to Faces** (a toggle drawn
  `pressed × 0.6` while on, its tooltip then **Show Whole Photos**) | **Set as Person
  Thumbnail** (the bust); a right-click opens the person menu. Zoomed, each cell is the person's
  face tile filling the square. The Unnamed section: **Expand Groups**, **Show Ignored Faces**.

### 5.3 Thumbnail grid

- Cells are squares of edge `t` (thumbnail size, slider §6.4: 64–320, default 144); each photo is
  scaled to fit its square, centred, keeping its aspect ratio (portraits narrower), never cropped,
  never rounded, with a subtle drop shadow (`black α90`, offset 2, blur 4).
- Columns: as many as fit with a minimum horizontal gap of 12; the remaining width is spread evenly
  so columns justify across the light table; vertical gap 22 (room for a caption line when
  captions are on). Left/right margins 24.
- **Caption** (View → Thumbnail Caption: None (default) · Filename · Caption · Tags · Date ·
  Resolution): one line centred under the cell, 10.5 pt `text_dim`, middle-elided; the line is
  16 high, directly under the cell, inside the 22 pt row gap (rows do not move when captions
  turn on).
- **Badges** (`design-system.md` §7): a right-aligned row inside the image's bottom-right corner in
  the order edited ✎ · geotag 📍 · held 📌 · star ★ (star rightmost); videos: a film-sprocket edge
  on the left side of the image and a duration pill "1:23" at its bottom-left (whole seconds, the
  nearest, at least 1; no pill while the length is unknown). A video with no poster frame yet is
  the **video tile**: the placeholder grey with a Play triangle centred, a third of its shorter
  side, in `text_dim`; the filmstrip draws the same tile with the sprocket edge (its 32 pt squares
  have no room for the pill). The edited mark
  shows on every photo with edits not yet saved into its file, a rotation or flip alone included:
  exactly the photos the section header counts as "%d edited", so it goes after a Save
  (maintainer, 2026-10-14).
- **Hidden** photos (only visible with View → Hidden Pictures) are drawn at 40 % opacity.
- **Small pictures** (< 64 px on both sides) are omitted unless View → Small Pictures.
- **Empty states**: no watched folders → "No pictures yet" + [Add a Folder…] [Import…]; search with
  no results → "No photos found", "Try fewer words or other filters." and its own primary
  [Show All] (`empty:search`, `button:empty.showAll`), which leaves search mode as the banner's
  Show All does (M10.52).

### 5.4 Selection

- **Click** selects one photo (2 pt outline: white on the anchor, `gray 170` on others; selected
  cell surround `cell_selected`); the tray mirrors the selection live.
- **Ctrl-click** toggles, **Shift-click** selects a range from the anchor, **rubber band** from empty
  space selects every thumbnail the band intersects. Pressing on a thumbnail and dragging starts a
  drag instead. With Ctrl the band adds to the selection; a click on empty space selects nothing.
  A band held at (or within 8 pt of) the view's top or bottom edge scrolls the view, faster the
  farther past the edge (120 pt/s plus 12 pt/s per point, at most 3000 pt/s), and keeps selecting
  the rows that come in; it stops at its section's top or end.
- Selection never crosses a section boundary; selecting in another section replaces it (unless
  items are held in the tray).
- Keyboard: arrows move (Shift extends); arrows stop at the section's ends; **Home/End** first/last
  photo of the section (Shift extends); **Ctrl+Home/End** top/bottom of the library; **Ctrl+A**
  select all in the section; **Ctrl+D** select none; **Ctrl+I** invert; **Ctrl+H** hold; **Enter**
  view and edit; **Space** star toggle in the library is not bound (Ctrl+8 is).
- With nothing selected, an arrow or Home selects the first photo of the **current section** (End
  its last): the section of the selection, else the folder selected in the tree, else the section
  at the top of the view. The photo an arrow reaches is scrolled into view (just below the pinned
  header; a first-row photo brings its section header into view). Ctrl+A / Ctrl+I / Select Starred
  act on the current section; a Shift range runs from the anchor, which keeps its white outline.
- Every gesture and key keeps the selection inside one section; only a script's `select.set`
  (`replace` or `toggle`) may name photos of several folders, and such a selection has no section.

### 5.5 Mouse

- **Hover**: after 600 ms a tooltip: filename, date taken, pixel size, file size.
- **Hold Ctrl+Alt while hovering**: full-screen preview of the hovered photo while held.
- **Double-click**: open in the edit view (videos open the video player in the edit view).
- **Right-click**: photo context menu (`organizing.md` §9.1); right-click on an unselected photo
  selects it first.
- **Drag** from the grid: reorder within the section (persisted manual order), onto a tree folder
  (move; Ctrl copies), onto an album (add), onto a person (assign faces), onto the tray (add), onto
  the map in the Places pane (geotag), out of Dalica to the file manager or other apps (file URIs;
  the receiver decides copy/move; OS-integration milestone). The drag pill shows the count and the operation ("Move 3 photos
  to Garden"). In a user album's section the gap nearest the pointer between two cells (the left or
  right half of a cell) shows the insertion marker; a drop there reorders the album's own photos,
  or adds others at that place. Esc during a drag ends it with nothing dropped.
- **Wheel** scrolls; **Ctrl+wheel** changes the thumbnail size.
- **Loupe**: press and drag from the magnifier icon left of the thumbnail-size slider; a round
  lens (≈ 104 pt) follows the pointer over the light table showing the photo under it at 100 %
  (fades in 0.15 s, out 0.4 s).

### 5.6 Scrollbar

16 wide on the right of the light table: **⇑ previous section** (16 × 24), **▲** (16 × 24), track
(click pages; page step = one thumbnail row + 90, animated), thumb, **▼**, **⇓ next section** (ids
`scroll:prevSection`, `scroll:up`, `scroll:track`, `scroll:thumb`, `scroll:down`,
`scroll:nextSection`). The ⇑/⇓ buttons scroll so the previous/next section header is at the top
(⇑ first snaps to the current section's start when mid-section; ⇓ after the last section goes to the
end). ▲/▼ move one thumbnail row. The four buttons and the track act on press and auto-repeat while
held (after 0.4 s, then every 0.06 s). Every step eases over 0.2 s like `section.scrollTo`, and
steps taken while one is still easing add up (each starts from where the last one was going). When
the content fits the view, the buttons are disabled and the track is empty (no thumb, no marker).

The thumb sets the scroll **speed**, not the position (maintainer decision, 2026-10-10, matching
the reference behaviour):
- At rest the thumb (16 × 64) sits at the **centre of the track**, whatever the scroll position.
- Dragging it up or down scrolls the light table continuously in that direction, **faster the
  further the thumb is from the centre**: a small dead zone (4 pt) does nothing, then the speed is
  `top · u^exponent` screen heights per second, `u` the share of the way from the dead zone to the
  end of the track, so it reaches its top speed at the end of the track. The curve and top speed
  were tuned by feel at the M4 and M5 reviews and kept (quadratic, top speed 40 screen heights per
  second; the maintainer's answer at M5.R, 2026-10-12).
  Both are read from the library's `settings.json`, so they can be tuned without a rebuild:
  `"scrolling": {"thumbExponent": 2, "thumbTopSpeed": 40}` (exponent 0.25–8, top speed 0.5–400;
  a value outside its range is the default).
- On release the view stops where it is and the thumb springs back to the centre (150 ms,
  ease-out).
- The scroll **position** is shown by a separate faint marker (16 × 3, `text_dim` at 40 %) in the
  track at the proportional position; it cannot be dragged. Jumping far is done with ⇑/⇓, a click
  in the folder list, Home/End and Page Up/Down.
- Keys (library context): **PgUp / PgDn** scroll a page (the view's height less one thumbnail row,
  `scroll.pageUp` / `scroll.pageDown`), **Ctrl+↑ / Ctrl+↓** move like ⇑ / ⇓
  (`scroll.prevSection` / `scroll.nextSection`), **Ctrl+Home / Ctrl+End** go to the top / bottom of
  the library (`scroll.top` / `scroll.bottom`); they ease and add up like the buttons.
- The folder list keeps an ordinary position scrollbar.

## 6. Bottom area

A 20 pt status line, then a 96 pt area split at **0.365·W**: the Photo Tray on the left, controls on
the right.

### 6.1 Status line

Full width, `chrome` fill, centred Small (11 pt) `text_dim` (`design-system.md` §3), fields separated by
wide spaces:
- nothing selected: "No selection";
- one photo: "IMG_0361.jpg     June 4, 2026 7:42 PM     4000 × 3000     3.1 MB";
- several photos or a folder: "48 pictures     June 4, 2026 to June 6, 2026     212.4 MB on disk"
  (+ "     Tags: lisbon (40), tram (6)" when tagged);
- edit view: prefixed "Folder › " and suffixed "(12 of 48)";
- while a scan runs: "Scanning: 1,204 pictures found" (the photos its walks have listed so far, in
  thousands grouped by commas) in place of "No selection", or as the last field when something is
  selected; it goes when the scan ends (`organizing.md` §1).
The text describes the Photo Tray's contents (§6.2: the held photos and the selection, each once),
so held photos count with a selection made elsewhere and an empty tray reads "No selection". Long
text drops the scan field first, then the folder prefix, then middle-elides the file name; a line still too long (a long
tag list) is cut at its end with "…". Transient messages ("Exported 12 pictures", or why an action
was refused) replace the text for 4 s; the next successful action clears one at once. Sizes are
decimal units with one decimal ("3.1 MB", "12.7 KB"); a selection whose photos share one day shows
that day alone. Tags are listed most used first, ties alphabetically.

### 6.2 Photo Tray

- A sunken rounded box (`tray_bg`, radius 4) from x = 6 to 0.365·W − 56, 80 high. Empty: centred
  `text_dim` watermark **"Selection"**.
- Contents: the selection plus the held photos, each photo once. A whole folder/album selected
  from the tree → one **token** at the tray's left (cover thumbnail + accent pill "Folder selected —
  48 photos" / "Album selected — 10 photos"; "1 photo"), unless every photo of it is held; individual
  photos → square centre-cropped mini thumbnails in rows, held photos first, shrinking as the count
  grows. The count picks the first size (54 pt up to 11 items, 48 from 12, 42 from 14, 36 from 16,
  30 from 18, 28 in up to two rows from 20, 22 in up to three rows from 40, 18 in three rows from
  67); a smaller step is taken while the minis do not fit the box (4 pt margin, 3 pt gaps), only
  the rows needed are used, centred; at 18 pt what does not fit is counted on the last place
  ("+38", Dalica addition). At 1400 × 900 (a box 449 wide): 1 item 54, 12 items 30, 40 items 22,
  100 items 18 with "+38".
- Held photos carry the held badge (pin on a dark plate) in their mini's top-right corner;
  tray-selected minis a 2 pt `accent` outline.
- The tray has its own selection (click a tray thumbnail, Ctrl-click toggles; rubber band from the
  box's empty space, Ctrl adds; a click on empty space selects none) used by **Clear** to remove
  just those items; it only ever holds photos the tray shows. Drag onto the tray adds; drag out of
  the tray works like dragging from the grid.
- Three stacked icon buttons at the tray's right edge (34 × 24):
  - **Hold** (pin; "Keep these photos in the tray") — Ctrl+H; held photos get the 📌 badge;
  - **Clear** (circle-slash; "Remove photos from the tray") — removes tray-selected items, or all
    (asking "Clear all photos from the tray?" [Clear Tray] [Cancel] when items are held; over
    automation `tray.clear` then needs `confirm: true`). Photos leaving the tray leave the
    light-table selection too;
  - **Add to Album ▾** (album + ▾; "Add these photos to an album") — popup opening upward: New
    Album…, then albums (recent first).
- Tray context menu: View and Edit · Hold · Remove from Tray · Rotate Clockwise · Rotate
  Counterclockwise · Locate on Disk · — · Properties. On a mini it acts on the tray-selected photos
  (a right-click on an unselected mini selects it first); on empty space, on the tray-selected
  photos or else the whole tray. Items whose command does not exist yet are greyed.
- The tray is not persisted across restarts. An action that needs photos with an empty tray shows
  a toast "Select some photos first".

### 6.3 Star and rotate

Right of the tray, top row: **Star** ("Star / unstar"), **Rotate counterclockwise**, **Rotate
clockwise** (36 × 24 each; disabled when the tray is empty). They act on the tray: Star stars every
photo in it in one undo step, or unstars them all when all are starred. Their shortcuts (Ctrl+8 /
F4, the rotate keys) and menu items act on the same photos: the held ones and the selection.

### 6.4 Thumbnail size, loupe, pane toggles

Top row, right side: **loupe** icon (24 × 24; drag to magnify, §5.5), the **thumbnail-size slider** (128 wide;
Ctrl+1 / Ctrl+2 jump to small (96) / normal (144)), then the four **side-pane toggles** (joined
32 × 24 icon segments): **People** ("Show/hide the People pane"), **Places**, **Tags**,
**Properties** (Info). In the edit view the slider becomes the zoom slider and **Fit** | **1:1**
buttons appear to its left.

### 6.5 Action buttons

Second row (below a 1 pt separator): `action_button`s (64 × 56: icon over label) acting on the
tray: **Export** ┆ **Collage**, **Slideshow**. The set is fixed (not configurable). Collage opens
the Collage room on the tray's photos (the held ones and the selection; with none the engine's
refusal lands on the status line). When the
window is narrower than 1100 (§11), all three collapse behind a **More…** button (an `action_button`
with a three-dot icon) that opens a vertical menu upward: one row per action, its icon in the left
gutter, enabled exactly when the button would be.

## 7. Side pane (right)

One pane at a time, 280 wide (« toggles the large width), pushes the light table; slides in/out
over 0.2 s; open state and pane persisted. When the window narrows until the light table (with the
tree at its minimum) would drop below 480, a large pane first takes its normal width, then the pane
closes; a pane opened on purpose in an already narrow window stays open. Header 30 high: « at left, centred SemiBold title
("People", "Places", "Tags", "Properties"), × at right. Toggles: bottom-bar segments, View →
People/Places/Tags/Properties, **Ctrl+0** (toggle the pane), Ctrl+T (Tags). Contents follow the
current selection (or the selected folder). Pane contents: People, Places and Tags in
`organizing.md` §5–7; Properties: header "Properties of IMG_0361.jpg", then label/value rows (Path,
File size, Dimensions, Date taken, Date digitized, File modified, Camera, Lens, Exposure, Aperture,
ISO, Focal length, Flash, Orientation, Color space, GPS, Caption, Tags), with a [Show All Metadata]
link listing every EXIF/XMP/IPTC field.

## 8. Rooms (tabs)

Import and Collage each open as a **room**: a tab in a
tab strip that appears between the menu bar and the content when at least one room is open:
`[▦ Library] [📷 Import ×] [▦ Collage ×]`. Tabs are 190 × 28, icon + label, the active tab joined
to the content below; Library has no ×. Closing a room with unsaved work asks first: closing Import
while it copies asks "Stop importing?" [Stop Import] [Keep Importing], and Stop Import cancels the
import (what it copied stays) and closes the room. The edit view is not a room: it replaces the
Library tab's content.

The strip is 28 high under the menu bar, its first tab 8 in from the left. A room in front
replaces everything between the strip and the status line: the toolbar, the tree, the light table,
the side pane and the bottom area (the menu bar and the status line stay). A click on a tab brings
its room to the front, and the keys are the front room's (the Import room's X and Enter act only
while it is in front). The Library tab brings back the library, or the edit view, exactly as it
was left: the scroll position and the selection are kept while another room is in front,
and the library's regions sit 28 lower while the strip shows. In a narrow window the tabs shrink
and then overflow into a menu at the strip's end, the front tab always kept.

## 9. Search mode

When any search text or filter is active:
- a **filter banner** (`filter_banner`, 32 high) spans the top of the light table: [**Show All**]
  ("Leave search") + "Showing 33 pictures in 4 folders (0.01 s) · 439.8 MB";
- only matching sections and photos are shown (section headers keep their buttons);
- a temporary album **Search results for "x"** appears at the top of Albums and can be acted on
  like any album; group headers in the tree show a magnifier;
- **Show All**, the search field's ×, Esc in the search field, or turning off the filters leave
  search mode and restore the previous scroll position.

## 10. Other views

| View | Entry | Spec |
|---|---|---|
| Edit view | double-click, Enter, Ctrl+3 | [`editing.md`](editing.md) |
| Slideshow | Ctrl+4, Play buttons | [`viewing.md`](viewing.md) |
| Photo viewer (standalone) | opening an image file from the OS | [`viewing.md`](viewing.md) |
| Rooms: Import, Collage | toolbar/bottom buttons, Create menu | [`organizing.md`](organizing.md) §3, [`creating.md`](creating.md) |

The edit view keeps the menu bar, the status line and the bottom area of §2 and replaces the
rest (`editing.md` §2):

| Region | Size | Notes |
|---|---|---|
| Edit top bar | 40 high, the whole width | Back to Library and Play on the left (240 wide with margins), the compare toggle and the two compare buttons on the right (220); for a collage (a readable recipe beside it) **Edit Collage** (120) left of the compare toggle, the filmstrip giving way by its width and a gap |
| Filmstrip | the top bar between its two groups | W − 460 wide: seven 56 pt thumbnails and the arrows at 900, nine at 1400 |
| Edit panel | **280** wide, the height of the middle | hidden by Ctrl+9 / View ▸ Show Edit Controls; the photo area takes its width |
| Photo area | the rest of the middle | the photo (`canvas`) above the caption bar |
| Caption bar | 36 high, under the photo, the photo area's width | |
| Side pane | **260** wide (large: as in §2) | the same panes as the library, toggled from the bottom bar |

At 1400 × 900 the edit panel is `[0, 72, 280, 712]`, the photo `[280, 72, 1120, 676]` and the caption
bar `[280, 748, 1120, 36]`; at 900 × 600 the photo is `[280, 72, 620, 376]`. `ui.inspect` → `regions`
names them `editTopBar`, `filmstrip`, `editPanel`, `canvas` and `captionBar`
(`control-protocol.md` §6.6).

**A video in the edit view** (`editing.md` §11, M15.21): the photo area (`region:video`) holds the
video above its 40 pt control bar: its poster frame, else the video tile (`picture:video`) with its
sprocket edge, fitted inside the 24 pt padding at the shape the video shows at (16:9 while unknown;
a tile has no pixels, so it may grow past its size). One Dalica cannot play says **"This video can't
be played"** under the Play triangle (`text:video.cannotPlay`, semibold body) with **Open With ▸**
10 pt under it (`button:video.openWith`: the photo menu's Open With rows in a popup to its right).
The edit panel's strip leads with the **video tab** (`tab:edit.video`, the Video Frames glyph,
shown) followed by tabs 1–5 greyed, their tooltips adding "A video takes no photo edits."; Show
original is not drawn. The video tab's page is a grid of tool buttons two to a row: **Save Frame as
Picture**, **Export Clip** (greyed until the player), **Rotate Left**, **Rotate Right**
(`button:video.saveFrame`, `…exportClip`, `…rotateCcw`, `…rotateCw`); Undo / Redo and the histogram
box stay. At 900 × 600 both rows and the six tabs fit the 280 pt panel.

## 11. Responsiveness rules

- Window narrower than 1100: the action-button row overflows into More…; the toolbar's filter
  caption hides; the tree keeps its width until the light table would drop below 480, then shrinks
  to its minimum.
- The tree never collapses entirely in the library view; the side pane closes automatically when
  the light table would drop below 480.
- The 480 counts the light table with its splitter and scroll bar. With a side pane open the tree
  makes room for the pane first: it is at most W − pane − 480 wide (never under 200), and the
  splitter drags it no further. Only when the tree is at 200 does a narrowing window take a large
  pane's width away, then close the pane (§7). The width the user gave the tree is kept and comes
  back as soon as there is room.
- **Full screen** (F11, View → Full Screen; F11 again leaves) fills the screen with the same
  layout: every region and control is where these rules put it for the screen's size, the menus
  and the caption buttons included, so leaving needs no hidden gesture. The window frame's own
  behaviour stops while full screen: no resize edges.
- All regions must be verified by screenshot at 1400 × 900 and 900 × 600 (and 1100 × 700, where
  the narrow-window rules begin), with the side pane open and closed, and in full screen.

## 12. Keyboard and screen readers

Every part of the window is usable without a pointer and described to a screen reader
(`SYS-A11Y`; `crates/ui/src/a11y.rs`, `a11y/keyboard.rs`, `panels/menubar.rs`).

- **The menu bar.** F10, or Alt pressed and released on its own (Windows and Linux; not macOS,
  where Option is no menu key), opens the first menu with the focus on its title; again, it closes
  the menu. In an open menu Left and Right open the menu beside it (wrapping round), Down and Up
  move through its enabled rows (wrapping; an open submenu's rows come right after its row), Enter
  or Space runs the focused row or opens its submenu, Esc closes the menu and leaves the focus on
  its title, and a second Esc leaves the menu bar. While a menu is open its keys are its own: the
  arrows move no photo. Every command is reached by its shortcut or by a menu row this way.
- **Tab.** Tab and Shift+Tab walk every control that takes the focus, region by region in
  reading order (the menu bar, the room tabs, the toolbar, the tree, the light table and its scroll
  bar, the side pane, the status line, the tray and the bottom rows; in the edit view its own
  regions), and within a region in reading order: rows top to bottom, each left to right. They
  wrap round. Under a dialog they cycle the dialog's controls only, in reading order (a field,
  then the options under it, then the buttons left to right), never the window under it.
- **A focused control** takes its own Enter and Space (a button presses, a toggle flips) and its
  arrows (a slider moves, a row group steps); the view's shortcuts keep working (Ctrl+8 stars),
  except while a text field has the focus. **A focused cell** of the light table leaves the
  arrows to the selection, as without focus, and the focus follows the selected photo. Esc gives
  the focus up.
- **The focus is always shown**: the focus ring (`design-system.md` §9) on the window's controls,
  egui's focus look on the menu rows.
- **What a screen reader hears** (the AccessKit tree, built only while one asks for it): each
  region is a named landmark: Menu bar, Rooms, Toolbar, Library tree (navigation), Light table
  (main), the side pane by its pane's name (complementary), Status line, Photo tray, Tray buttons,
  View controls, Actions; in the edit view its Edit toolbar, Filmstrip, Edit panel, Edit view
  (main) and Caption; a room in front is the main landmark named after the room; a slideshow and
  the photo viewer one main landmark each. The light table's visible cells are list items named by
  the file name, selected or not, with "Starred", "Edited", "Video, 1:23" and the whole caption (a
  long one is elided under the cell, never in the description), inside a group per section named by
  its title. The tree's rows say their level, whether they are open (rows with rows below) and
  "20 photos". The edit view's photo, the compared photo, the collage, the slideshow's photo and
  the viewer's photo are images named by what they show, with their zoom, the open tool or the
  photo's place in the show; the map is a group with its zoom (its markers are controls inside
  it). Dialogs are modal dialogs titled as they are drawn. Cells and rows off screen are never
  drawn, so never in the tree: the tree grows with the window, never with the library.
- **Scaling.** At UI scale 150 % with the Large font the window at 1400 × 900 pixels (933 × 600
  points), the edit view and every dialog keep their controls inside the window and no label is
  cut at its side (`crates/ui/tests/scaling.rs`).
