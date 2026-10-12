# What's new in Dalica

<!--
Adapted from LightCraft `docs/whats-new.md` (revision 88ea1a8); MIT OR Apache-2.0.

This file is the user-facing changelog. Help ▸ Release Notes compiles it into the app
(`include_str!`) and HTML comments like this one are not shown. Conventions:

- Newest first. One `## <Month Year>` section per month (or `## <version> — <Month Year>` once
  releases exist), then `### <Area>` subsections named after what users see: Library, Folders,
  Albums, People, Places, Editing, Effects, Viewing, Creating, Import, Export, Languages,
  Reliability, Agents.
- One bullet per change a user would notice, written for users: what they can do now and where to
  find it (menu path with ▸, shortcut), not how it was built. Mention command ids only where
  agents need them (in backticks), and issue numbers as (issue #123).
- Be honest: say what is approximate, partial or not yet supported. Data-safety changes (what is
  written where, format version bumps, what older versions can no longer read) are always listed.
- No task ids, commit hashes, internal crate names or test counts. Never name the reference
  organizer; describe behaviour in our own words.
- Update it in the same commit as the user-visible change. Re-wrapping or editing a line breaks
  its translation until the catalogs catch up (docs/localization.md → Release notes).
-->

## 0.1.0 — October 2026

The first preview of Dalica, an organizer for the photos in your own folders. You can browse,
star, caption, tag, rotate, hide and re-date your photos. Dalica keeps all of it in a small file in
each folder, and writes captions, tags and dates into the photos' own metadata. It never changes
your pictures' pixels. Editing, exporting and slideshows come in later releases: see "Not in this
release yet" at the end.

### Installing

- Linux, on x86_64 and ARM64: an AppImage, a .deb, an .rpm, a Flatpak bundle and a .tar.gz.
  AppImageUpdate fetches only the changed parts of a newer AppImage. On its first start, the
  AppImage adds itself to your applications menu (set `DALICA_NO_DESKTOP_INTEGRATION=1` to stop
  that).
- The Flatpak sees only your Pictures folder and has no network access. To add a folder
  elsewhere, grant it first: `flatpak override --user --filesystem=<folder> org.dalica.Dalica`.
- If your system lacks a library Dalica needs (Wayland or X11, xkbcommon, the Vulkan loader),
  Dalica says which package to install and exits instead of crashing.
- Windows x64: an MSI installer and a portable zip. The installer adds a Start Menu shortcut and
  lists Dalica under "Open with" for the picture formats it reads, without ever making it your
  default viewer.
- The portable zip keeps its library, thumbnails and log in a `data` folder beside `dalica.exe`
  (because of the file named `portable` next to it). The `.dalica.json` files Dalica keeps in
  your photo folders stay there.
- macOS 11 or later, on Apple silicon and Intel: a disk image with Dalica.app, and the
  command-line tool `dalica-cli` as a zip.
- This release is not signed with a developer certificate, so Windows and macOS warn you before
  its first start. On macOS, allow it in System Settings ▸ Privacy & Security.
- Every download is listed with its SHA-256 in `SHA256SUMS.txt`. Check yours with
  `sha256sum -c SHA256SUMS.txt --ignore-missing` (Linux) or
  `shasum -a 256 -c SHA256SUMS.txt --ignore-missing` (macOS).
- Help ▸ About Dalica (Dalica ▸ About Dalica on macOS) shows the version, the commit it was built
  from and the build date. Its Contributors and Models tabs credit the people and the AI models
  who built Dalica. Help ▸ Release Notes shows this page.

### Folders

- On its first start, Dalica offers to show your Pictures, Documents and Desktop folders, or lets
  you choose your own.
- Tools ▸ Folder Manager… (also File ▸ Add Folder…) sets each folder to Scan Always (watched for
  changes), Scan Once (read once, then left alone; for slow or removable drives) or Remove from
  Dalica. A setting applies to the folders inside it unless they have their own.
- Folder ▸ Remove from Dalica… takes a folder out of Dalica. Nothing on disk changes, and Edit ▸
  Undo brings it back.
- Scanning runs in the background. Folders appear as they are found, and the activity indicator in
  the toolbar shows each scan's progress, with Cancel.
- Scan Always folders are watched. New, changed, renamed and deleted photos show up by
  themselves, and a full check every few hours catches anything the system did not report.
- Folders on a drive that is unplugged stay in the list, dimmed and marked offline, with their
  thumbnails. What you change meanwhile is written when the drive comes back, and never into the
  empty folder an unplugged drive leaves behind.
- The folder list shows every folder by year, or as a tree (View ▸ Folder View). The toolbar's
  view options sort it by date, name, size or recent changes. Clicking a folder jumps to it.
- Folder descriptions: click "Add a description" under a folder's title, type, and press Enter.
- Folder Properties (double-click a folder in the list or its title, or Folder ▸ Edit
  Description…) sets the folder's date, place and description. Type the date as YYYY-MM-DD, or
  tick "Use the date of the oldest photo". Renaming a folder and its music for slideshows are
  greyed out until later releases.
- Folder ▸ Hide moves a folder to Hidden Folders, shown only with View ▸ Hidden Pictures. The
  folder stays hidden when you move it, because the setting is kept in the folder itself.

### Library

- All your folders in one long, scrolling light table, a header for each folder. The header of
  the folder you are in stays at the top, and thumbnails never go blank while you scroll.
- Thumbnail size: the slider at the bottom, Ctrl+wheel, or View ▸ Small Thumbnails (Ctrl+1) and
  Normal Thumbnails (Ctrl+2).
- Select with a click, Ctrl, Shift or by dragging a box. The Photo Tray at the bottom shows the
  selection, and Hold keeps photos there while you select others.
- Star photos with Picture ▸ Star / Unstar (Ctrl+8) or the star button under the tray.
- Rotate with Picture ▸ Rotate Clockwise (Ctrl+R) and Rotate Counterclockwise (Ctrl+Shift+R) or
  the buttons under the tray; flip with Picture ▸ Flip Horizontal (Ctrl+Shift+H) and Flip Vertical
  (Ctrl+Shift+V). The thumbnail turns at once. A rotation is kept as an edit beside the photo:
  the file itself is not turned, so other applications still show it as it was.
- Captions: Picture ▸ Edit Caption opens a field under the photo. Enter (or a click elsewhere)
  keeps the text, Esc leaves the caption as it was.
- View ▸ Thumbnail Caption shows the file name, caption, tags, size in pixels or date under every
  thumbnail.
- Picture ▸ Hide hides photos; View ▸ Hidden Pictures shows them again, faded.
- Tools ▸ Adjust Date and Time… gives the selected photos a new date and time, or shifts them all
  by the same amount (for a camera whose clock was wrong). Photos re-sort by their new dates.
- The Properties pane (View ▸ Properties or Picture ▸ Properties, Alt+Enter; Cmd+I on macOS)
  shows a photo's file, size, dates, camera, lens, exposure, GPS position, caption and tags. Show
  All Metadata lists every field the file holds. With several photos selected, it sums them up.
- Every change is one step of Edit ▸ Undo (Ctrl+Z) and Redo (Ctrl+Y; Cmd+Shift+Z on macOS),
  however many photos it touched.
- Formats shown: JPEG, PNG, TIFF, WebP, GIF (its first frame), BMP, JPEG XL, PSD and PSB (the
  merged picture), and the camera's preview inside RAW files (CR2, CR3, CRW, NEF, ARW, DNG, PEF,
  ORF, RW2, SRW, RAF, MRW, X3F). Colour profiles are applied (View ▸ Use Color Management).
- A file Dalica cannot show is still listed, with a "!" whose tooltip says why ("Damaged file",
  "Too large to display").

### Tags

- The Tags pane (View ▸ Tags, Ctrl+T): type into "Add a tag:" and press Enter to tag every
  selected photo. Separate several tags with commas; suggestions come from the tags you already
  use.
- The pane lists the tags of the selected photos with how many hold each ("beach (3 of 8)"). The
  × on a tag removes it from the selection.
- Right-click a tag: Add to All Selected, Remove from Selected, Rename Tag…. Renaming onto a tag
  you already use merges the two.
- Quick tags: ten buttons in the Tags pane toggle a tag on the selection, as do Ctrl+Alt+1 …
  Ctrl+Alt+0. The top two follow your most recently added tags. The gear button opens Configure
  Quick Tags, which can fill the empty buttons with your most used tags.
- The status line lists the tags of the selected photos, most used first.

### Your photos and their files

- Dalica never changes a photo's pixels in this release. There is no Save yet, so there is no
  `.dalica-originals` folder either; it arrives with Save.
- Everything you do is kept in a `.dalica.json` file in each folder: stars, captions, tags,
  hidden photos, rotations and flips, dates you set, and the folder's description, date, place
  and hidden setting. It is plain text, it moves with the folder, and other programs' entries in
  it are kept. Dalica's own index is only a cache of these files.
- Captions, tags and dates are also written into the photo files, as standard metadata that other
  photo applications read: in JPEG (XMP, IPTC and EXIF), PNG (XMP and EXIF), WebP (EXIF and XMP)
  and TIFF (XMP and IPTC). Only those fields change. The pixels and everything else in the file,
  other applications' metadata included, stay byte for byte as they were. Each new version is read
  back and checked before it replaces the file.
- When: about a second after your last change, in the background ("Writing metadata" in the
  activity indicator, with Cancel). Undo writes the old value back. Quitting Dalica writes what is
  still waiting. A cancelled write stays in `.dalica.json`, and the next change of that field
  writes it.
- How: the file is replaced in one step, only if no other program changed it since Dalica read
  it (otherwise Dalica reads it again first). The file's modification date is put back as it was.
- Not written into files: rotations, flips, hidden photos and folder information (all in
  `.dalica.json` only); stars, unless you turn that on (below); a photo's place, which Dalica
  cannot edit yet. GIF, BMP, HEIC, JPEG XL, PSD and AVIF files are never written.
- RAW files are never written. With the option below turned on, their captions, tags and dates go
  into an `.xmp` file beside them, which keeps what raw developers wrote there. A damaged `.xmp`
  file is kept aside as `<name>.xmp.corrupt-<time>`.
- A read-only file is left alone. A message names the file and what could not be written; the
  value stays in `.dalica.json`.
- Reading: the first time Dalica sees a photo, it takes the caption, tags and date from the file,
  and a rating of 5 as a star (from a RAW's `.xmp` file too). What `.dalica.json` already says
  wins.
- A value changed later in another application comes in as one undo step, "Import from Files".
  When both Dalica and the other application changed it, Dalica keeps (and writes) its own value,
  and the Properties pane notes "Changed in another application; Dalica's value kept".
- The write options and their defaults:
  - Write captions, tags and places into photo files: on.
  - Write date changes into photo files: on.
  - Write stars as ratings: off. On: a star is written as a rating of 5, and unstarring sets 0,
    but never clears a rating another application set.
  - Update the file's modification date when its metadata changes: off.
  - Write XMP sidecars for RAW files: off.
- There is no Options dialog yet. To change an option, quit Dalica and edit the `"files"` object
  in `settings.json` in Dalica's library folder (Linux `~/.local/share/dalica/library`, macOS
  `~/Library/Application Support/Dalica/Library`, Windows `%LOCALAPPDATA%\Dalica\Library`, or
  `data\library` beside the portable `dalica.exe`): `writeCaptionsTagsPlaces`, `writeDates`,
  `writeStarsAsRatings`, `updateModificationTime` and `writeRawXmpSidecars`, each `true` or
  `false`.

### Agents

- Every action is a command with a stable id. Drive the running app through its control channel
  (`dalica --control <port>`), or headless with `dalica-cli run`, `inspect` and `snapshot`.
- New commands: `photo.rotate.cw`, `photo.rotate.ccw`, `edit.flipH`, `edit.flipV`,
  `photo.hide`, `photo.unhide`, `folder.hide`, `folder.unhide`, `photo.caption.set`,
  `folder.info.set`, `photo.dates.set`, `photo.tags.add`, `photo.tags.remove`,
  `photo.tags.toggle`, `tags.list`, `tags.rename`, `tags.quick.get`, `tags.quick.set`,
  `tags.quick.apply`, `photo.metadata` (the Properties rows), and the dialogs' `app.about` and
  `help.releaseNotes`.

### Not in this release yet

- No editing: the edit view, crop, Auto Fix, tuning, effects and Save come next. Export, collages
  and slideshows are not there either, and their buttons do nothing yet.
- No albums, people, places on a map or search. Moving, renaming and deleting files from inside
  Dalica come later; so does the Options dialog.
- Videos are not listed.
- HEIC and HEIF photos (from iPhones) are listed but not shown: these builds leave the HEIC decoder
  out, and the tooltip on their "!" says "HEIC support isn't included in this build". Dalica built
  from source with `--features heif` shows them. AVIF files are listed and never shown.
- Open With: on Windows it starts Dalica on its library window, without the photo you chose. On
  macOS, Dalica starts from Open With and ignores the file; on Linux it is not offered.
- On Windows, `dalica.exe --version` typed in a console prints nothing, because Dalica is a
  windowed program. `dalica-cli --version` or Help ▸ About Dalica tell you the version.
