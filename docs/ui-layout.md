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
│ File  Edit  View  Folder  Picture  Create  Tools  Help                                                 │ menu bar 24
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
| Menu bar | 24 high | in-window on Windows/Linux, native on macOS (generated from the same menu model) |
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
column).

## 4. Library tree (left pane)

- **Groups** (collapsible, each shown only when non-empty), in this order: **Albums**, **People**,
  **Projects** (flat mode only: Exported, Collages, Video Frames),
  **Folders**, **Other** (flat mode only: folders of mostly tiny images), **Hidden Folders** (only
  with View → Hidden Pictures).
- **Group header**: 30 high, full width, disclosure triangle, SemiBold 13 title with count "Albums
  (3)". Collapsing a group hides its rows **and its sections in the light table**. During search the
  triangle is replaced by a magnifier glyph. Headers are sticky: a collapsed group's header stays
  docked at the bottom of the pane when the list above it is short.
- **Rows**: 24 high; 16 pt icon (folder, album, automatic album, person face thumbnail 20 pt in
  28-high rows, project), name, count in `text_dim`; long names elide in the middle. Tree mode:
  16 pt indent per level with disclosure triangles, parents show their own count (often "(0)",
  dimmed). Flat mode sorted by date: **year dividers** (the year in `text_dim` 11 pt with a 1 pt
  rule to the right edge, 18 high), newest year first, only when folders span more than one year.
- **Selection**: selected row = `canvas` fill with `text` label (focused list: plus a 2 pt `accent`
  bar at the left edge). Hover = `hover × 0.6`.
- **Click** = scroll the light table to that section + select the folder as a unit (§1).
  **Double-click** = Folder/Album Properties. **Right-click** = folder/album context menu
  (`organizing.md` §9.2). **Delete** on a folder = Delete Folder (confirmation).
- **Drop target**: dragging photos over a folder row highlights it with `drop_target` (drop = move;
  Ctrl = copy; drag pill text changes accordingly); over an album row = add to album; over a person
  = assign faces (from the Unnamed view).
- Keyboard: ↑/↓ move the selection (scrolling the light table), ←/→ collapse/expand in tree mode.

## 5. Light table

### 5.1 Sections

Each visible folder/album is a section: **header block** + **thumbnail grid**. Sections follow the
tree order. 24 pt gutter between a grid's last row and the next header.

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
  is inside this section, else the whole section.
- Top-right, only when the section has unsaved edits: "%d edited" (`text_dim`) and a **Save**
  text button (same as the 💾 button).
- **Description** line: the description (`text_label`) or the placeholder **"Add a description"**
  (`text_dim`); click to edit inline (Enter commits, Esc cancels).
- Right-click anywhere on the header = the folder/album context menu.
- **Sticky**: the header of the section being scrolled through stays pinned at the top of the
  light table (collapsed to its first line, 32 high) and is pushed up by the next header.
- Person sections: header buttons **Zoom to Faces / Show Whole Photos**, **Set as Person
  Thumbnail**, Play, Collage; the Unnamed section: **Expand Groups**, **Show Ignored Faces**.

### 5.3 Thumbnail grid

- Cells are squares of edge `t` (thumbnail size, slider §6.4: 64–320, default 144); each photo is
  scaled to fit its square, centred, keeping its aspect ratio (portraits narrower), never cropped,
  never rounded, with a subtle drop shadow (`black α90`, offset 2, blur 4).
- Columns: as many as fit with a minimum horizontal gap of 12; the remaining width is spread evenly
  so columns justify across the light table; vertical gap 22 (room for a caption line when
  captions are on). Left/right margins 24.
- **Caption** (View → Thumbnail Caption: None (default) · Filename · Caption · Tags · Date ·
  Resolution): one line centred under the cell, 10.5 pt `text_dim`, middle-elided.
- **Badges** (`design-system.md` §7): a right-aligned row inside the image's bottom-right corner in
  the order edited ✎ · geotag 📍 · held 📌 · star ★ (star rightmost); videos: a film-sprocket edge
  on the left side of the image and a duration pill "1:23" at its bottom-left.
- **Hidden** photos (only visible with View → Hidden Pictures) are drawn at 40 % opacity.
- **Small pictures** (< 64 px on both sides) are omitted unless View → Small Pictures.
- **Empty states**: no watched folders → "No pictures yet" + [Add a Folder…] [Import…]; search with
  no results → "No photos found" + [Show All].

### 5.4 Selection

- **Click** selects one photo (2 pt outline: white on the anchor, `gray 170` on others; selected
  cell surround `cell_selected`); the tray mirrors the selection live.
- **Ctrl-click** toggles, **Shift-click** selects a range from the anchor, **rubber band** from empty
  space selects every thumbnail the band intersects. Pressing on a thumbnail and dragging starts a
  drag instead.
- Selection never crosses a section boundary; selecting in another section replaces it (unless
  items are held in the tray).
- Keyboard: arrows move (Shift extends); arrows stop at the section's ends; **Home/End** first/last
  photo of the section (Shift extends); **Ctrl+Home/End** top/bottom of the library; **Ctrl+A**
  select all in the section; **Ctrl+D** select none; **Ctrl+I** invert; **Ctrl+H** hold; **Enter**
  view and edit; **Space** star toggle in the library is not bound (Ctrl+8 is).

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
  to Garden").
- **Wheel** scrolls; **Ctrl+wheel** changes the thumbnail size.
- **Loupe**: press and drag from the magnifier icon left of the thumbnail-size slider; a round
  lens (≈ 104 pt) follows the pointer over the light table showing the photo under it at 100 %
  (fades in 0.15 s, out 0.4 s).

### 5.6 Scrollbar

16 wide on the right of the light table: **⇑ previous section** (16 × 24), **▲**, track (click
pages; page step = one thumbnail row + 90, animated), thumb, **▼**, **⇓ next section**. The ⇑/⇓
buttons scroll so the previous/next section header is at the top (⇑ first snaps to the current
section's start when mid-section). All four buttons auto-repeat when held.

## 6. Bottom area

A 20 pt status line, then a 96 pt area split at **0.365·W**: the Photo Tray on the left, controls on
the right.

### 6.1 Status line

Full width, `canvas` fill, centred 11.5 pt `text_label`, fields separated by wide spaces:
- nothing selected: "No selection";
- one photo: "IMG_0361.jpg     June 4, 2026 7:42 PM     4000 × 3000     3.1 MB";
- several photos or a folder: "48 pictures     June 4, 2026 to June 6, 2026     212.4 MB on disk"
  (+ "     Tags: lisbon (40), tram (6)" when tagged);
- edit view: prefixed "Folder › " and suffixed "(12 of 48)".
Long text drops the folder prefix first, then middle-elides the file name. Transient messages
("Exported 12 pictures") replace the text for 4 s.

### 6.2 Photo Tray

- A sunken rounded box (`tray_bg`, radius 4) from x = 6 to 0.365·W − 56, 80 high. Empty: centred
  `text_dim` watermark **"Selection"**.
- Contents: a whole folder/album selected from the tree → one **token** (cover thumbnail + pill
  "Folder selected — 48 photos" / "Album selected — 10 photos"); individual photos → square
  centre-cropped mini thumbnails in rows, shrinking as the count grows (54 pt up to ~11 items, then
  48, 42, … 28 in two rows, 18 in three rows).
- The tray has its own selection (click a tray thumbnail; rubber band) used by **Clear** to remove
  just those items. Drag onto the tray adds; drag out of the tray works like dragging from the grid.
- Three stacked icon buttons at the tray's right edge (34 × 24):
  - **Hold** (pin; "Keep these photos in the tray") — Ctrl+H; held photos get the 📌 badge;
  - **Clear** (circle-slash; "Remove photos from the tray") — removes tray-selected items, or all
    (asking "Clear all photos from the tray?" [Clear Tray] [Cancel] when items are held);
  - **Add to Album ▾** (album + ▾; "Add these photos to an album") — popup opening upward: New
    Album…, then albums (recent first).
- Tray context menu: View and Edit · Hold · Remove from Tray · Rotate Clockwise · Rotate
  Counterclockwise · Locate on Disk · — · Properties.
- The tray is not persisted across restarts. An action that needs photos with an empty tray shows
  a toast "Select some photos first".

### 6.3 Star and rotate

Right of the tray, top row: **Star** ("Star / unstar"), **Rotate counterclockwise**, **Rotate
clockwise** (36 × 24 each; disabled when the tray is empty). They act on the tray.

### 6.4 Thumbnail size, loupe, pane toggles

Top row, right side: **loupe** icon (drag to magnify, §5.5), the **thumbnail-size slider** (128 wide;
Ctrl+1 / Ctrl+2 jump to small (96) / normal (144)), then the four **side-pane toggles** (joined
32 × 24 icon segments): **People** ("Show/hide the People pane"), **Places**, **Tags**,
**Properties** (Info). In the edit view the slider becomes the zoom slider and **Fit** | **1:1**
buttons appear to its left.

### 6.5 Action buttons

Second row (below a 1 pt separator): `action_button`s (64 × 56: icon over label) acting on the
tray: **Export** ┆ **Collage**, **Slideshow**. The set is fixed (not configurable). When the
window is too narrow, the overflow collapses behind a **More…** button that opens a vertical menu.

## 7. Side pane (right)

One pane at a time, 280 wide (« toggles the large width), pushes the light table; slides in/out
over 0.2 s; open state and pane persisted. Header 30 high: « at left, centred SemiBold title
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
to the content below; Library has no ×. Closing a room with unsaved work asks first. The edit view
is not a room: it replaces the Library tab's content.

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

## 11. Responsiveness rules

- Window narrower than 1100: the action-button row overflows into More…; the toolbar's filter
  caption hides; the tree keeps its width until the light table would drop below 480, then shrinks
  to its minimum.
- The tree never collapses entirely in the library view; the side pane closes automatically when
  the light table would drop below 480.
- All regions must be verified by screenshot at 1400 × 900 and 900 × 600.
