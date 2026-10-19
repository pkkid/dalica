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

## 0.9.0 — October 2026

Dalica can now make collages, and it reads the photo data your previous organizer left in your
folders: stars, captions, edits, albums, people and places show up on the first scan, with nothing
to import and the old files never touched.

### Creating

- Collage: hold photos in the Photo Tray and press Collage on the bottom bar, use a folder's or a
  person's Collage button, or Create ▸ Picture Collage…. The Collage room opens with your photos
  laid out on a page.
- Six styles: Picture Pile, Mosaic, Frame Mosaic, Grid, Contact Sheet and Multiple Exposure. Each
  offers the options that suit it: picture borders (none, white or instant), grid spacing, a
  background colour (with an eyedropper) or one of your photos as the background, drop shadows and
  captions. The page can take any crop ratio or your display's shape, landscape or portrait.
- On the canvas, drag pile pictures to move them and use the round handle to turn and size them;
  drag a picture onto another in the grids and mosaics to swap them. Right-click for Bring to Top,
  Send to Bottom, Set as Background, Set as Frame Center, View and Edit. Scramble Collage tries a
  new arrangement and Shuffle Pictures deals the pictures into the same places again. Ctrl+Z takes
  back changes in the room.
- The Clips tab lists the collage's pictures: + and − add and remove them, and Get More takes you
  back to the library to pick more (Back to Collage returns).
- Create Collage writes a JPEG (3000 px on its long side, or your display's size) into
  Pictures/Dalica/Collages, beside a small recipe file, and opens it. It never replaces a file.
  The Collages folder appears under Projects.
- Edit Collage, in the edit view's top bar, reopens a collage you made with its pictures and
  settings. Closing the room with unsaved changes asks: Save Draft, Discard or Cancel.

### Folders

- Folders your previous organizer managed show their stars, hidden photos, captions, tags,
  rotations, crops, supported edits, albums, people and places right after the first scan. A note
  tells you once how many folders it found.
- Dalica only reads those files: the first change you make in such a folder writes Dalica's own
  file beside them, and the old files stay exactly as they were. A folder that has both reads
  Dalica's.
- Photos the previous organizer saved with edits can be taken back from the originals it kept:
  the edit view's Undo (it reads Undo Save) or File ▸ Revert….
- Edits Dalica can't reproduce are kept and shown in the edit panel as "1 edit from the previous
  program (not supported)".
- Folder Properties says which photo data a folder uses. Options ▸ General ▸ Read photo data left
  by the previous organizer turns all this off (and back on) without writing anything.

### Places

- A map marker that groups nearby places now opens a bubble for all of its photos: "2 photos
  here", and Show These Photos and Remove Location take both (it used to describe only one).

### Reliability

- A folder's first Dalica file is now created in a way that can never replace a file another
  program wrote there at the same moment.

### Agents

- Collages over MCP and the control channel (`create.collage`, `collage.*`), and `legacy.inspect`
  for which photo data each folder uses. A collage is written only inside the folders automation
  may write to, whether it is made by a command or by clicking the room's buttons.

### Not in this release yet

- Collages of videos, printing a contact sheet and setting a collage as the desktop background are
  not here.
- The canvas doesn't show pile captions or contact-sheet file names; the created JPEG has them.
- Some of the previous organizer's edits aren't reproduced (Straighten, the effects that take a
  centre point, borders with rounded corners or captions); they are kept, listed in the edit panel
  and left out of the picture. Its central database, contacts file and thumbnail caches outside
  your folders are not read.
- Slideshow music, videos, importing from cameras over a cable, translations and making Dalica
  your default viewer still come later.

## 0.8.0 — October 2026

Dalica now knows who and where: it can find the faces in your photos, group them so you can name
people, and put photos on a map. Face finding happens entirely on your computer with a model you
choose to download, and Dalica connects to nothing unless you allow it.

### People

- Tools ▸ Face Model… lists the face models Dalica offers (YuNet to find faces, SFace or AuraFace
  to tell them apart) with their licences and what is known about how they were made. Nothing is
  downloaded until you tick that you accept the licence; a download can be stopped and picks up
  where it left off, and every file is checked before it is used. You can also add a model file
  you already have.
- With a model installed and Find faces in my photos on (Options ▸ People), Dalica finds faces in
  the background at the lowest priority. It pauses while you type, scroll or drag, and the window
  never waits for it.
- The People group in the folder tree lists Unnamed and everyone you have named, with counts. Click
  Unnamed to see the faces grouped by likeness, largest group first: type a name under a group to
  name every face in it at once (Ctrl+Z takes it back), press × to ignore faces, or drag faces onto
  a person in the tree.
- A person's section shows their photos; Zoom to Faces shows just their faces. The person menu has
  Rename, Merge Into, Delete Person, Set as Person Thumbnail and Play.
- Suggestions: when a face looks clearly like someone you have named, it shows "Ana?" with ✓ and ×.
  Confirm All accepts a person's suggestions in one step, Find More looks a little further, and a
  face you reject is never suggested for that person again. Options ▸ People sets how sure Dalica
  must be.
- The People pane (View ▸ People) lists the people in the selected photos; hover a face to see its
  box on the photo. In the edit view you can draw a box around a face Dalica missed and name it.
- Search for `person:Ana`, or use the toolbar's Faces filter for photos with faces.
- Faces in your photos guide the crop suggestions and the slideshow's Pan and Zoom.
- Names you give are kept in Dalica's files next to your photos. With Store people names in photos
  on (Options ▸ People), they are also written into the photo files in the standard way other apps
  read; names other apps wrote there come in on the first scan. Tools ▸ Experimental ▸ Write
  People Names to Photos… writes them for the selected photos.
- Find Faces Again (on the photo menu) and the Folder Manager's Remove Faces start over for those photos or folders; names you gave stay.

### Places

- View ▸ Places opens a map of the selected photos (or the folder's). Drag photos onto the map to
  put them there, or use the pin; Dalica asks before moving photos that already have a place.
  Ctrl+Z takes it back.
- Tools ▸ Geotag ▸ Copy Location, Paste Location and Clear Location…. A place is written into the
  photo file's GPS and its XMP copy when Write captions, tags and places is on; a location you
  cleared stays cleared even if another app writes GPS into the file again.
- Photos from your phone show on the map from their own GPS after the first scan.
- The map works offline (coastlines and a grid). Map tiles from OpenStreetMap and searching for a
  place by name are off until you allow them: the Places pane asks once, and Options ▸ Network has
  the switch and the server addresses (for your own server or a proxy). Typed coordinates never
  need the network.

### Effects

- Every effect is now quick on large photos: the preview while you drag stays under 16 ms and the
  final under a tenth of a second on two cores, and saving is two to thirty times faster
  (Focal Zoom's final went from 2.7 s to under 0.1 s). On photos above 2 MP Focal Zoom's blur is
  very slightly softer across its rays.
- The effect tiles of a photo that already has a frame show the whole picture, and a frame reads
  clearly at tile size.

### Agents

- People and places over MCP: `set_metadata {geo}`, `photo_info` with the place and faces, and
  person search. Without `--automation-allow-people`, agents see "Person" instead of names and no
  face boxes; downloading models or allowing map tiles needs `--automation-allow-external`.

### Not in this release yet

- Face finding needs a model download (about 40 MB for YuNet and SFace, 261 MB for AuraFace) and is
  slow on older computers; SFace's commercial-use terms are unclear upstream, which the Face Model
  dialog says.
- Grouping is not perfect: in our test set of 10 people, SFace splits two people in two at the
  default confidence; AuraFace keeps all ten together.
- Places from addresses (reverse geocoding), GPS tracks, faces in videos, and a People Manager are
  not here yet.
- Slideshow music, videos in slideshows, importing from cameras over a cable, and making Dalica your
  default viewer still come later.

## 0.7.0 — October 2026

Dalica now has effects: 27 of them on the edit view's three effect tabs, each with a live preview
on your photo. Effects are steps of a photo's history like any other edit, so they undo, survive a
restart and never touch the original until you save.

### Effects

- Open a photo (Ctrl+3) and click the edit panel's third, fourth or fifth tab. Each tab shows a grid
  of tiles that preview every effect on the photo you are editing; the grid fills in the background
  and never makes you wait.
- Sepia, Black and White, Warmify and Invert Colors apply with one click (their tiles carry a
  "1-click" badge); Ctrl+Shift+B is Black and White. Every effect is one step of the photo's
  history, named after it, so Ctrl+Z takes it back.
- The other effects open a panel under the tiles: sliders with their label above, checkboxes,
  colour swatches with a picker and an eyedropper that takes a colour from the photo, Fade, Reset,
  Apply and Cancel. The photo previews the change live; Cancel records nothing.
- Tab 3: Sharpen, Sepia, Black and White, Warmify, Film Grain, Tint, Saturation, Soft Focus, Glow,
  Filtered B&W, Focal B&W and Graduated Tint.
- Tab 4: Infrared Film, Plastic Lens, HDR Look, Widescreen Film, Dream Glow, Invert Colors and
  Duo-Tone.
- Tab 5: Boost, Soften, Vignette, Pixelate, Focal Zoom, Border, Drop Shadow and Museum Matte.
- Soft Focus, Focal B&W, Graduated Tint and Focal Zoom put a crosshair on the photo: drag its centre
  (or nudge it with the arrow keys), and Graduated Tint's handle sets the angle. It lands where you
  put it on a rotated or cropped photo too.
- Soften softens only where you paint: Paint and Erase in its panel, the brush circle on the photo,
  + and − to resize the brush.
- Border, Widescreen Film, Drop Shadow and Museum Matte make the picture larger (or wider). The
  thumbnail, the status bar, Save and Export all use the new size; the photo's own pixels inside the
  frame are unchanged.
- Picture ▸ Batch Edit's Sepia, Sharpen, Warmify, Film Grain and Black and White now work: one step
  over all the photos in the tray, undone with one Ctrl+Z.

### Viewing

- The slideshow prepares the next photo off screen, so a large photo no longer makes the change
  stutter.
- In the edit view, the caption bar's captions button now shows when captions are on.

### Agents

- The effects are edit operations like the others: `list_edit_ops` lists all 27, `apply_edit`
  stacks them and `render_photo` returns the framed picture at its grown size.

### Not in this release yet

- Some effects are slow on large photos: a final preview of Focal Zoom, HDR Look, Soften, Boost,
  Glow and a few others can take from a tenth of a second to a few seconds on a 24 MP photo. A
  quicker draft shows first. They get faster in the next release.
- Slideshow music, and videos in slideshows (they are skipped); both come with sound support.
- Importing from cameras and phones connected by cable; use a card reader or the phone's storage
  as a drive.
- `dalica --view` while Dalica is already open opens a read-only viewer; handing the picture to
  the open Dalica, making Dalica your default viewer and Open With come later.

## 0.6.0 — October 2026

Dalica now imports photos from cards and drives, compares two photos or two versions side by side,
plays slideshows, and opens any picture on its own as a photo viewer. Edits can be copied, pasted
and applied to many photos at once.

### Import

- File ▸ Import From… (Ctrl+M), or the toolbar's Import, opens the Import room in its own tab
  beside the Library. Pick a memory card or removable drive from the list, or a folder with
  Folder…. Its photos arrive in groups by the time they were taken; click a group's clock to select
  the group.
- Press X (or the preview's exclude button) to leave a photo out. Photos already in your library
  are left out for you while Exclude duplicates is on. Star and rotate a photo in the preview before
  importing, and step through them with the arrow keys.
- Choose where photos go (Import to), how the new folders are named (Date taken or Today's date),
  then Import All or Import Selected. Every copy keeps its date and is checked byte for byte
  against the original before it counts. An existing file is never replaced: a name that is
  already taken gets `-001`.
- After copying, Dalica can delete the copied files from a card, but only the files whose copies
  were checked, and only after asking. Duplicates and photos you left out stay on the card.
  Automation and AI agents can never delete from a card.
- An import is one step: Edit ▸ Undo moves the copies to the trash (a copy you changed since is
  kept). Once the card was emptied, the import can no longer be undone.
- Options ▸ General: the folder imported photos go to by default ("Save imported pictures in") and
  whether duplicates are detected.

### Editing

- Compare (the A│B / A│A toggle above the photo): A│B shows two photos side by side, and the
  filmstrip changes the one with the "Selected" pill. A│A shows one photo twice, so you can try
  edits on the right side, including Crop, Straighten, Retouch, Tuning and Fill Light. When you
  leave it, "Keep which version?" keeps the left, the right, or asks again. Zoom and pan move both
  sides together.
- Hold `\` (or the panel's Original button) to see the photo without its edits.
- Edit ▸ Copy All Edits and Paste All Edits copy one photo's edits onto others. A photo holding
  edits from a newer Dalica is skipped and named, so nothing of it is lost.
- Picture ▸ Batch Edit ▸ applies Auto Fix, Auto Contrast, Auto Color, rotations and more to every
  photo in the Photo Tray as one step: one Ctrl+Z puts them all back. Picture ▸ Undo All Edits…
  removes every edit from the selected photos, after asking, and can itself be undone.

### Viewing

- Slideshow (Ctrl+4, the toolbar's Slideshow, a section's ▶, or Play in the edit view) plays a
  folder, an album or search results full screen from the selected photo, with seven transitions,
  captions, Loop and a display time of 1 to 60 seconds. Move the mouse for the control bar: Previous,
  Play/Pause, Next, the transition, captions, the display time and zoom. Space pauses, the arrow
  keys and the wheel step, Ctrl+8 stars, Ctrl+R rotates and Esc goes back to where you were.
  Options ▸ Slideshow keeps the defaults. The next photo is always ready, so a change never waits
  for the disk.
- Hold Ctrl+Alt over a thumbnail to see it full screen at once; turn the wheel to step through the
  folder, let go to come back with that photo selected.
- `dalica --view photo.jpg` opens any picture on its own, full screen over a dimmed backdrop, with
  its folder's other pictures in a filmstrip. Zoom, 1:1, rotate, star (the folder then joins your
  library as a Scan Once folder), delete, play a slideshow, or press E to open it in Dalica. Space
  switches the backdrop; Esc closes. Tools ▸ Configure Photo Viewer… and Options ▸ Photo Viewer
  set how the wheel and the controls behave.

### Agents

- MCP: Batch Edit is one tool call and one undo step, and imports run through the new `import.*`
  commands. Deleting copied files from a card is refused over automation and MCP, and an import's
  destination must be inside the allowed write roots.

### Not in this release yet

- Slideshow music, and videos in slideshows (they are skipped); both come with sound support.
- Importing from cameras and phones connected by cable; use a card reader or the phone's storage
  as a drive.
- Batch Edit's Sepia, Black and White, Warmify, Film Grain and Sharpen are greyed until the effects
  arrive.
- `dalica --view` while Dalica is already open opens a read-only viewer; handing the picture to
  the open Dalica, making Dalica your default viewer and Open With come later.
- At 100 % in the viewer, a large picture outside your library is shown from a screen-sized copy,
  and the Import room's preview is an enlarged thumbnail.

## 0.5.0 — October 2026

Dalica now has its preferences and a keyboard shortcut editor, and AI agents can drive it through
the Model Context Protocol. This release also fixes four ways a photo's details could have been
written wrongly or late, and finishes the last gaps in everyday use.

### Preferences

- Tools ▸ Options… (Ctrl+,; Dalica ▸ Settings… on a Mac) opens Options with four tabs. Changes show
  at once where they can; Apply or OK keeps them and Cancel puts the last kept ones back.
- General: the interface scale (Auto, or 100 % to 200 %), the text size, tooltips, animations,
  the system title bar, and when to write captions, tags and dates into your photo files. Options
  that arrive in a later version are shown greyed with the reason.
- File Types: choose which kinds of picture Dalica shows, and the size below which small pictures
  are hidden. Turning a type off hides those photos after a quick rescan; their stars, captions and
  edits are kept and come back when you turn it on again. Nothing is deleted.
- Export: the default size, quality, folder and watermark for File ▸ Export to Folder….
- Keyboard Shortcuts (or Help ▸ Keyboard Shortcuts…, Ctrl+/): search commands by name, menu or
  key, give a command up to two keys in each view, and press Backspace to remove one. If a key is
  already taken, Dalica says by what and asks before taking it. Reset All brings every default back.

### Library

- Recently Updated now shows, in the folder tree and at the top of the library, the photos you
  changed recently (a caption, a star, a rotation, an edit, or the file changed on disk). Newly
  added photos do not count. The list survives Library ▸ Rebuild.
- Photos with a place now carry the place badge.
- A search with no results says "No photos found", with a Show All button.
- While photos are exported, the status line says "Exporting 6 of 20".

### Folders

- If a folder's `.dalica.json` is damaged, its section shows a warning, and Dalica writes nothing in
  that folder (neither the sidecar nor your photo files) until you choose Reset Folder Data. The
  damaged file is kept beside it, and if you put back a good copy yourself, Dalica reads it.
- Folder Properties picks the date with the arrow keys or Home and End, and refuses dates that do
  not exist. The Folder Manager can be made larger and keeps its size.
- Folder ▸ Sort By shows a mark beside the current order.

### Editing

- The edit view's trash button now works: it asks, moves the photo to the trash, and shows the next
  one.
- Space stars the photo in the edit view, Ctrl+Shift+S exports it, F2 renames it, and Ctrl+3 and
  Ctrl+9 work there as they do in the library.
- A caption being typed in the edit view is kept if you close the window.

### Agents

- `dalica-cli mcp` is an MCP server for AI agents. Add it to a client such as Claude Code with
  `claude mcp add dalica -- dalica-cli mcp --demo` to try it on demo photos, or give it your
  library. Agents can scan folders, search, star, caption, tag, edit, render and export, each with
  one undo step, and every command is also a tool.
- With `--connect`, it drives a running Dalica started with `--control` instead, and the window
  shows what the agent does.
- Agents can read only the folders you allow and write only where you allow; anything that deletes
  asks first, and `--read-only` changes nothing on disk.

### Formats

- HEIC photos (the iPhone's default) are organized like any other photo, with their date and
  details, but this version cannot show their picture: decoding HEIC needs a patent licence that
  Dalica's releases do not have. Their tile says how to get JPEGs instead (on an iPhone: Settings ▸
  Camera ▸ Formats ▸ Most Compatible).
- JPEGs marked Adobe RGB without an embedded profile now show their colours correctly.

### Reliability

- Adding a caption or tags to a photo from many phones and cameras (iPhone, Pixel, Sony, Panasonic)
  no longer breaks the small extra pictures stored inside the file.
- If the disk fills up while Dalica records a change, the changes after it are no longer lost.
- On Windows, if Dalica stops in the middle of a Save, the original is never left under two names.
- If Dalica stopped in the middle of moving or copying photos, the repair at the next start could
  lose a moved photo's star and other details kept only in `.dalica.json`. It no longer does.
- A caption or tag is written into the photo file within about a second, even while Dalica sits
  idle; before, it could wait for the next click.
- On Windows, the folders you allow agents to use now work however their path is spelled.

### Not in this release yet

- HEIC pictures are not shown (see Formats), and 12-bit JPEGs show a "not supported" tile.
- Language, Detect duplicates while importing and the import folder are greyed in Options until
  importing arrives.
- Importing from a camera or card, the slideshow, the photo viewer, compare, people, places,
  effects, collages and video come in later versions.

## 0.4.0 — October 2026

Your library can now be organised. Albums gather photos from any folders in an order you choose,
search finds photos as you type, and photos and folders can be moved, copied, renamed and deleted
from inside Dalica. Captions, tags, stars, edits and album places travel with every file, and
nothing is deleted outright: it goes to the system's trash.

### Albums

- File ▸ New Album… (Ctrl+N, or the New Album button) makes an album from the selected photos and
  the tray. Albums are listed under Albums in the folder tree, and each has its own section in the
  library with its name, date, place and description; click the description to edit it.
- An album can hold photos from any number of folders. Drag photos onto an album in the tree, use
  Add to Album ▸ in a photo's right-click menu, or Add to Album ▾ under the tray. Drag photos
  between others inside an album to change their order.
- Album ▸ Album Properties… (or a double-click on the album) changes its name, date, place and
  description. Album ▸ Sort Album By sorts it by name, date or size, or reverses it; Set as Album
  Cover picks the photo that stands for it. Remove from Album (Delete, in an album) takes photos
  out of the album only; the files stay where they are.
- Starred Photos lists every starred photo in one place.
- Every album change can be undone with Edit ▸ Undo. Albums are kept in each folder's `.dalica.json`
  and in an `albums.json` in the library, so an album and its order come back even if the index is
  rebuilt or `albums.json` is lost.

### Search

- Type in the search field (Ctrl+F) to search as you type: words match the start of words in a
  photo's name, caption, tags, folder, place and albums, ignoring case. `-word` leaves photos out,
  `tag:beach` and `camera:canon` search one field, and dates such as `2026`, `2026-06` or `June`
  find photos taken then. Suggestions of tags, folders and albums appear as you type.
- The filter buttons next to the field show only starred photos, videos or photos with a place, and
  the age slider shows only photos up to a day, a week, months or years old. They combine with the
  text.
- While you search, a banner says what is shown ("Showing 20 pictures in 1 folder"); Show All or
  Esc brings the whole library back where you left it. Tools ▸ Experimental ▸ Save Search
  Results… keeps the results as an album. In the Tags pane, a tag's menu has
  Search for This Tag.
- Tools ▸ Experimental ▸ Show Duplicate Files (Ctrl+F6) finds files that are exactly the same, byte
  for byte, wherever they are in the library.

### Moving, copying and renaming

- Drag photos onto a folder in the tree to move them there; hold Ctrl (Cmd on a Mac) to copy
  instead. A label under the pointer says what will happen. Edit ▸ Cut, Copy and Paste do the
  same from the keyboard.
- Dalica asks the first time you move photos (tick the box to stop it asking). If a photo of the
  same name is already there, it asks whether to rename yours (`IMG_0001-001.jpg`) or skip it; it
  never writes over a file.
- File ▸ Move to New Folder… and Split Folder Here… (in a photo's right-click menu) make a new folder
  beside the current one, with its date, place and description.
- File ▸ Rename… (F2) renames one photo or a batch: type a name, and Dalica numbers them in order
  (`Lisbon-001.jpg`), adding the date and size if you like; the dialog shows an example as you
  type. Folders are renamed in Folder Properties, and moved with Folder ▸ Move….
- A moved, copied or renamed photo keeps its caption, tags, star, edits, album places and its saved
  original. Moving to another drive copies the photo, checks the copy byte for byte, and only then
  sends the original to the trash.
- Every move, copy and rename can be undone with Edit ▸ Undo. A notice says what was done ("Moved 3
  photos to Lisbon") or what failed and why.

### Deleting

- File ▸ Delete from Disk… (Delete in a folder, Ctrl+Delete anywhere) sends photos to the system's
  trash after asking; Folder ▸ Delete… does the same for a folder and everything in it. On Linux
  and Windows Edit ▸ Undo brings them back. Dalica never deletes a file outright: on a drive
  without a trash, the photo stays and the notice says why.

### Editing

- In the edit view, Ctrl+Z after a Save now undoes the Save, the same as the Undo Save button.
- After a Save, the edit view, the tray and the thumbnails show the saved picture at once; before,
  they could keep the old one until Dalica restarted.

### Reliability

- If Dalica or the computer stops in the middle of a move, copy, rename or delete, the next start
  finishes or undoes it, so every photo is in one place with its details. The repair is reported.
- A folder you move or rename outside Dalica, while it runs or between sessions, keeps its photos'
  captions, tags, stars, edits and album places.
- Exports now always list under Projects from `Pictures/Dalica`, whatever export folder you choose.
- A caption or tag set just before a Save is written first, so the Save no longer stops with
  "changed on disk".

### Not in this release yet

- Recently Updated is not shown yet: how it should count newly added photos is still being
  decided.
- Show Duplicate Files lists all the duplicates together in one section rather than one section
  per group.
- `color:` and `person:` searches find nothing yet, and the Faces filter is greyed until Dalica
  finds faces. Month names are recognised in English only, and accented letters must be typed as
  they are.
- On a Mac, deleting cannot be undone from Dalica; restore the photo from the Trash in Finder.
- On Linux, a photo whose file name is longer than about 245 bytes cannot be sent to the trash; the
  delete stops and the photo stays.
- Photos in a folder cannot yet be put in an order of your own by dragging, and photos cannot be
  dragged out to other applications.

## 0.3.0 — October 2026

Your edits can now leave Dalica. Save writes them into the photo itself, and the original is kept
safely beside it, so you can always go back. Save a Copy and Save As write a new file and leave
the photo as it is, and Export to Folder turns a selection, a folder or the tray into finished
files, resized and watermarked if you like.

### Saving

- Save (File ▸ Save, Ctrl+S) writes a photo's edits into the file. A folder's header shows how
  many of its photos have unsaved edits ("2 edited") with a Save button that saves them all. The
  first time, Dalica explains what Save does; tick the box to stop it asking.
- Every photo with unsaved changes shows the pencil mark in the library, a photo you only rotated
  or flipped included, so the marks always match the header's count. The mark goes once the photo
  is saved. (issue #454)
- Before the first Save, the original file is moved into a hidden `.dalica-originals` folder next
  to the photo. It stays there however often you save again: each Save starts from that original,
  so quality never wears down. Undo Save (the edit panel's Undo button reads "Undo Save") puts the
  original back and keeps your edits, unsaved; File ▸ Revert… puts the original back and drops the
  edits.
- A JPEG is saved at the quality it already had, with its colour layout kept, and keeps its camera
  details, caption, tags and star. A photo you only rotated or flipped is turned without
  re-compressing it at all, when its size allows; otherwise its orientation flag is changed.
- PNG files are saved in place too. Formats Dalica cannot write (RAW, TIFF, WebP, HEIC, PNG with
  transparency and others) get a JPEG beside them, a new photo of its own; the original file is
  never changed and keeps its edits.
- Save a Copy (File ▸ Save a Copy) writes `name-001.jpg`, `name-002.jpg` and so on beside the
  photo. Save As (File ▸ Save As…) asks where, as JPEG or PNG, and
  never writes over an existing file: pick another name if it is taken. Copies carry the caption,
  tags and star.
- If Dalica or the computer stops in the middle of a Save, the next start finishes or undoes it, so
  the original is never lost.

### Export

- File ▸ Export Pictures to Folder… (Ctrl+Shift+S, or the Export button at the bottom and on a
  folder's header) writes the selected photos, the tray or a folder into a new folder, with your
  edits applied. Choose the size (the original, or a long side from 100 to 4000 pixels; photos are
  never enlarged), the quality (Automatic keeps each JPEG's own, or Normal, Maximum, Minimum or
  your own), numbered names (`001-…`) and a watermark of your own text in the bottom-right corner.
- Exports run in the background with a progress window; Cancel stops after the current photo and
  leaves only complete files. When it ends, Show Folder takes you to the new folder.
- Exported folders are listed under Projects in the folder tree, wherever you put them.
- Exports keep the photos' details (camera, dates, caption, tags, star).

### Editing

- The Tuning sliders show the photo's current Tuning even after you rotate or flip it later, and a
  drag after those edits stays one step of Undo.
- The edit view stays quiet while a preview is rendering, and a cropped thumbnail keeps its new
  shape while its new picture is made.

### Reliability

- On Windows, Dalica now recognises a file by its real file identity, so a file replaced by
  another program with the same size and time is never written over.
- Opening never waits for folder checks or thumbnails: they carry on in the background, shown by
  the activity indicator. While a library opens, the window says what Dalica is doing and counts
  the seconds if it takes a while. (issue #451)
- Every press and drag on the title bar moves the window; before, only every other one did on
  Linux. (issue #453)

### Not in this release yet

- Edits beyond rotating and flipping are saved with 8 bits per channel: a 16-bit PNG stays 16-bit
  but its finer steps are not kept through other edits.
- Removing the GPS position from exports (keeping the place names) is built, but its switch lives
  in Options, which comes in a later release; until then only automation can turn it on
  (`export.run` with `removeLocation`).
- Videos are skipped (and counted) by Export until Dalica can show them.
- In the edit view, Ctrl+Z after a Save does not yet undo the Save; use the Undo Save button.

## 0.2.0 — October 2026

Dalica can now edit your photos. Open a photo in the edit view to crop, straighten, retouch and
fix its light and colour, then step back through every change with Undo. Your edits are kept
beside the photo, and every thumbnail shows them. The original file is never changed: there is no
Save yet, so other applications still see the photo as it was. Save and Export come in the next
release.

### Editing

- Open a photo in the edit view with Enter, a double-click, View ▸ Edit View (Ctrl+3) or Picture ▸
  View and Edit. Esc, Ctrl+1 or Back to Library takes you back, with your place and selection kept.
- ← and → (also Page Up, Page Down, Home and End) move through the photos of the folder. The
  filmstrip at the top shows the photos around this one; click one to open it.
- Zoom with the slider at the bottom right, the mouse wheel, a touchpad pinch, or Ctrl+= and Ctrl+−.
  Zooming keeps the point under the pointer in place and stops at 100 % on its way. Click and hold
  on the photo to see that spot at 100 %; a click fits the photo again; Fit and 1:1 sit beside the
  slider. Drag to move around a zoomed photo; a small map of the photo shows where you are.
- The edit panel on the right (hide it with View ▸ Show Edit Controls, Ctrl+9) has Basic Fixes:
  - Crop: choose a shape (free, the photo's own, print sizes such as 4 × 6, or your own ratios
    through Add Custom Ratio…), draw or drag the rectangle, or click one of three suggested crops.
    Rotate swaps the shape between landscape and portrait; Preview shows the result.
  - Straighten: drag the slider on the photo, up to 10° either way, or press ← and → for 0.1°
    steps. A grid helps you line up the horizon, and the photo is enlarged just enough that no
    corner is ever empty.
  - Retouch: click a spot, then a clean area to copy from; the patch blends in. Change the brush
    size with its slider or + and −. Undo Patch and Redo Patch step through your patches.
  - Auto Fix (Ctrl+Shift+E), Auto Contrast and Auto Color: one click each.
  - Fill Light: brightens the dark parts without washing out the bright ones.
- The Tuning tab has Fill Light, Highlights, Shadows, Color Temperature and a neutral colour
  picker: click something that should be grey or white to remove a colour cast. The two magic
  wands set the light and the colour for you, and Reset puts every slider back.
- Every edit is one step of the photo's own Undo and Redo, at the top of the edit panel (Ctrl+Z
  and Ctrl+Y in the edit view; Cmd+Shift+Z on macOS). The buttons name the step ("Undo Crop").
  A whole slider drag is one step. The history stays after you quit Dalica.
- The Histogram & Camera Info box at the bottom of the panel shows the photo's colours and the
  camera, lens and exposure.
- The other tabs (effects) are empty in this release. Play, the A│B comparison and the trash
  button in the edit view are greyed out until later releases.
- The Tuning sliders show 0 again after you rotate or retouch a photo that you tuned earlier
  (the tuning is still there); this is fixed in the next release.

### Library

- Edited photos show their edits everywhere: in the light table, the Photo Tray and the
  filmstrip, with a small pencil badge at the corner. While a new thumbnail is being made, the old
  one stays on screen.

### Your photos and their files

- Edits are kept in the folder's `.dalica.json`, like rotations, as a list of steps. Dalica keeps
  any step it does not understand (from a newer version, say) where it is, and leaves it alone.
- Writing captions, tags and dates into a photo keeps its modification date even if Dalica or the
  computer stops in the middle of the write.
- Undoing Tools ▸ Adjust Date and Time… now puts the original date back into the file too, also
  after the new date was already written.
- TIFF files: a place or date that cannot be written there is refused before anything is written,
  with a message naming the field. Photos whose IPTC captions and keywords were written by
  ImageMagick (PNG and TIFF) are now read and updated.
- A caption or tag that cannot be written (for example into a read-only file) shows an amber
  message with the whole text, the file name kept whole.

### Agents

- New commands: `view.edit`, `view.library`, `edit.panelToggle`, `edit.zoom.fit`,
  `edit.zoom.actual`, `edit.zoom.in`, `edit.zoom.out`, `edit.zoom.set`, `edit.tool.open`,
  `edit.tool.action`, `edit.tool.apply`, `edit.tool.cancel`, `edit.tool.reset`, `edit.autoFix`,
  `edit.history.undo`, `edit.history.redo`, `photo.history.apply`, `photo.history.undo`,
  `photo.history.redo`, `photo.history.state`, `photo.history.tuning`, `photo.crop.apply`,
  `photo.crop.suggest`, `photo.straighten.apply`, `photo.tuning.auto`, `photo.tuning.pick`,
  `crop.ratios.add`, `crop.ratios.list` and `crop.ratios.remove`.

### Not in this release yet

- No Save: edits live beside the photo and the file itself is not changed. Save, Save a Copy,
  Save As and Export come in 0.3. No collages or slideshows yet either.
- No albums, people, places on a map or search. Moving, renaming and deleting files from inside
  Dalica come later; so does the Options dialog.
- Videos are not listed.
- HEIC and HEIF photos (from iPhones) are listed but not shown in these builds (the HEIC decoder
  is left out); AVIF files are listed and never shown.
- Open With: on Windows it starts Dalica on its library window, without the photo you chose. On
  macOS, Dalica starts from Open With and ignores the file; on Linux it is not offered.
- On Windows, `dalica.exe --version` typed in a console prints nothing; `dalica-cli --version` or
  Help ▸ About Dalica tell you the version.
- This release is not signed with a developer certificate, so Windows and macOS warn you before
  its first start (see "Installing" under 0.1.0).

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
