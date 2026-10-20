# Viewing: slideshow, quick preview, photo viewer, video

Behaviour spec for every way of *looking at* photos outside the light table. The edit view (with its
zoom and compare modes) is in [`editing.md`](editing.md).

| Surface | Entry | Exit |
|---|---|---|
| Slideshow | section header ▶, edit-view ▶ Play, View → Slideshow (Ctrl+4), photo viewer ▶ | Esc, Exit button |
| Quick full-screen preview | hold **Ctrl+Alt** while hovering a thumbnail (or in the edit view) | release |
| Photo viewer (standalone) | opening an image file from the OS file manager when Dalica is the default viewer; `dalica --view <file>` | Esc, × |
| Video player | double-click a video | Back to Library |
| Whole-window full screen | F11 | F11 |

## 1. Slideshow

### 1.1 What plays

- The section's folder/album, starting at the selected photo if one is selected in it (else the
  first). From the edit view: the current folder/album starting at the current photo. From a search:
  the results. Videos are included and play to the end before advancing (muted unless "Play video
  sound in slideshows" is on).
- Order = the section's display order.

### 1.2 Presentation

- Full screen, pure black background, photo fitted (never upscaled beyond 2× of its pixel size),
  centred. Rendered at screen resolution (always full quality).
- The next photo is decoded ahead of time; a transition never waits on the disk.
- Captions (when the caption toggle is on): the photo's caption centred near the bottom, 22 pt
  Inter on a `black α160` rounded plate, max 70 % of the screen width, wrapping to 2 lines.
- Starts **playing**; the control bar is visible for 2 s at start, then hides. Moving the mouse or
  pressing Space shows the bar; **showing the bar by mouse does not pause**; Space toggles
  pause/resume. (The reference's versions disagreed here; this is our decision.)
- End of show: without Loop, the last photo stays and the bar appears with Play enabled (Play
  restarts from the beginning); with Loop, it wraps around.

### 1.3 Control bar

A rounded translucent dark panel (`black α170`, radius 10) centred along the bottom: shown for 2 s
when the show starts, for 2.5 s after a mouse move or Space, for good at the end, and kept while the
pointer rests on it or its drop-up is open; it fades in and out over 0.2 s. Left → right:

```text
╭──────────────────────────────────────────────────────────────────────────────────────────────╮
│ [⇦ Exit] [🔍]○━━━━━━  [↺][↻]   ⇦   (▶/❚❚)   ⇨   [★]   [Dissolve     ▴]  [▭ ☑]   Display time  │
│                                                                                  [−] 3 s [+] │
╰──────────────────────────────────────────────────────────────────────────────────────────────╯
```

| # | Control | Behaviour |
|---|---|---|
| 1 | **Exit** | ends the show (Esc) and returns to where it started, with the last shown photo selected |
| 2 | **Zoom** (magnifier + slider) | zooms the current photo (pauses while zoomed; drag to pan) |
| 3–4 | **Rotate left / right** | rotates the photo (the same non-destructive rotation as in the library) |
| 5 | **Previous** | disabled on the first photo when Loop is off |
| 6 | **Play / Pause** | toggle |
| 7 | **Next** | |
| 8 | **Star** | toggles the star on the current photo |
| 9 | **Transition** (drop-up list) | **Cut**, **Pan and Zoom**, **Dissolve** (default), **Wipe**, **Circle**, **Push**, **Rectangle** |
| 10 | **Captions** toggle | caption icon + checkbox |
| 11 | **Display time** | `[−] 3 seconds [+]`, 1–60 s in 1 s steps (default 3) |

Transition, display time and caption settings persist (Options → Slideshow mirrors them): the bar's
controls store them at once (`options.set`, no undo step), and the edit view's caption-bar toggle
is the same captions setting. A change applies to the show playing straight away (§1.6).
Transitions last 0.8 s (Pan and Zoom spans the whole display time: a slow 5–10 % zoom/pan toward a
detected face or the centre: the photo's largest named face, else its largest unnamed face, else its
centre).

### 1.4 Keys and mouse

| Input | Action |
|---|---|
| Esc | exit |
| Space | pause / resume (and show the bar) |
| → / ↓ / PgDn | next |
| ← / ↑ / PgUp | previous |
| Home / End | first / last |
| Ctrl+R / Ctrl+Shift+R | rotate |
| Ctrl+8 | star |
| + / − | zoom in / out |
| Mouse move | show the bar |
| Wheel | previous / next |
| Click | next (only when the bar is hidden) |

### 1.5 Options → Slideshow

**Loop slideshow** (off) · **Play music during slideshows** (off) + music folder [Browse…] (every
MP3/OGG/FLAC/WAV in the folder plays in name order, looping) · **Display time** · **Transition** ·
**Show captions** (on) · **Play video sound** (off). A folder's own music (Folder Properties →
Music) overrides the global music folder.

Until Dalica plays sound, Play music, the music folder, Play video sound and Folder Properties'
Music are drawn disabled with the reason "Arrives with sound support". The tab groups them under
**Sound**, below Loop, Show captions, Display time (a 1–60 s slider with its number field) and
Transition.

### 1.6 Decisions (M11.4, M11.11, M11.21, M13.23)

The reference left these open; this is how Dalica behaves.

- **Timing.** A photo's display time starts the moment its change begins: with 3 s and Dissolve a
  photo fades in over 0.8 s and is then alone for 2.2 s. Pausing freezes the display time (and Pan
  and Zoom's movement); a change already under way still finishes.
- **A new display time** counts the time the photo has already been shown; when that is already
  past the new value the show moves on by one photo, never skipping any.
- **The end.** Next on the last photo without Loop ends the show as its timer would (the bar stays,
  Play restarts from the first photo); stepping back from the end shows that photo paused. Space at
  the end is Play.
- **Zoom.** Zooming in (+, the magnifier, the slider; 1 fitted … 8) pauses the show; Space while
  zoomed resumes it. Zooming back out to fitted lets the show go on as it was. Moving to another
  photo (a key, the wheel, the timer) ends the zoom: the next photo is shown fitted. Zoomed, a drag
  pans the photo, which always covers the screen where it is larger than it. The magnifier goes from
  fitted to 2× and back to fitted from any zoom.
- **Transitions.** Pan and Zoom changes photos with a 0.8 s dissolve; each photo grows by 5 % plus
  (its place in the show mod 6) % about its focus point, within its fitted frame. The focus point is
  the centre of the photo's largest named face (by its area on the picture shown), else of its
  largest unnamed face, else the photo's centre; ignored faces and faces cropped away do not count,
  and a face is mapped through the photo's crop, straighten and turns. `slideshow.list` names that
  face and its point for each photo that has one (M13.23). Wipe runs left to
  right, Push comes in from the right, Circle and Rectangle open from the centre, all eased (slow
  in, slow out). Each photo of a change is painted over black within its own area, the incoming one
  on top at its opacity, so Dissolve is a true cross-fade.
- **Caption.** The plate has 14 × 8 pt of padding and radius 8; its bottom sits 8 % of the screen's
  height above the edge, and never lower than the control bar's top plus 8 pt, so the bar never
  covers it.
- **Never waiting on disk.** The photo shown and the next are kept as screen-size previews, and the
  two after that are prepared as soon as a photo is shown, so with decoding as slow as half the
  display time no change waits. A photo removed from the library during the show is skipped.
- **What the bar keeps.** Its settings change Options → Slideshow's values at once, and the show
  playing takes them from its next frame: a new transition from the next change, Loop (Previous is
  live on the first photo with Loop on), the display time as above.

## 2. Quick full-screen preview (Ctrl+Alt)

Hover a thumbnail and hold **Ctrl+Alt** (macOS: Cmd+Option): the photo fills the screen on black
immediately (from the best cached preview, refined in place). While held, the **mouse wheel** steps
through the neighbouring photos of the section (one photo per notch; it stops at the section's first
and last photo, and the light table does not scroll). Releasing returns to the light table, with the
last shown photo focused and scrolled into view. Works on the current photo in the edit view too
(releasing on another photo opens that one). Over empty space, in a dialog or while typing in a text
field, the keys do nothing. No controls are shown. Agents hold the keys with the control channel's
`ui.key {state: "down"}`, or run `peek {id}` and `peek {id: null}`.

## 3. Photo viewer (standalone)

A lightweight mode of the same binary for viewing image files from the file manager, without the
library window. Until the OS-integration milestone it opens with `dalica --view <file>`; registering as the
default viewer for chosen file types (offered at first run, opt-in per type, and in Options →
Photo Viewer) comes with that milestone (`product.md` → Scope).

### 3.1 Look

```text
                                       (desktop / dark overlay)                         close ✕
                     ┌────────────────────────────────────────────────┐
                     │                                                │
                     │                    PHOTO                       │
                     │                                                │
                     └────────────────────────────────────────────────┘
                               IMG_0151.jpg (3008 × 2000)
══════════════════════════════════════════════════════════════════════════════════════════════
  [th][th][th][th][th][th]                 ╭───╮                 [th][th][th][th][th][th]      ← filmstrip
        [+][−][1:1]   [Open in Dalica]     │ ▶ │    [↺][↻]   [★]   [⋯]
══════════════════════════════════════════╰───╯═══════════════════════════════════════════════
```

- A borderless full-screen window drawn over a dark translucent overlay (`black α210`; fully
  opaque `#000` where the platform cannot do translucent windows, e.g. some Wayland compositors).
  Space toggles translucent ↔ opaque background.
- The photo is fitted above the bottom strip. Above the strip: "filename (W × H)" in white SemiBold.
- **Filmstrip**: a dark band across the full width with thumbnails of the other images in the same
  folder, centred on the current one (others dimmed); click to jump.
- **Control bar** over the strip: zoom in **+**, zoom out **−**, **1:1**, **Open in Dalica** (opens
  the photo in the main window's edit view), a large round **Play** (slideshow of the folder),
  rotate left/right, **star**, **⋯** More (Open With ▸: the applications the system offers for the photo, asked when the
  submenu opens, none for a photo the library does not hold; Show in File Manager: the photo
  shown, selected in the system's file manager; Settings…). "close ✕" at the top-right. Controls auto-hide after 3 s of no mouse movement.
- Shows Dalica edits (from the folder sidecar) when present. A photo the library holds is drawn
  from the edit view's preview; any other is decoded by its path at the screen size (and small for
  the filmstrip) on the viewer's own workers, the next and previous decoded ahead (M11.17).

### 3.2 Keys and mouse

| Input | Action |
|---|---|
| ← / → , J / K | previous / next image in the folder |
| Wheel | zoom (default; Ctrl+wheel navigates) — or navigate (Ctrl+wheel zooms), per Settings |
| + / −, PgUp / PgDn | zoom in / out |
| 1 | toggle 100 % |
| drag | pan when zoomed |
| Ctrl+R / Ctrl+Shift+R | rotate |
| 8, Ctrl+8 | star |
| E | open in Dalica |
| Space | toggle background |
| Delete | move to trash (confirmation) |
| Esc, click on the overlay outside the photo | close |

Stepping stops at the first and last photo of the folder and shows the new photo fitted. The
changes act on the photo shown; Open in Dalica opens the photo shown, whichever it is (M11.17).

### 3.3 Settings (⋯ → Settings…, Options → Photo Viewer)

Tabs: **File Types** — radio "Use Dalica as the default viewer for these file types:" with a
checkbox per type showing the current handler ("JPEG (Image Viewer)"), [Current] [Select All]
[Select None]; radio "Don't use Dalica as a viewer". **Interface** — "Mouse wheel zooms (hold Ctrl
to navigate)" / "Mouse wheel moves through photos (hold Ctrl to zoom)", "Hide controls
automatically", "Translucent background". [OK] [Cancel]. The dialog opens on Interface (File
Types needs the OS integration and is drawn disabled until then); OK sets what changed, and the
viewer follows the options at once (M11.17).

### 3.4 The viewer's session and `dalica --view` (decided, M11.12)

- **Launch.** `dalica --view FILE` opens the library as usual and the viewer on FILE (a relative
  FILE is taken from the working folder; any name, a leading `-` included, is a file, never an
  option). The window is borderless and fills the screen, transparent where the platform allows and
  opaque black otherwise or when "Translucent background" is off (a platform that cannot show
  through clears the transparent window to black). With `--headless` there is no window: the
  viewer's state is served over the control channel (`engine.execute viewer.list`).
- **Listing.** The engine lists the file's folder on a worker (`viewer.list {path?, refresh?}`,
  `{pending: true}` until it is there): the photos Dalica can show, as a scan reads that folder
  alone (its subfolders are not listed), in name order (case ignored, then exact), and the file's
  place among them. A file Dalica cannot show is refused with its reason (no such file, a folder,
  not a picture Dalica reads or its type off in Options ▸ File Types, or why its header could not
  be read). Only the viewer's folder or a folder the library indexes is listed, so automation reads
  nothing through the viewer it could not read otherwise.
- **Whose photos.** A photo of a folder the library holds is the library's own (its id, its star as
  the library has it); any other is shown from its file and its folder's sidecar, read only.
  Viewing writes nothing and adds nothing to the library.
- **The first change** to a photo of a folder the library does not hold (star, rotate, delete, and
  Open in Dalica) adds that folder as Scan Once, as Open File… does: the folder and its photos as
  listed enter the library and its rule is set inside the same command, so a star or a rotation
  and the adding are one undo step (one Undo takes both back). The engine commands are
  `viewer.star {path, on?}`, `viewer.rotate {path, direction}`, `viewer.delete {path, confirm?}`
  and `viewer.adopt {path}` (Open in Dalica); a later change is an ordinary change of a library
  photo. A delete is a file job with an undo step of its own: there the adding is a library step
  just before it.
- **Another Dalica holds the library:** the viewer opens on an empty read-only library of its own in
  a temporary folder (removed when the app ends): every photo is shown from its file and sidecar,
  edits included, and the changes and Open in Dalica are disabled with "Dalica is already open".
  Handing the file to the running Dalica instead is a question for the milestone's review.
- **Closing.** Esc and × close the viewer in the engine (`Session::close_viewer`) and the app
  quits. Open in Dalica (`Session::open_in_dalica`) turns the same window into the main window: out
  of full screen and opaque, the frame's decorations, its saved size and place, on the edit view of
  that photo. The viewer's UI (§3.1–§3.3) starts on the viewer view when the session holds an open
  viewer.

## 4. Video player (in the edit view)

See [`editing.md`](editing.md) §11 for the panel and trim controls. Playback: play/pause, scrubber
with time readout "00:00:09 / 00:00:41 (trimmed to 00:00:21)", volume, full screen; keys **/** or
Space play/pause, **,** / **.** frame step (hold to rewind / fast-forward). Unsupported codecs show
the poster frame with "This video can't be played" and [Open With ▸].

Library: videos show their first non-black frame as the thumbnail with the film-sprocket edge and
the duration pill; the **Videos** filter shows only videos.
