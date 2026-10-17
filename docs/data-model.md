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
| Saved-edit backup and its record (the recipe saved, the original's size, time, pixel size and digest, pending or done) | — | — | yes (the record, read at scan and rebuild) | `.dalica-originals/<file>` + `.dalica-originals/.dalica.json` (§6) |
| Copy saved beside (a format Dalica cannot write in place) | `"savedAs": { "name", "recipe" }` | — | yes | |
| Project mark (a folder an export made) | `"folder": { "project": "exported" }` | — | yes | |
| Album membership and order | `"albums": { "<album id>": "<position key>" }` per photo (§3.2); the album names at the top level | — | yes | album definitions, covers and a copy of the order: `albums.json` (§4) |
| Last-changed time (when Dalica last changed what is said about a photo: Recently Updated) | `"changed": "2026-06-04T19:45:02.250Z"` | — | yes | |
| Search facts (camera, video, geotagged), content digests, first-indexed times | — | read from the file (facts, digests) | yes (only there) | |
| Folder description/date/place, hidden folder | `"folder"` object (`"hidden": true` in the folder's own sidecar, so it stays hidden when the folder moves) | — | yes | |
| Folder photo sort (Sort Folder By, Reverse Order) | `"folder": { "sort", "reverse" }` | — | yes | |
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
    "hidden": true,
    "sort": "name",
    "reverse": true,
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
      "albums": { "a91c0e": "a1", "a77d02": "bz0" },
      "geo": [38.7116, -9.1389],
      "date": "2026-06-04T19:42:10",
      "changed": "2026-06-04T19:45:02.250Z"
    }
  },
  "albums": { "a91c0e": { "name": "Best of 2026" }, "a77d02": { "name": "Trams" } },
  "people": { "p7f3a2c": { "name": "Ana Costa" } }
}
```

Format rules:
- The example shows the keys in a readable order; the file itself is printed with the keys of
  every object sorted by their UTF-8 bytes, at every level (so photos sort by file name), with
  list items in their own order. That is the documented key order every writer keeps.
- File names are object keys under `"photos"`, so JSON string escaping round-trips any name
  (including `]`, `"`, `;`, `=` and non-ASCII). `sort` is `manual | name | date | size | modified`.
- Lists (`order`, `tags`, `edits`, `redo`) are JSON arrays; a photo's `albums` is an object from
  album id to position key (§3.2). Each edit operation is one
  string in the operation syntax of §3.1, the same text the recipe API uses.
- Album ids are `a` and person ids `p`, followed by 4 to 16 lowercase hex digits (`a91c0e`,
  `p7f3a2c`); anything else is kept verbatim and reported (`security.md` §5.4).
- Coordinates are **normalised floats** in 0..1, at most six decimals, `[x0, y0, x1, y1]`, measured
  on the photo after its EXIF orientation and before Dalica's rotations and flips (§3.3).
- Unknown objects and keys are **preserved verbatim** on rewrite (forward compatibility). A value of
  the wrong type is kept as-is and reported once; it never aborts reading the rest.
- **The M6 fields** (built in M6.8: `dalica-sidecar` `fields`, `dalica-store` `mapping`). Per
  photo: `star`, `caption` (string), `tags` (list of strings), `hidden`, `edits` and `redo` (lists
  of operation strings, §3.1), `date` (a local date and time, `2026-06-04T19:42:10`; the EXIF
  spelling `2026:06:04 19:42:10` is read too). Per folder, in `"folder"`: `description`, `date`
  (`2026-06-04`), `place`, `hidden`. An empty value (`false`, `""`, `[]`, no date) is written as
  no key at all, and a photo entry or the `"folder"` object left with no keys goes; `"hidden":
  false` written by hand reads as not hidden and is left alone.
  - Reading: a value of the wrong type (a date that does not parse counts) is not used, the model
    keeps its own value, and the import reports it once. Text past the caps of `dalica-library`
    (caption and description 16 KiB, place 1 KiB) is shown cut and kept whole in the file; a tag
    list is cleaned (trimmed, no two the same ignoring case, at most 1 000); a recipe or redo list
    past 10 000 operations or with one over 64 KiB is not used and is kept in the file. An unknown
    operation is just another string: it keeps its place and the operations after it (§10.4).
  - Writing changes only the keys whose value differs from what the file reads as, so a file that
    already says what the model says keeps its bytes (a cleaned tag list or an EXIF-style date
    stays as written). A value of the wrong type is written over only when the user sets that
    field; a `photos` entry or `"folder"` that is not an object is never written over.
  - The last-synced metadata hashes (`xmp-interop.md` §5) are index-only: never in a sidecar, and
    empty again after a rebuild.
- **The M8 fields** (built in M8.9: `dalica-sidecar` `fields`, `dalica-store` `mapping`). Per
  photo: `savedAs`, the JPEG a Save wrote beside a photo Dalica cannot write in place (§6), an
  object `{"name": "IMG_0001.jpg", "recipe": ["sepia()"]}`: the copy's file name in the same
  folder and the recipe it was rendered with (an empty recipe is written as no key). Per folder,
  in `"folder"`: `project`, the folder's project mark; `"exported"` (a folder an export made) is
  the only kind so far. Both follow the M6 rules above; besides:
  - `savedAs` is used only when `name` is one plain path component (`security.md` §5.4) and the
    recipe is within the caps; otherwise, like a value of the wrong type, it is kept, reported
    and not used. Writing a new copy keeps the keys of the `savedAs` object Dalica does not
    know.
  - A `project` kind Dalica does not know is kept and reported, and the folder is not marked.
  - The saved record itself is never in the folder sidecar: it lives in the originals record
    (§6), so the sidecar stays what the user decided.
- **The M9 fields** (built in M9.6: `dalica-sidecar` `fields` and `albums`, `dalica-store`
  `mapping`, `albums` and `rebuild`). Per photo: `albums`, an object from album id to position key
  (§3.2). Per folder, in `"folder"`: `sort` (`name`, `size`; `date`, the default, is written as no
  key) and `reverse` (written only when `true`). At the top level: `albums`, an object from album
  id to `{"name": …}` naming every album with a member in the folder, so an album can be
  recovered when `albums.json` is lost. Each follows the M6 rules above; besides:
  - An entry of either `albums` object whose id is not an album id, whose position key does not
    parse (`dalica-core` `poskey`, at most 512 bytes) or whose name is not a non-empty string is
    kept verbatim, reported once and never used; a write never touches it (a membership the model
    sets for the same album replaces it). An `albums` that is not an object is a value of the
    wrong type.
  - The model is the truth for the memberships of its own albums and for the folder's album
    names: a write sets and clears those. A membership of an album the library does not have
    (deleted here, or never in this library) is kept as written; with no name for it anywhere it
    is reported and not used.
  - Album names are cleaned as Album Properties cleans them (control characters made spaces,
    trimmed, cut to 1 KiB); a name that is nothing once cleaned is not used.
  - A `sort` Dalica cannot sort by yet (`manual`, `modified`, anything else) is kept, reported and
    not used, and the folder keeps its own sort until the user sorts it.
  - The search facts, content digests and first-indexed times are index-only: never in a
    sidecar, and empty again after a rebuild. The last-changed times were index-only too until
    M10, which put them in the sidecar (below).
  - A scan reading a folder sidecar follows the same rules: `dalica-store` `mapping::read_photo`
    (`albums`), `mapping::read_folder` (`sort`, `reverse`, `FolderReading::photo_sort`) and
    `mapping::read_album_names` (`dalica-scan` reads them on the scan's thread with the same
    rules, a test holding the two together). **Built (M9.10).** As for the M6 fields, only what
    the model does not know comes in, in the op that adds it, so no sidecar write can run in
    between: a new folder takes its sort, a new photo its memberships with their keys; an album
    the model lacks is created with its definition from `albums.json` when it has one, else under
    the name the folder gives it (dated by its newest photo); a membership of an album named
    nowhere, or deleted in this session, stays in the file unused. A folder whose sidecar cannot
    be read takes its photos' memberships from `albums.json`'s copy of the order. A rebuild's
    scan first waits until every write sent before it landed (sidecars, `albums.json`), so it
    never reads a file a pending write would change.
- **The M10 field** (built in M10.29: `dalica-sidecar` `fields`, `dalica-store` `mapping` and
  `rebuild`, `dalica-scan` `walk` and `diff`). Per photo: `changed`, when Dalica last changed what
  is said about the photo (a star, a caption, tags, hidden, the recipe, a date, album places, a
  move or rename), as a UTC time with milliseconds (`"2026-06-04T19:45:02.250Z"`). Recently
  Updated reads it (`organizing.md` §4.1), so a rebuild keeps the list. It follows the M6 rules
  above; besides:
  - A sidecar without it says no change was recorded; a value that is not such a time (a number,
    no `Z`, other than three digits of milliseconds) is kept, reported once and never used.
  - Every command that changes a photo's data sets it in the same undo step, so the same write
    carries it; an undo puts the earlier time back (or removes the key). An entry left holding only
    `changed` is still Dalica's data: it is marked and pruned like any other when its file is gone.
  - A scan brings it with a photo new to the model (in the op that adds the photo, like `savedAs`);
    for a photo the model knows, the model's word stands.
  - Only Dalica's changes are kept here: a file changed on disk counts for Recently Updated by its
    own modification time against the first-indexed time, which is index-only, so after a rebuild
    only the changes made in Dalica remain in the list.
- Readers accept a BOM and CRLF; a duplicate key takes the later value. Nesting is bounded
  (`security.md` §8).
- `dalica.missing` is Dalica's own bookkeeping for stale entries (§7): an object from a photo's
  file name to the UTC time (`"2026-10-12T05:15:00Z"`) Dalica first found that photo's file gone
  while its entry still held Dalica's data. A name leaves it when its file comes back or its entry
  is pruned; the key goes when it is empty. **Built (M5.17: `dalica-store` `stale`).**
- Writers write only when the content changed, and never more often than once per second per
  folder (debounced); a crash loses at most the last second of changes in that folder, never the
  file.
- If a sidecar is unreadable, it is renamed to `.dalica.json.corrupt-<timestamp>` and kept; the
  folder is shown with a `caution` banner; nothing in it is written until the user chooses
  "Reset folder data" or restores the file. **Built (M10.52: `crates/engine/src/cmd/folder_data.rs`,
  the banner in `crates/ui/src/panels/light_table/folder_banner.rs`):**
  - The first write that meets the file (a scan's, an edit's, a carry's) keeps it aside and lists
    the folder as damaged. Its changes stay in the library (and the journal) but nothing is
    written there: no sidecar write and no entry carried in or out by a move. Metadata write-back
    into its photos (captions, tags and dates into their XMP/IPTC) is held too until the reset
    (M10.57): the write stays pending without keeping the app busy, is not made at close, is sent
    when the folder's state is lifted (a hold from before a restart included, as the library still
    marks the field unwritten), and is dropped only for a photo that left the folder first, which is
    reported; the library keeps the value either way.
  - The list is kept in `damaged-folders.json` in the library directory (§4), by folder path, so a
    restart, or an index rebuilt from the folders, does not quietly start writing into the folder
    before the user chose.
  - **Reset folder data** (`folder.resetData {folder, confirm}`, the banner's button) asks first.
    It reads the folder's sidecar again: a readable one (restored by hand) gives the library its
    values, as a rebuild reads them, and nothing is written by the reset; none there, a new sidecar
    is written from the library, what was set while the folder was damaged included; an unreadable
    one there (a second damage) is kept aside under a name of its own (`…-001`, never replacing the
    first copy) before the new one is written. The kept copies are never read, renamed, overwritten
    or deleted, and the reply names them. A second damage after a reset is found and listed like
    the first.
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
| `fill_light(a)` | 0..1 | the Basic Fixes slider as older recipes store it: read and rendered as `tuning(a,0,0,0,none)` and kept as written; Dalica writes `tuning` |
| `tuning(fill,highlights,shadows,temp,neutral)` | 0..1, 0..0.5, 0..0.5, −1..1, `#rrggbb` or `none`; defaults `0,0,0,0,none` (no change) | the whole Tuning tab is one operation, updated in place while the tab is open; the Basic Fixes Fill Light slider is its `fill`. Numbers out of range are clamped when read; `NaN`, a missing field or a bad colour leaves the entry kept and skipped |
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

### 3.3 The coordinate frame

**Decided 2026-10-12 (the maintainer; reserved decision 11, option (b)).** Every stored normalised
rectangle and point is measured on the photo **after its EXIF orientation and before Dalica's own
rotations and flips** (the *stored frame*): the `crop` rectangle, `retouch` source and target
points, the centre points of the focal effects (Soft Focus, Focal B&W, Graduated Tint, Focal Zoom)
and face rectangles (`"faces"`). The picture the user sees is that photo turned by the recipe's
rotations and flips (the *displayed frame*). **Built (M7.1: `dalica-edit` `frame`).**
- `(0, 0)` is the top-left corner of the frame and `(1, 1)` its bottom-right corner, `x` across and
  `y` down, whatever the photo's size; a rectangle is stored with its corners in order
  (`x0 ≤ x1`, `y0 ≤ y1`).
- Coordinates are converted only to draw them and to read the user's clicks and drags back:
  `edit::frame` maps a point or a rectangle from the stored to the displayed frame by the recipe's
  net rotation and flip (one of the eight, `Recipe::orientation()`) and back by its inverse. The
  round trip gives the stored value back (exactly for values such as `k / 2ⁿ`, otherwise to within
  one rounding of `1 − v`).
- A rotation or flip is therefore one recipe entry and nothing else: adding or undoing it never
  rewrites a crop, a retouch stroke, an effect's centre or a face rectangle, and an unknown
  operation in the recipe never has to be understood to keep its coordinates right.
- Faces are found on the upright decode (EXIF orientation applied), which is the stored frame, so
  their rectangles stay valid however the photo is turned later. XMP face regions (on the
  EXIF-upright photo, [`xmp-interop.md`](xmp-interop.md)) are in the same frame and need no
  conversion.
- Lengths relative to the photo's shorter side (retouch and effect radii) are the same in both
  frames. An angle measured in the stored frame (`straighten`) turns the other way on screen when
  the recipe mirrors the photo.
- Rendering does the crop and the straighten in the stored frame, then the recipe's rotations and
  flips, wherever they stand in the recipe (the same picture as turning first and cropping with the
  converted rectangle).
- Save (§6) writes the rotations and flips into the pixels and clears the recipe, so the photo's
  stored frame becomes the displayed one: Save maps the rectangles and points it keeps (face
  rectangles) into the new frame with the same functions, and Undo Save maps them back.

## 4. Library directory (index and global data)

The library folder (`apps/dalica/src/paths.rs`), in this order: `$DALICA_HOME` when it names an
absolute folder (tests and agents); in portable mode (a `portable` file beside the executable)
`data/library` next to it; else the platform's data folder: Linux `$XDG_DATA_HOME/dalica/library`
(default `~/.local/share/dalica/library`), macOS `~/Library/Application Support/Dalica/Library`,
Windows `%LOCALAPPDATA%\Dalica\Library`. `--library DIR` opens another folder as the library. It
holds:

| File | Contents | Rebuildable? |
|---|---|---|
| `folders.json` | the Folder Manager's settings, one entry per folder that sets its own: `{"version": 2, "folders": [{"path", "mode"?: "always" \| "once" \| "remove", "faces"?: bool, "indexed"?: true}]}` (`indexed`: a Scan Once folder's first scan finished; an entry without `mode` sets only the faces flag; the `remove` entries are the exclusions). A v1 file (M1: `{"version": 1, "folders": [{"path"}]}`, every folder watched) is read as Scan Always entries and written back as v2 at once, atomically; keys and entries this version does not know are kept verbatim | no — small, backed up with settings |
| `albums.json` | album definitions (id, name, description, date, use the newest photo's date, place, cover) and a copy of the order, below | yes, from sidecars (each member folder carries the album's name; each member its position key, §3.2) |
| `people.json` | people (id, name, nickname), merges, settings | yes, from sidecars (names are mirrored per folder) |
| `index.log` + `index.snap` | the library index: one record per photo (path, size, mtime, dimensions, dates, EXIF summary, content hash, colour sketch, face descriptors), what the user said about photos, folders and albums (as in the sidecars and `albums.json`), the last-synced metadata hashes, the search facts, content digests and first-indexed and last-changed times; journal + snapshot as in LightCraft | yes, by rescanning (the synced hashes, facts, digests and first-indexed times start empty; the last-changed times come back from the sidecars) |
| `pending-writes.json` | the folders whose sidecar write waits for an offline volume (§7): `{"version": 1, "folders": [path, …]}`, written through the I/O worker when the list changes; a damaged file is kept as `.corrupt-<time>` and the list found again by the next online checks (M5.13) | yes, partly: the journal keeps the edits; a lost list is rebuilt for roots found offline |
| `damaged-folders.json` | the folders whose sidecar was damaged and kept aside (§3), until the user resets them: `{"version": 1, "folders": [{"path", "kept": [path]}]}`, written through the I/O worker when the list changes, read at open; a damaged file is kept as `.corrupt-<time>` and read as empty (a notice says so; such a folder is found again by the next write that meets it, and one whose damaged file was already kept aside gets a new sidecar then) (M10.52) | no — small; a lost list only loses the warning |
| `file-ops.json` | the file-operation journal: the moves, copies, renames and trashes in progress and each item's state (§8), written through the I/O worker; a damaged file is kept as `.corrupt-<time>` and the operations that read are kept (M9.9) | no — repaired from at the next open, then empty |
| `lock` | single-process lock | — |
| `settings.json` | preferences (atomic write; damaged file kept as `.corrupt-<time>`); unknown keys kept; `"keymap"` holds the user's shortcut overrides (`menus-and-shortcuts.md` §2.0); the other keys below | no |

**`settings.json`** (`crates/engine/src/settings.rs`) is one JSON object of objects, each merged key
by key when written, so keys and objects this version does not know are kept verbatim. A value of
the wrong type or out of its range reads as its default and is reported once, by key (never by
value), when the library opens. The objects:

| Object | Keys (default) | Since |
|---|---|---|
| `keymap` | a list of `{command, context, key}` overrides (`key: null` removes a binding) | M1 |
| `window` | `{x, y, width, height, maximized}`: the window as last seen | M3 |
| `view` | `{treeWidth, pane, paneLarge, thumbSize, treeMode, folderSort, folderSortReverse, caption, smallPictures, hiddenPictures, colorManagement}`: the view restored at the next start | M3–M5 |
| `interface` | Options → General → Interface: `systemTitleBar` (off; M3), and from M10 `animations` (on), `tooltips` (on), `uiScale` (`"auto"`, or a percentage 75–300 in steps of 25), `fontSize` (`"small"`, `"medium"`, `"large"`; small), `language` (`"system"` or a language tag; system) | M3, M10 |
| `scrolling` | `{thumbExponent, thumbTopSpeed}`: the speed-control thumb's curve, read only | M4 |
| `fileTypes` | Options → File Types: `png`, `gif`, `bmp`, `tiff`, `webp`, `heif`, `avif`, `jxl`, `psd`, `raw`, `video` (each on unless `false`; JPEG always on), and from M10 `smallPictureThreshold` (64 px, 16–1024) | M5, M10 |
| `cache` | `{thumbnailBytes}` (2 GiB) | M5 |
| `files` | Options → General → Files: `writeCaptionsTagsPlaces` (on), `writeStarsAsRatings` (off), `updateModificationTime` (off), `writeRawXmpSidecars` (off), `writeDates` (on) | M6 |
| `tags` | `{quick, useRecent, recent}`: the quick tags and the recently used tags | M6 |
| `edit` | `{cropRatios, lastCropRatio, photoSizes, singleClickLeaves}` (the last is Options → General → "Single click on the photo returns to the library", off) | M7 |
| `save` | `{explain, lastFolder, lastFormat}` (§6) | M8 |
| `export` | `{location, resize, size, quality, customQuality, numbers, watermark, watermarkText, removeLocation}` (`creating.md` §2; Options → Export) | M8 |
| `organize` | `{askBeforeMoving, removeFromAlbumsWithoutAsking, deleteWithoutAsking, recentDays, lastMoveFolder}` (`organizing.md` §4, §9) | M9 |
| `slideshow` | Options → Slideshow (`viewing.md` §1.5): `loop` (off), `displayTime` (3 s, 1–60), `transition` (`"cut"`, `"panAndZoom"`, `"dissolve"`, `"wipe"`, `"circle"`, `"push"`, `"rectangle"`; dissolve), `captions` (on), `music` (off), `musicFolder` (none), `videoSound` (off) | M11 |
| `viewer` | Options → Photo Viewer → Interface (`viewing.md` §3.3): `wheel` (`"zoom"` or `"navigate"`; zoom), `hideControls` (on), `translucent` (on) | M11 |
| `import` | `folder` (Options → General → "Save imported pictures in"; unset: the Pictures folder), `detectDuplicates` (on), `after` (the room's "After copying", `"leave"` or `"delete"`; leave), `recentSources`, `recentDestinations` (absolute folders) and `recentFolderNames`, each the last 8, most recent first (`organizing.md` §3) | M11 |
| `compare` | what "Don't ask again" remembers in compare (`editing.md` §4): `askBeforeApplying` (on) and `keep` (`"ask"`, `"left"` or `"right"`; ask) | M11 |

The Options dialog's rows name these keys (`menus-and-shortcuts.md` §5). An option whose feature
arrives later (`files.readLegacyData`, `interface.language`, `slideshow.music`,
`slideshow.musicFolder`, `slideshow.videoSound`) is never written by the Options dialog until that
feature reads it; `viewer.defaultViewer` is the catalogue's row for the Photo Viewer's file types
and has no key of its own until the OS integration.

**`albums.json`** (built in M9.6: `dalica-sidecar` `albums`, `dalica-store` `albums` and
`rebuild`), printed like the folder sidecar (UTF-8, two-space indentation, LF, keys sorted by
their bytes at every level, unknown keys and entries kept verbatim):

```json
{
  "albums": {
    "a91c0e": {
      "cover": "/home/ana/Pictures/Lisbon/IMG_0002.jpg",
      "date": "2026-06-04",
      "description": "The best of the year",
      "name": "Best of 2026",
      "order": [["/home/ana/Pictures/Lisbon/IMG_0002.jpg", "a0"], ["/home/ana/Pictures/Porto/IMG_0107.jpg", "a1"]],
      "place": "Portugal",
      "useNewest": true
    }
  },
  "dalica": { "version": 1 }
}
```

- One entry per album under `"albums"`, keyed by its id. `name` is required; `description`,
  `place`, `date`, `useNewest`, `cover` (the cover photo's absolute path) and `order` (the members
  in album order, each `[absolute path, position key]`) are absent when empty. Photos are named
  by path because photo ids belong to one index. A path that is not UTF-8 is left out of the copy.
- The memberships live in the folder sidecars; the order here is a copy, used only for the photos
  of a folder whose sidecar is damaged or cannot be read.
- An entry whose id is not an album id, that is not an object or that has no usable name is kept
  and reported, never used; a known key of the wrong type reads as empty and is reported; text
  past the library's caps is shown cut and kept whole.
- It is written after album changes, merged into the file on disk (entries of albums the library
  no longer has go), only when its bytes change, through the atomic replace over the version just
  read. A damaged file is set aside as `albums.json.corrupt-<time>` and reported, and a new one
  written. **Built (M9.10: `crates/engine/src/albums_file.rs`).** The engine writes it on the I/O
  worker after every op that changes an album, a membership or a cover, or moves, renames or
  removes photos or folders while there are albums; it is held while scans apply what they found
  and while a rebuild runs (written once they are done), and read when the library opens (a
  damaged or missing one is written anew from the model; one that reads is never written over
  at open). "No longer has" means albums the session saw go: an entry of an album the session
  never had in its model (the index was lost and no scan brought it back yet) is kept as the
  file says it.
- **Rebuilding the albums:** definitions and covers come from `albums.json`; an album it lacks
  whose name a folder's top-level `"albums"` carries is created under that name (dated by its
  newest photo; description, place and cover empty), so losing `albums.json` loses no album and no
  order. Memberships come from every folder sidecar, sorted by key (§3.2). Deleting an album
  removes its name from the folders, so it never comes back from them. In the engine (M9.10) the
  rebuild's scan reads `albums.json` on its thread before the folders; once every folder is in,
  the albums it defines that no folder brought back (an empty album) are created, its definitions
  given and its covers set where the photo is a member. A rebuild into an empty model (a lost
  index) does this even when cancelled, so `albums.json` written afterwards still holds every
  album. Deleting an album (or undoing its creation) leaves its photos' entries in the sidecars
  as written, unnamed and never used again (an album the library does not have is not the
  model's to write).

`<cache>/thumbs/` in Dalica's cache folder (`control-protocol.md` §2.1: Linux
`$XDG_CACHE_HOME/dalica/thumbs`, macOS `~/Library/Caches/Dalica/thumbs`, Windows
`%LOCALAPPDATA%\Dalica\cache\thumbs`; `$DALICA_CACHE_DIR/thumbs` when the `DALICA_CACHE_DIR`
environment variable names an absolute folder, which tests set so they never write to the user's
cache) holds the thumbnail/preview disk cache: pack files plus an
index per size of the thumbnail ladder (`architecture.md` §6), 2 GiB default, deletable at any
time. Demos and tests keep a temporary cache that goes with their session; a cache another Dalica
process holds is not shared (the session falls back to a temporary one and says so). Face detection descriptors are cached in the index; face *assignments* live in sidecars.

"Rebuild library" (Help → Troubleshooting) deletes the index and rescans; it must never lose user
data, and a test proves it (scan → edit everything → rebuild → compare).

## 5. Writing metadata into files

> Status: built for captions, tags, stars and dates (M6.30 the write queue, M6.33 the session's
> write-back: `crates/engine/src/metadata.rs`); places and people wait for their model fields
> (M13). Edits and rotations reach pixels only on Save (M7).

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

- **What is written:** the fields a command changed (undo and redo included), each only while its
  option is on (Options → General → Files); changes that came from the files themselves (scans,
  imports) are never written back. A photo's changes go out together, in one write, once it has
  been left alone for a second (the queue's debounce).
- **The writer** runs on a thread of its own; due writes run as a "Writing metadata" job
  (`metadataWrite`, progress, Cancel). Cancel drops the writes not made yet: the folder sidecar
  keeps those values, and the next change of the field writes it. Writes waiting for an offline
  volume are retried on their own and land when it returns. When the library closes, the writer
  writes what is still queued first.
- **What comes back:** the last-synced hashes (index only); a value another application changed
  is imported as one undoable step ("Import Metadata"); both changed is reported by
  `photo.metadata` ("Changed in another application; Dalica's value kept"); a file Dalica wrote
  takes its new size and time in the library, so a rescan does not mistake the write for an
  outside change; a failed write becomes one toast naming the file and the fields.
- **Before a Save, Undo Save or Revert reads a photo**, its own queued write is made at once on
  the writer thread (only that photo's; one waiting for an offline volume is not tried), and the
  Save expects the version Dalica's own writes left the file at, starting from the version the
  library knew. A caption set just before Save is therefore written and the Save goes ahead; a
  version no write of Dalica's explains (a change made elsewhere) is still refused as changed on
  disk.
- `library.inspect` → `pendingWrites.metadata` counts the photos still to write.

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
4. Record the recipe, the original's size, mtime, dimensions and digest in
   `.dalica-originals/.dalica.json` (the originals record, below); clear `"edits"` from the folder
   sidecar.
5. For files Dalica cannot write in their own format (RAW, PNG with alpha, GIF, video), Save writes
   a JPEG next to the source (`name.jpg`, or `name-001.jpg` on collision) and leaves the source
   untouched.
A Save whose recipe holds only rotations and flips is **lossless** for JPEGs (DCT-domain rotation when dimensions allow; otherwise
the EXIF orientation flag is updated instead). The DCT-domain transforms (`dalica-image`
`jpeg_transform`, M8.8) keep the file's own quantisation tables: the transposing transforms
(EXIF orientations 5–8, the quarter turns and the transposes) transpose each table along with the
coefficients, so every coefficient keeps its own quantiser and nothing is requantised; the others
copy the tables byte for byte. A file with data after its end-of-image marker (MPF secondary
images, trailers) is refused and takes the orientation-flag fallback, since that data's offsets
and orientation would go stale.

**The originals record** (`.dalica-originals/.dalica.json`; built in M8.9: `dalica-sidecar`
`originals`, `dalica-store` `mapping` and `rebuild`, `dalica-scan` `originals`). One per folder
that has a kept original, written by Save, Undo Save and Revert through `fs`'s atomic replace
(M8.12) and printed like the folder sidecar (UTF-8, two-space indentation, LF, keys sorted by
their bytes at every level, unknown keys kept verbatim):

```json
{
  "dalica": { "version": 1 },
  "originals": {
    "IMG_0001.jpg": {
      "digest": "md5:9e107d9d372bb6826bd81d3542a419d6",
      "height": 3000,
      "mtime": 1780000000123,
      "recipe": ["rotate(90)", "autofix()"],
      "size": 2483112,
      "state": "done",
      "time": 1791000000000,
      "width": 4000
    }
  }
}
```

- One entry per kept original under `"originals"`, keyed by the photo's file name (the backup's
  name in `.dalica-originals/`). `recipe`: the whole recipe the saved file holds (a second Save
  renders it followed by the new edits, from the kept original, so quality loss never compounds);
  `size`: the original's bytes; `mtime`: its modification time in milliseconds since 1970 (UTC);
  `width`, `height`: its size in pixels as displayed (0 when unknown); `digest`: the original's
  bytes hashed, written `md5:<32 lowercase hex digits>`; `state`: `pending` (written before
  anything moves) or `done` (written last); `time`: when the Save was made, in milliseconds since
  1970. `size`, `mtime` and `state` are required; the others may be absent (empty, 0, unknown).
- The digest only confirms that a backup is still the original that was recorded, before Undo
  Save or Revert moves it back (`fs` `originals`, M8.2); an empty digest, or one under an
  algorithm Dalica does not know, cannot confirm it, and the backup is then left alone.
- An entry is used only when its name is one plain path component (`security.md` §5.4: `../x`
  and `CON` are not) and every key Dalica knows has the right type, with the recipe and the digest
  (at most 256 bytes) within the library's caps. Anything else is kept in the file, reported once,
  and never reaches a file or the model.
- The record is index-only data for the library: the scan reads it on its worker for each folder
  it walks (the walk itself still skips `.dalica-originals`, so the kept originals are never
  photos of the library) and a rebuild reads it back, both turning it into the photos' saved
  records as derived data (no undo step, no revision bump). A photo whose name has no usable entry
  has no saved record; a record that cannot be read leaves the model's records as they are.
- `.dalica-originals` or the record is never reached through a link; a file in place of the
  folder is reported and nothing in it is read.
- A damaged record (not JSON, not an object, past the limits of `security.md` §4.2) is renamed to
  `.dalica.json.corrupt-<time>` beside it and reported; the photos keep their saved records in the
  index, and the next Save starts a new record without writing over the old one.
- A pending entry found at open or by a scan is a Save that was interrupted: it is repaired,
  rolled forward when the new file is complete and verifies, else rolled back (the original moved
  back to its name and the entry dropped). A `done` entry whose kept original is gone from the
  backup while the file at the name is that original is an Undo Save or Revert that stopped
  before dropping it; the repair drops it (`dalica-create` `save::recover`, M8.18):
  - **Forward** only when the kept original is the recorded one and the file at the name holds
    the recorded recipe: it is not the original, not the version the library last saw there (an
    earlier Save's file), it decodes whole, and its pixels are the original rendered again with
    the recipe through the output pipeline within a small tolerance (`TOLERANCE`, 3 levels on
    average). The check is by content, so it never depends on clocks.
  - **Back** otherwise, when the recorded original is in hand: moved back over whatever is at the
    name (that is a rendering the recipe can make again) or found still there, then the entry
    dropped. The library puts the recorded recipe back as unsaved edits, in front of the edits not
    in it, so an interrupted Save, Undo Save or Revert never loses an edit.
  - **Left** as found, with the reason, when the recorded original is neither kept nor at its
    name, or the backup is not the recorded one: nothing is moved or dropped.
  - The record follows the files (an entry is marked or dropped only after the move), so a repair
    that is itself interrupted is finished by the next. The atomic-replace temporary files of the
    record and of the photos it names are removed by their shape, only those older than the
    repair.
  - The engine checks, reading only, the folders that can hold one when a session opens (photos
    with unsaved edits or a saved record) and repairs those that need it in one file job; a scan
    that reads a pending entry starts the same job unless a file job is running. Each repair is
    logged and listed in the session's warnings, and the job's end is a notice.

**How a Save in place runs** (`dalica-create` `save`, M8.12; on a worker, every disk step through
`dalica-fs`). The record's entry is the journal: it is written before anything moves and marked
done last, so at every moment the original's bytes are at the photo's name or in
`.dalica-originals/` with an entry naming them.

1. **Refuse** a recipe holding an operation Dalica cannot render: an unknown or `legacy:`
   operation, a known one without a renderer yet (the effects until M12), or parameters its module
   refuses. The error names the operation; baking pixels never drops one.
2. **Read** the photo whole with its identity; a read-only file, a name that is not one plain
   component, and a file whose size or modification time is not what the library last saw are
   refused. Read the folder's record: a `done` entry means the photo was saved before, and its kept
   original is read and checked against the entry's size and digest; a `pending` entry is an
   interrupted Save, repaired first; a backup that no entry names is never replaced.
3. **Render** through the one output pipeline at Match quality, with a fresh EXIF thumbnail and the
   owned fields merged under the write-back options: from the photo on its first Save, else from the
   kept original with the recorded recipe followed by the new edits. Nothing on disk has changed.
4. **Write the entry `pending`** (the whole recipe; the original's size, modification time, pixel
   size and digest; the time) through the atomic replace, over the version of the record just read.
5. **Keep the original:** a first Save moves the photo into `.dalica-originals/` by one rename that
   never replaces, after checking its identity once more; a later Save checks the kept original
   again and leaves it.
6. **Commit** the new file at the photo's name: where nothing is (a first Save just freed the
   name), or over the saved file only while it is still the version read in step 2.
7. **Verify** the file on disk: the bytes written, the pixel size, orientation 1, the owned fields.
8. **Mark the entry `done`.**

An entry's states are therefore: none (never saved, or Undo Save or Revert done), `pending`
(steps 4–7: a Save under way or interrupted) and `done` (a saved photo whose original is kept). A
refusal or failure up to step 5 takes the entry back to what it was and changes nothing (a first
Save whose move into the backup was made before a later part of step 5 failed, its `fsync` or its
check, moves the original back first); a failed
step 6 of a first Save moves the original back when nothing took its name, and a failed step 6 of a
later Save that wrote nothing takes the entry back. Any other failure leaves the entry `pending`
for the repair above.

**Undo Save and Revert** move the kept original back over the saved file, only while the backup is
still the recorded original (its size and digest) and the saved file is the version the library
last saw (a saved file that is gone takes the original back into its free name), then drop the
entry. Undo Save answers the recorded recipe to put back; Revert answers none.

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
watched it is added as Scan Always so the new file appears in the library. The format follows
the chosen name's extension (`.jpg` or `.png`); a name without one takes the format chosen, else
the last one used. The dialog suggests the photo's name with the extension of the format Save As
last wrote, in the folder it last wrote into (the photo's own folder until then), numbered like
Save a Copy's (`IMG_0041-001.jpg`) when the library already knows a file of that name there. The
app follows the save it started: a name that turns out to be taken opens the dialog again, titled
"Choose Another Name", with that name in its folder; a folder that is gone opens it again in the
photo's folder; the status line and a toast give the reason. The copy, from either command, is a
new photo without edits that keeps the photo's caption, tags, star and date (`photo.saveCopy`,
`photo.saveTo`; M8.17; the app's half M8.22).

**Export** never changes originals: it renders copies into a chosen folder (see
[`creating.md`](creating.md) → Export), adds the export folder to the library as an
**Exported** collection member (adding it to the watched folders as Scan Always when it is outside
them), and names collisions `-001`.

Photos with unsaved edits show the *edited* badge; folders with unsaved edits show a Save button in
their folder header.

## 7. Scanning and watching

Scans never stop or block the user (maintainer, 2026-10-14). Opening a library reads its index and
shows it; scans, rescans, the verification pass and thumbnails all run in the background, shown by
the activity indicator (click it for their status). Closing the app mid-scan is safe: what was
found is already in the index, and the verification pass walks the folders again after the next
start. The opening screen names what it is doing and counts the seconds when it takes a while. The
one exception is a brand-new demo, which scans its three small folders before it shows them, so its
ids are the same everywhere (`control-protocol.md` §5.4).

- **Folder Manager** modes per folder: **Scan Always** (watched: `notify` events + periodic
  verification), **Scan Once** (indexed once, then frozen — for removable or slow media),
  **Remove from Dalica** (not indexed; nothing on disk changes). Setting a mode on a folder applies
  to its subfolders unless they have their own setting.
  - A folder's **effective mode** is that of its nearest ancestor (itself included) that sets one;
    below no setting a folder is not indexed. The "Find faces in this folder" flag inherits the
    same way (on when nothing sets it).
  - Each folder set to Scan Always or Scan Once is a **unit** that owns its subtree minus the
    folders inside it with a setting of their own. A scan walks one unit's region at a time (the
    folders inside it that set a mode are never entered); a rebuild walks every unit, entering a
    removed folder only for a unit inside it and keeping nothing found there.
  - A change (`folders.set`, the Folder Manager's OK; `folder.remove`, the context menu) is one
    library undo step: the folders under a changed folder that are no longer indexed leave the
    library (photo files and sidecars untouched), and regions that became indexed are scanned in
    the background (newly watched, or indexed for the first time). Undo puts back both the
    settings and the folders with their photos, ids and stars; a Scan Once region brought back by
    undo is not scanned again.
  - A Scan Once unit is frozen once its first scan finished: the verification pass rescans the
    Scan Always units and the Scan Once units not indexed yet, never a frozen one. Switching it to
    Scan Always scans it.
  - Setting a volume root or the home folder to Scan Always asks first (`organizing.md` §2); over
    automation the command needs `"confirm": true`, as `folder.remove` does.
- **First run** offers: "Scan only my Pictures, Documents and Desktop folders" (default) or
  "Choose folders…". Scanning the whole disk is not offered.
- **Recognised files:** JPEG, PNG, TIFF, WebP, GIF, BMP, HEIC/HEIF (if built with support), AVIF,
  JPEG XL, PSD (composite), the RAW formats supported by the codecs crate; videos MP4/MOV/M4V/MKV
  (indexed with a poster frame when decodable, otherwise a generic video
  tile). Per-type toggles in Options → File Types (`settings.json` → `fileTypes`, one boolean per
  type id; JPEG is always on), given to every scan.
- **Skipped:** dot-folders and dot-files, `.dalica-originals`, OS system folders, cache/temp
  folders, images smaller than 64 × 64 (shown only with View → Small Pictures).
- **External changes:** created/deleted/renamed files and folders in watched roots are picked up
  within seconds; a file whose (size, mtime) changed is re-read; a folder moved outside Dalica keeps
  its data because the sidecar moves with it. A file deleted outside Dalica disappears from the
  library; its stale sidecar entry is pruned after 30 days or on "Clean up folder data".
  **Built (M5.17)** as follows (`dalica-engine` `watching`, `dalica-store` `stale`):
  - **Events are hints, verification is the truth.** The outermost Scan Always units are watched
    on a worker thread through the system's change events, or a pure-Rust poller where those cannot
    be had (network shares, watch limits, lost roots; M5.11). Folders set to Remove or Scan Once
    inside them are left out. Changes are debounced (500 ms quiet, 5 s at most) on the session
    clock and handed out as dirty folders.
  - **Rescans:** the dirty folders of each unit are scanned again (only them: a folder alone, or a
    new or renamed one with its subtree) on a worker thread, once no scan is running, as a `scan`
    job; the result is applied a few hundred photos per frame and diffed again if anything else
    changed the library meanwhile, so an edit made in between is never overwritten. A file renamed
    in place (same size and modification time) keeps its photo id and star; a renamed or moved
    folder keeps its folder and photo ids, stars and sketches, and everything else the model said
    about it and its photos (captions, tags, recipes, dates, memberships and covers, its sort, the
    index-only facts and times: M9.10), put back right after the op that moved it; a folder moved
    in from elsewhere brings its stars and the rest of its sidecar, album memberships included. Rescans and the verification pass wait while a file job
    (Save, Undo Save, Revert, Save a Copy, Export, a move, copy, rename or delete) is writing, and
    a rescan or a full scan (start-up, the verification pass, a folder just added, a rebuild)
    running when one starts is put off, its job ending as "put off while files are written", and
    walks again after the job: a scan overlapping the job's writes would read some files before
    them and some after, and could put back a version the job's result has moved the library past
    (a saved photo's old size, a record without its entry). What a put-off scan already applied
    stays; it removes nothing, and a put-off rebuild keeps the old model.
  - **Verification pass:** each Scan Always unit (and each Scan Once unit not indexed yet) is
    scanned whole 6 hours after a scan last walked it, 60 s after the session's first frame if no
    scan walked it since the session opened (changes made while the app was closed), and again a
    minute later if it could not be walked (offline). It catches anything the events missed; with
    the watcher switched off it alone finds every change. Each unit is its own scan, and a folder
    moved from one unit into another is still one folder that moved (§10.2): a folder holding
    photos that one scan finds gone stays in the library while another merge scan runs, a folder
    another scan finds at an unknown path waits until every running merge scan's walk is over and
    then pairs with it (by its photos, as a rename), and only the folders no scan paired leave the
    library, once no merge scan runs.
  - **Never remove what was not seen:** only what a scan walked to the end may leave the library.
    A root that cannot be walked (offline) keeps everything, and so does a walked root that holds
    no entries at all (the empty mount point an unmounted volume leaves behind; a warning says so
    when the library holds folders there). The online checks count such a root as **offline**
    too, and so a root whose device changed (Unix device ids; not checked on Windows) where none
    of the library's files is found (another volume mounted in its place; a drive plugged in
    again with a new device number but its files is accepted). Nothing is written under an
    offline root, even where a folder is there: a sidecar never lands in an empty mount point.
    The online checks run every 10 s, so each write also checks its root right before it lands
    (M6.43): the sidecar writer (at most once per root per write pass) and the metadata writer
    into photo files (before each file, M6.30) run the root's probe with the online checks' own
    rule and hold a write whose root fails it as offline; it waits, is reported unsaved by a
    flush, and lands when the volume returns.
  - **Stale temporaries:** each folder a scan or rescan hands out has its stale atomic-replace
    temporaries removed (`security.md` §5.8).
  - **Stale entries:** a photo deleted outside Dalica leaves the library at once. Its sidecar entry
    stays: the folder's next sidecar write marks it in `dalica.missing` (§3) with the time, and the
    first write 30 days or more later prunes the entry (pruning happens only on a write). An entry
    is stale only while it holds Dalica's data, has a plain file name, is not a photo of the
    library and has no file in the folder; a file that comes back (or one the scans skip) keeps its
    entry, and entries only other programs wrote are never touched. The sidecar, not the index,
    remembers the time, so it survives a rebuild of the index and travels with the folder.
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

**The files themselves** move through `dalica-fs` (built in M9.2: `transfer`, `trash`): a move or
rename is one rename that never replaces a file (a taken name is reported, nothing changes, and
the operation renames to `-001` or skips); across volumes the file is copied to a temporary name,
`fsync`ed, compared byte for byte, given its name by the same rename that never replaces, and only
then is the source sent to the OS trash (never deleted; a volume without a trash keeps the source
and the move is reported as a copy). Folders move the same way, a tree copied file by file. Delete
is always the OS trash.

**Entries travel with their files** (built in M9.9: `dalica-store` `carry`, `SidecarWriter::carry`):

- A moved photo's whole entry under `photos`, keys Dalica does not know included, leaves the source
  folder's sidecar and enters the destination's under the new name; a renamed photo's stays in its
  folder under the new name; a copied photo's is duplicated. A folder merge carries the entry of
  every photo it moves, in one pass; what the source sidecar says about its own folder stays in it.
- Sidecars are merged entry by entry and never replaced. When the destination already has an entry
  under the new name (a stale one, §7), the two are merged key by key with the carried keys
  winning. The top-level album names the carried memberships need are copied where the
  destination lacks them; the stale marks of the names involved are dropped.
- The carry is sequenced with the folders' debounced writes: the pending writes of both folders go
  first, then the carry, then the destination is written again from the model, so the model's
  values (a star made just before the move, a caption cleared) stand over the carried keys while
  the keys the model does not know arrive unchanged.
- The destination is written before the source: an interruption between the two leaves the entry
  in both files, never in neither, and carrying again finishes (an entry no longer in the source
  is not carried twice).
- A photo's originals backup (`.dalica-originals/<name>`, §6) moves, or is copied, with it through
  `dalica-fs` under the new name, then its originals-record entry travels like a sidecar entry. A
  backup of that name already in the destination is never touched: the photo's backup and record
  stay where they were, and the operation reports it.
- Refused, with nothing written: a folder that is not there (never created), a read-only folder (an
  earlier damaged sidecar), a damaged sidecar or record met on the way (kept aside as
  `.dalica.json.corrupt-<time>`; the folder becomes read-only), an entry or `photos` of the wrong
  type where the entry must go, a name that is not one plain path component.

**The file-operation journal** `file-ops.json` in the library directory (built in M9.9:
`dalica-store` `intents`) records what an operation set out to do, so a crash at any step can be
finished or undone at the next open:

```json
{"version": 1, "ops": [{"id": 7, "kind": "move", "direction": "do", "items": [
  {"from": "/home/ana/Pictures/Lisbon/IMG_0001.jpg", "to": "/home/ana/Pictures/Porto/IMG_0001.jpg", "state": "pending"}]}]}
```

- `kind` is `move | copy | rename | trash | moveFolder | renameFolder | trashFolder`, `direction`
  `do | undo | redo` (an undo journals the reverse items). Each item names its absolute UTF-8
  `from` and, except for a trash, its `to`; `state` is `pending`, `placed` (a cross-volume copy in
  place and verified, its source not yet in the trash) or `done` (its files are done).
- Written through the I/O worker, atomically over the version it last wrote: before an operation
  starts (every item `pending`; nothing moves until it is on disk), after each item, and when the
  operation's results are in the library, which removes it. The name a clash gave is written before
  the file moves there. The progress of an operation of more than 64 items is written in batches
  (at most 64 rewrites per pass over its items), which is safe because recovery decides from the
  disk and the state only refines it.
- A journal that cannot be read whole (not JSON, another version, over 256 MiB, an operation with
  a wrong type, a relative path or `..`, a duplicate id, an item without the `to` its kind needs,
  more than 4 096 operations or 1 000 000 items) is kept aside as `file-ops.json.corrupt-<time>`
  and reported; the operations that read are written to a new journal.

**The order of one item's steps:** the journal entry is on disk → the file moves (one rename, or
copy and verify, `placed`, then the source to the trash) → its backup and record move → the item
is `done` → its library ops are applied and the folders' new states go to the writer → its entry
is carried → when every item has its results in the library, the operation leaves the journal.

**Repair at open** (`intents::plan`, pure; the engine runs it as one file job at open, built in
M9.21): for each item, from what is on disk at `from` and `to` (and whether `to` is a verified copy
of `from`):

| Kind | On disk | Decision |
|---|---|---|
| move, rename (also of folders) | source there, destination not | back: it never moved; nothing to change |
| | destination there, source not | forward: carry the entry and backup |
| | both, item `done` | forward: carry (its volume had no trash; the source stays) |
| | both, destination verifies | forward: send the source to the trash, then carry |
| | both, destination does not verify | back: the destination is not the photo's copy |
| copy | destination there and verifies, or the item `done` | forward: duplicate the entry and backup |
| | otherwise with the source there | back |
| trash | the source gone / still there | forward (in the trash; recovery leaves its entry to the stale sweep, §7, so a photo restored from the trash within 30 days gets its data back) / back |
| any | neither side there | lost: reported, nothing to do |
| any | a side cannot be looked at (offline volume) | wait: the operation stays in the journal |

Rolling back never removes anything: a partial copy has a temporary name, which the stale-temporary
sweep removes. Carrying is harmless when it was done before, so a forward item is always carried
again. Folder moves carry nothing (the sidecar is inside the folder).

How the repair reads the disk and follows it (M9.21):

- **Looking.** A side in an offline folder, or one that cannot be looked at, is unknown (the
  operation waits). A name that a volume ignoring case resolves to the other end's file counts only
  where its folder lists that exact spelling, so a file is never taken for its own copy and sent to
  the trash; two hard links of one file are a copy of each other. "Verifies" is byte for byte, a
  folder file by file. An undone copy (the copy to the trash) is judged as a trash; an item naming
  one folder at both ends (Move to New Folder's folder) has nothing to repair.
- **The order of a forward item:** a source still to go goes to the OS trash after the copy is
  checked once more (a volume without a trash keeps it, and the move became a copy); then the
  photo's backup with its record, then its entry, carried as a copy when the source stays. A carry
  that fails with an I/O error is tried again at once (the session's writer may have replaced the
  sidecar meanwhile), then left for the next open.
- **The library follows** as a change from the files: a moved or renamed photo the library still
  shows at its old place moves there with its id and data; a moved folder likewise, its
  watched-folder entries rebased; a trashed photo or folder leaves it; any other end the library
  shows otherwise than the disk (a copy, a photo put back) has its folder rescanned, which reads
  the carried entry.
- **Leaving the journal:** once its items are repaired and the library's ops are journalled. An
  operation with an item that waits, or whose carry failed with an I/O error, stays for the next
  open; one refused for good (a damaged sidecar kept aside, a read-only or vanished folder, a name
  taken) is reported and not tried again. Each item's repair is logged and listed
  (`library.inspect` → `fileOpRepairs`), and the job's end is a notice.
- **While Dalica runs**, two things leave an operation to this repair instead of losing anything:
  a carry of an entry or a backup that failed or left something behind keeps the operation in the
  journal (the next open carries again); and an item whose step failed after its file or folder
  had moved (the folders' `fsync`, the check after the rename) is reported failed, but the library
  follows the file where it went and its entry is carried, as the repair would. A batch rename in
  one folder that fails that way puts that file back with the others, so it stays all or
  nothing.

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
2. Moving a folder outside Dalica keeps all its data: while Dalica watches (the watcher's rescan)
   and between sessions (the next session's verification pass), the folder and its photos keep
   their ids, stars, captions, tags, recipes, album memberships in their order, the albums' covers
   and their first-indexed times.
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
13. A file operation (a move on one volume or across volumes, a copy, a batch rename, a delete, a
    folder move) stopped at any step — an error or a crash at every fault point of its file
    transfers, its trash, its journal and its sidecar writes — loses nothing: after the repair at
    the next open every photo is at a path the library lists (once for a move, twice for a
    finished copy), with its data and its sidecar entry, and the journal is empty.
