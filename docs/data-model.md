# Data model and file handling

How Dalica stores what the user does, what it writes to disk, and when. This is the most
safety-critical document in the repository: read it before touching `crates/sidecar`,
`crates/meta`, `crates/fs`, `crates/store`, `crates/scan` or any command that moves, renames, saves or
deletes files.

## 1. Principles

1. **Folders on disk are the library.** Dalica shows the photos and videos that live in the
   folders the user tells it to watch. It never invents its own folder hierarchy, never moves files
   on its own, and never imports by copying unless the user asks (camera/card import).
2. **Pixels are never changed implicitly.** Edits are stored as a *recipe* (an ordered list of
   operations). The original image file's pixels change only when the user explicitly **Saves**,
   and Save first moves the untouched original into a hidden per-folder originals directory so it
   can always be restored.
3. **Everything the user typed or decided is portable.** Stars, captions, tags, hidden flags,
   rotations, edit recipes (including undo/redo), album membership and order, custom sort order,
   face rectangles and their person, places — all of it is written to a small human-readable
   **sidecar file in each folder**. The central library index is a cache: deleting it loses
   nothing but time (a rescan rebuilds it).
4. **Interoperability through standard metadata.** Captions, tags, places, and (opt-in) people
   names are also written into the photo's own XMP/IPTC/EXIF metadata so other applications see
   them (see §5).
5. **Atomic, verified, reversible.** Every write is temp-file + fsync + rename. Deletes go to the
   OS trash. Unknown data in files we touch is preserved. Every destructive command asks first and
   is undoable where the platform allows.

## 2. Where each piece of data lives

| Data | Folder sidecar `.dalica.json` | Photo file metadata | Library index (cache) | Elsewhere |
|---|---|---|---|---|
| Star (boolean) | `"star": true` | XMP `xmp:Rating=5` when "Write stars as ratings" is on (default **off**) | yes | |
| Caption | `"caption"` | XMP `dc:description` + IPTC Caption-Abstract (default **on**) | yes | |
| Tags | `"tags"` | XMP `dc:subject` + IPTC Keywords (default **on**) | yes | |
| Hidden | `"hidden": true` | — | yes | |
| Edit recipe, including 90° rotations and flips | `"edits"` (ordered), `"redo"` | — (until Save) | yes | |
| Saved-edit backup | — | — | yes | `.dalica-originals/<file>` + `.dalica-originals/.dalica.json` |
| Album membership and order | `"albums": { "<album id>": "<position key>" }` per photo (§3.2) | — | yes | album definitions: §4 |
| Folder description/date/place | `"folder"` object | — | yes | |
| Custom folder sort order | `"folder": { "order" }` | — | yes | |
| Face rectangles + person | `"faces"` | XMP MWG Regions when "Store people names in photos" is on (default **off**) | yes | |
| People (names) | `"people"` entries for people present in the folder | (as above) | yes | `people.json` in the library dir |
| Ignored faces | `"faces"` entry with person `ignored` | — | yes | |
| Place (geotag) | `"geo": [lat, lon]` | EXIF GPS (default **on**) | yes | |
| Date taken corrections | `"date"` | EXIF DateTimeOriginal when "Write date changes to files" is on (default **on**) | yes | |
| Thumbnails, previews | — | — | disk cache | `<cache>/thumbs/` |
| Watched folders | — | — | yes | `folders.json` in the library dir |

Rules:
- When a value exists both in the sidecar and in file metadata and they disagree, the **sidecar
  wins** for display and the file is rewritten on the next metadata write. A metadata-only value
  (file came from another app, no sidecar entry) is imported into the sidecar on first scan.
  A value changed by another application since Dalica last synced it (and unchanged in the
  sidecar) is imported instead of overwritten; see `xmp-interop.md` §5.
- Formats Dalica cannot write metadata into (RAW, video, GIF, BMP, read-only files) keep everything
  in the sidecar only. RAW files may get an XMP sidecar (`<name>.xmp`) when "Write XMP sidecars for
  RAW files" is on (default off), merged, never replaced.
- Metadata writes never change pixels and never re-encode the image: they rewrite only the
  APP1/APP13 segments of JPEGs (or the equivalent in TIFF/PNG/WebP), preserving all other segments
  byte for byte. A metadata write must not change the file's modification time unless
  "Update file modification time on metadata changes" is on (default off).

## 3. The folder sidecar `.dalica.json`

One per folder that has user data, created lazily (a folder the user never touched has none).
UTF-8 JSON (RFC 8259), LF line endings, pretty-printed with two-space indentation, written
atomically, keys sorted deterministically (photos by file name) so diffs are minimal. Hidden
(dot-file) on every platform.

```json
{
  "dalica": { "version": 1 },
  "folder": {
    "description": "Weekend in Lisbon with the family",
    "date": "2026-06-04",
    "place": "Lisbon, Portugal",
    "sort": "manual",
    "order": ["IMG_0003.jpg", "IMG_0001.jpg", "IMG_0002.jpg"]
  },
  "photos": {
    "IMG_0001.jpg": {
      "star": true,
      "caption": "Tram 28 at sunset",
      "tags": ["lisbon", "tram", "sunset"],
      "hidden": false,
      "edits": ["rotate(90)", "crop(0.08,0.10,0.92,0.86)", "straighten(1.5)", "autofix()", "fill_light(0.25)", "sepia()"],
      "redo": ["glow(0.4,3.0)"],
      "faces": [
        { "rect": [0.41, 0.22, 0.52, 0.38], "person": "p7f3a2c" },
        { "rect": [0.70, 0.30, 0.78, 0.44], "person": "ignored" }
      ],
      "albums": { "a91c0e": "V", "a77d02": "b3" },
      "geo": [38.7116, -9.1389],
      "date": "2026-06-04T19:42:10"
    }
  },
  "albums": { "a91c0e": { "name": "Best of 2026" }, "a77d02": { "name": "Trams" } },
  "people": { "p7f3a2c": { "name": "Ana Costa" } }
}
```

Format rules:
- File names are object keys under `"photos"`, so JSON string escaping round-trips any name
  (including `]`, `"`, `;`, `=` and non-ASCII). `sort` is `manual | name | date | size | modified`.
- Lists (`order`, `tags`, `edits`, `redo`) are JSON arrays; a photo's `albums` is an object from
  album id to position key (§3.2). Each edit operation is one
  string in the operation syntax of §3.1, the same text the recipe API uses.
- Album ids are `a` and person ids `p`, followed by 4 to 16 lowercase hex digits (`a91c0e`,
  `p7f3a2c`); anything else is kept verbatim and reported (`security.md` §5.4).
- Coordinates are **normalised floats** in 0..1, at most six decimals, `[x0, y0, x1, y1]`. Which
  frame they are measured in is an open question (§3.3).
- Unknown objects and keys are **preserved verbatim** on rewrite (forward compatibility). A value of
  the wrong type is kept as-is and reported once; it never aborts reading the rest.
- Readers accept a BOM and CRLF; a duplicate key takes the later value. Nesting is bounded
  (`security.md` §8).
- Writers write only when the content changed, and never more often than once per second per
  folder (debounced); a crash loses at most the last second of changes in that folder, never the
  file.
- If a sidecar is unreadable, it is renamed to `.dalica.json.corrupt-<timestamp>` and kept; the
  folder is shown with a `caution` banner; nothing in it is written until the user chooses
  "Reset folder data" or restores the file.
- A folder may also hold the legacy organizer's sidecar. Which file is read, and when Dalica starts
  its own, is in §9.

### 3.1 Edit operations (`"edits"`)

The recipe is the edit history in the order the user applied it. Rendering applies operations in
a fixed **pipeline order** (geometry first, then tone/colour, then effects in the order applied),
so the result does not depend on the order of crop vs. straighten. Every operation is versioned by
name; an unknown operation is skipped (and preserved), it never aborts the rest of the recipe.

| Operation | Parameters | Spec |
|---|---|---|
| `rotate(deg)` | 90, 180 or 270 | 90° steps (Ctrl+R / Ctrl+Shift+R, rotate buttons); consecutive rotations merge into one entry, and a net 0 removes it |
| `flip_h()` / `flip_v()` | — | mirror horizontally / vertically (Ctrl+Shift+H / Ctrl+Shift+V); two consecutive identical flips cancel |
| `crop(x0,y0,x1,y1)` | normalised rect | only the last crop counts |
| `straighten(deg)` | −10..+10 (crop to fill) | |
| `retouch(sx,sy,tx,ty,r;…)` | source/target points and radius per stroke | stored as parameters |
| `autofix()` | — | the one-click fix (auto contrast + auto colour + fill) |
| `autocontrast()` / `autocolor()` | — | |
| `fill_light(a)` | 0..1 | Basic Fixes slider |
| `tuning(fill,highlights,shadows,temp,neutral)` | 0..1, 0..0.5, 0..0.5, −1..1, `#rrggbb` or `none` | the whole Tuning tab is one operation, updated in place while the tab is open |
| effects | see [`editing.md`](editing.md) → Effects | e.g. `sharpen(a)`, `sepia()`, `bw()`, `warmify()`, `grain()`, `tint(color,preserve)`, `saturation(a)`, `soft_focus(x,y,size,amount)`, `glow(i,r)`, `filtered_bw(color)`, `focal_bw(x,y,size,sharpness)`, `graduated_tint(y,feather,shade,color)` … |
| `trim(start,end)` | seconds | videos only |

The authoritative list with ranges and defaults is in [`editing.md`](editing.md).

Rotations and flips are ordinary entries, so both undo histories can step over them
(`architecture.md` §4): a rotation made in the library is one library undo step that changes the
photo's `"edits"`; one made in the edit view is a step of the photo's own history.

### 3.2 Album order: position keys

An album's manual order spans many folders, so it is stored as one **position key** per member:
a short string (digits and ASCII letters) that sorts in album order by plain byte comparison.
- Adding a photo gives it a key after the album's last key; dropping a photo between two others
  gives it a key that sorts between theirs (a key always exists between two different keys). Only
  the moved photo's sidecar is rewritten; no other folder is touched.
- A key is at most 512 bytes. Inserting at one spot over and over (appending, prepending, dropping
  next to the last drop) keeps keys under 10 bytes for 10 000 inserts; only repeated drops into the
  middle of the newest pair grow them steadily, and when a key would pass the limit Dalica rewrites
  the album's keys as Sort Album By does. The encoding is documented in `dalica-core` (`poskey`).
- Rebuilding an album = collect every member from every folder sidecar and sort by key; equal keys
  (possible after merges) sort by folder path, then file name.
- Sort Album By ▸ rewrites the keys of all members (one undo step).
- The folder-level `"albums"` object carries each album's name so an album can be recovered when
  `albums.json` is lost.

### 3.3 Open question: the coordinate frame

`TODO(maintainer)`: which frame do normalised coordinates use? They appear in `crop`, face
rectangles (`"faces"`), `retouch` spots and the centre points of the focal effects (Soft Focus,
Focal B&W, Graduated Tint, Focal Zoom). The choice only matters when a photo is rotated or flipped
*after* such data exists. Options: (a) the photo as displayed after Dalica's rotations, which means
rewriting every stored rectangle on each rotation; (b) the photo after EXIF orientation but before
Dalica's own rotations, converting only for display. To decide before the edit view's crop tool
is built.

## 4. Library directory (index and global data)

`<data>/Dalica/` (Linux `$XDG_DATA_HOME/dalica`, macOS `~/Library/Application Support/Dalica`,
Windows `%LOCALAPPDATA%\Dalica`):

| File | Contents | Rebuildable? |
|---|---|---|
| `folders.json` | watched folders and their mode (Scan Always / Scan Once / Removed), exclusions; M1 writes `{"version": 1, "folders": [{"path": …}]}` (modes arrive with the Folder Manager, M7) | no — small, backed up with settings |
| `albums.json` | album definitions (id, name, description, date, place, cover) and a copy of the order | yes, from sidecars (each member folder carries the album's name; each member its position key, §3.2) |
| `people.json` | people (id, name, nickname), merges, settings | yes, from sidecars (names are mirrored per folder) |
| `index.log` + `index.snap` | the library index: one record per photo (path, size, mtime, dimensions, dates, EXIF summary, content hash, face descriptors), journal + snapshot as in LightCraft | yes, by rescanning |
| `lock` | single-process lock | — |
| `settings.json` | preferences (atomic write; damaged file kept as `.corrupt-<time>`); unknown keys kept; `"keymap"` holds the user's shortcut overrides (`menus-and-shortcuts.md` §2.0) | no |

`<cache>/Dalica/thumbs/` holds the thumbnail/preview disk cache (2 GiB default), deletable at any
time. Face detection descriptors are cached in the index; face *assignments* live in sidecars.

"Rebuild library" (Help → Troubleshooting) deletes the index and rescans; it must never lose user
data, and a test proves it (scan → edit everything → rebuild → compare).

## 5. Writing metadata into files

| Item | When | Where |
|---|---|---|
| Caption | when the caption field loses focus or Enter is pressed | XMP `dc:description`, IPTC 2:120 |
| Tags | immediately | XMP `dc:subject`, IPTC 2:25 |
| Place | immediately | EXIF GPS IFD (lat/lon + refs) |
| Date taken (Adjust Date and Time) | on OK | EXIF DateTimeOriginal (+ CreateDate) |
| People names | only if "Store people names in photos" is on | XMP MWG Regions (`mwg-rs:Regions`) |
| Star | only if "Write stars as ratings" is on | XMP `xmp:Rating` 5 / 0 |
| Edits, rotation | only on Save / Save a Copy / Export | new pixels |

All writes are queued to a background writer, batched per file, merged with existing metadata
(existing XMP/IPTC fields from other apps are preserved), written atomically, and verified by
re-reading. A failed write shows a `caution` toast and leaves the sidecar as the record.

## 6. Save, Undo Save, Revert, Save a Copy, Export

**Save** (File → Save, Ctrl+S in the edit view, the folder header's "Save" button for all edited
photos in the folder):
1. First time only: a confirmation dialog explaining that the edited version replaces the file and
   the original is kept in a hidden backup (checkbox "Don't show this again").
2. Move the original byte-for-byte to `<folder>/.dalica-originals/<name>` (only if no backup exists
   yet — the *first* original is always the one kept).
3. Render the recipe at full resolution and write it under the original name (JPEG quality matched
   to the original's estimated quality, 90 if unknown; all metadata carried over; orientation
   normalised to "top-left" after applying rotation).
4. Record the recipe, the original's mtime and dimensions in `.dalica-originals/.dalica.json`; clear
   `"edits"` from the folder sidecar.
5. For files Dalica cannot write in their own format (RAW, PNG with alpha, GIF, video), Save writes
   a JPEG next to the source (`name.jpg`, or `name-001.jpg` on collision) and leaves the source
   untouched.
A Save whose recipe holds only rotations and flips is **lossless** for JPEGs (DCT-domain rotation when dimensions allow; otherwise
the EXIF orientation flag is updated instead).

**Undo Save** (button in the edit panel for saved photos): restore the original from the backup and
put the recipe back as unsaved edits.

**Revert** (File → Revert): confirmation *"Revert to the original? Your edits will be lost. To undo
only the last save and keep the edits, choose Undo Save."* with buttons [Revert] [Undo Save]
[Cancel] (Cancel default). Restores the original and discards edits.

**Save a Copy**: writes `<name>-001.jpg` (next free number) with edits applied, in the same folder;
the original and its edits are untouched.

**Save As…** (one photo): Save a Copy with a file dialog to choose the name, the folder and the
format (JPEG, or PNG; JPEG quality as for Save). It never overwrites an existing file (the dialog
asks for another name), never touches the original or its edits, and if the chosen folder is not
watched it is added as Scan Always so the new file appears in the library.

**Export** never changes originals: it renders copies into a chosen folder (see
[`creating.md`](creating.md) → Export), adds the export folder to the library as an
**Exported** collection member (adding it to the watched folders as Scan Always when it is outside
them), and names collisions `-001`.

Photos with unsaved edits show the *edited* badge; folders with unsaved edits show a Save button in
their folder header.

## 7. Scanning and watching

- **Folder Manager** modes per folder: **Scan Always** (watched: `notify` events + periodic
  verification), **Scan Once** (indexed once, then frozen — for removable or slow media),
  **Remove from Dalica** (not indexed; nothing on disk changes). Setting a mode on a folder applies
  to its subfolders unless they have their own setting.
- **First run** offers: "Scan only my Pictures, Documents and Desktop folders" (default) or
  "Choose folders…". Scanning the whole disk is not offered.
- **Recognised files:** JPEG, PNG, TIFF, WebP, GIF, BMP, HEIC/HEIF (if built with support), AVIF,
  JPEG XL, PSD (composite), the RAW formats supported by the codecs crate; videos MP4/MOV/M4V/MKV
  (indexed with a poster frame when decodable, otherwise a generic video
  tile). Per-type toggles in Options → File Types.
- **Skipped:** dot-folders and dot-files, `.dalica-originals`, OS system folders, cache/temp
  folders, images smaller than 64 × 64 (shown only with View → Small Pictures).
- **External changes:** created/deleted/renamed files and folders in watched roots are picked up
  within seconds; a file whose (size, mtime) changed is re-read; a folder moved outside Dalica keeps
  its data because the sidecar moves with it. A file deleted outside Dalica disappears from the
  library; its stale sidecar entry is pruned after 30 days or on "Clean up folder data".
- **Offline volumes:** folders on unplugged drives stay in the tree (dimmed, `caution` "offline"
  marker) with their cached thumbnails; edits to their photos are queued in the index and written
  to the sidecar when the volume returns.
- Walks are bounded (depth 32, 1,000,000 entries, symlink loops detected) and never run on the UI
  thread.

## 8. File operations from inside Dalica

| Operation | Disk effect | Sidecar effect | Undo |
|---|---|---|---|
| Move photos (drag to a folder) | rename/move (copy+verify+delete across volumes); collisions → dialog [Rename Duplicates] [Skip Duplicates] [Cancel], renaming to `-001` | the photo's entry moves with it (including its `.dalica-originals` backup) | yes |
| Move folder | move directory | sidecar moves with it | yes |
| Rename photo / batch rename | rename | entry renamed | yes |
| Rename folder | rename directory | — | yes |
| Delete photo / folder | to the OS trash after confirmation | entry removed (kept in the undo record) | best effort (restore from trash) |
| Create folder | mkdir | — | yes (if still empty) |
| Hide / Unhide | none | `"hidden"` | yes |
| Remove folder from Dalica | none | none | yes |
| Locate on Disk | opens the file manager with the file selected | — | — |

Merging folders (moving a folder onto one that exists) merges sidecars entry by entry; it
never overwrites one sidecar with another.

## 9. Legacy sidecars (read-only compatibility)

Folders that the legacy organizer managed may still hold its per-folder INI sidecar (stars,
captions/tags for non-JPEG files, rotations, edit lists, crops, face rectangles and person names,
album membership and album definitions) and its hidden originals directory with its recipe file.
Dalica reads them so a library keeps its data; it never writes them. The format is documented only
in the private notes (`notes/`); the reader lives in `crates/sidecar/src/legacy.rs` with a
provenance header.

Which file is used, per folder:

| Folder contains | Dalica reads | Dalica writes on the first change |
|---|---|---|
| neither | nothing | a new `.dalica.json` |
| only the legacy sidecar | the legacy sidecar, converted in memory | a new `.dalica.json` holding the full converted data plus the change |
| only `.dalica.json` | `.dalica.json` | `.dalica.json` |
| both | `.dalica.json` only; the legacy sidecar is ignored | `.dalica.json` |

Rules:

- Reading is automatic during scanning; there is no import step. Options → General → "Read photo
  data left by the previous organizer" (default **on**) turns it off.
- Legacy files are **read-only**: never modified, renamed or deleted (users may still run the old
  program against the same folders). Once a folder has `.dalica.json`, later changes the old program
  makes to its own sidecar are not picked up.
- The first write converts everything the legacy sidecar holds for that folder, not just the photo
  that changed, so switching files never loses data. The conversion is verified by re-reading
  before the write is committed.
- Legacy edit operations are mapped to Dalica operations where an equivalent exists; operations
  without an equivalent are kept as opaque `legacy:<name>(…)` entries at the end of the recipe,
  shown in the edit panel as "1 edit from the previous program (not supported)".
- Album definitions and person names found in legacy sidecars become Dalica albums and people
  (matched by the legacy id, so the same album seen in many folders is one album).
- The legacy hidden originals directory is recognised and offered as the original for Revert and
  Undo Save; Dalica copies the original back from it and never writes into or removes anything
  from it.
- Which file is in effect for a folder (Dalica, legacy, both, none) is shown in folder Properties
  and reported by the control API.

## 10. Lessons encoded as tests

Each of these is a regression test in `crates/fs` / `crates/sidecar` / `crates/store` / `crates/engine`:

1. Rebuilding the index loses nothing (every user-visible field survives delete-index + rescan).
2. Moving a folder outside Dalica keeps all its data.
3. Moving photos between folders inside Dalica carries their data and their originals backup.
4. An unknown edit operation does not drop the operations after it.
5. A crash during a sidecar write leaves either the old or the new file, never a torn one.
6. Two Dalica processes on one library: the second refuses with a clear message.
7. File names with `]`, `"`, `;`, `=`, non-ASCII and very long names round-trip.
8. Metadata write-back preserves foreign XMP/IPTC fields byte for byte and never touches pixels.
9. Save → Undo Save → Save → Revert returns the exact original bytes.
12. An album whose members live in several folders keeps its exact order after deleting
    `albums.json` and the index and rescanning (position keys, §3.2).
10. Unplugging a volume and plugging it back loses nothing and queued edits are written.
11. A folder with only a legacy sidecar shows its data; the first change writes `.dalica.json` with
    all of it, the legacy file stays byte-identical, and a folder with both files reads only
    `.dalica.json`.
