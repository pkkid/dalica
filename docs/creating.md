# Creating: export and collage

Behaviour spec for every output Dalica makes. All outputs act on the **Photo Tray** (bottom-bar
buttons, menus) or on a **section** (section-header buttons), never modify originals, render edits
into the output, and run in the background with progress and Cancel.

Out of scope: the reference organizer's online services (web-album upload and sync, sign-in,
blogging, online print ordering, online editors) and its dated output features: email, printing
(print room, contact sheets, posters), movie maker, web-page export, gift folders, screensaver,
set as desktop, backup and the configurable button bar. Sharing happens through **Export**: the
user sends or uploads the exported files with whatever app they like.

## 1. Entry points

| Output | Bottom bar | Section header | Menu | Shortcut | Opens as |
|---|---|---|---|---|---|
| Export | Export | Export | File → Export Pictures to Folder… | Ctrl+Shift+S | dialog |
| Collage | Collage | ▦ | Create → Picture Collage… | | room |

Generated files go under `Pictures/Dalica/` (`Exports/<name>`, `Collages`, `Video Frames`) and
appear in the **Projects** group of the library tree (flat view) or under their folder in tree view.

## 2. Export

Dialog **"Export to Folder"** (labels right-aligned, controls left-aligned):

```text
Export location:          [~/Pictures/Dalica/Exports            ] [Browse…]
Name of exported folder:  [Lisbon Weekend                        ]
                          [ ] Add numbers to file names to keep the order
Image size:               ( ) Use original size
                          (•) Resize to: [1600] pixels  ───────○─────
Image quality:            [Automatic ▾]  ───────────○──  (enabled for Custom)
Export videos as:         (•) First frame   ( ) Full video (no resizing)    (disabled without videos)
Watermark:                [ ] Add watermark  [© Ana Costa 2026                 ]
                          Stamp photos with your name, a web address or a copyright notice.
                                                                    [Export] [Cancel]
```

- Defaults: location `Pictures/Dalica/Exports`, folder name = the section/album name (or "Export
  2026-06-04" for a mixed tray), **Resize to 1600** (slider 100–4000 with stops at 480, 640, 800,
  1024, 1280, 1600, 2048, 3840), quality **Automatic**.
- **Image quality**: Automatic ("Keeps the original's quality"), Normal ("Balances quality and
  size", 85), Maximum ("Keeps fine detail; large files", 95), Minimum ("Small files with some quality
  loss", 60), Custom (slider 1–100, shows "Custom (75)").
- Resize = longest side; never upscales. Output JPEG (PNG sources with transparency → PNG); EXIF/XMP
  copied (GPS removable via Options → Export → "Remove location from exported photos").
- Watermark: bottom-right, white text with a soft shadow, size 3 % of the long side.
- Name collisions get `-001`. A new folder per export.
- Progress in the status line and a small progress window ("Exporting 12 of 48…", [Cancel]); when
  done a toast "Exported 48 pictures" with [Show Folder] (*OS integration*; until then the
  library scrolls to the export folder); the export folder appears in the library in the
  **Exported** project group. If the chosen location is not inside a watched folder, the export
  folder is added to the watched folders as Scan Always so it can appear there.
- File names: each photo keeps its own stem, sanitized (`security.md` §5.4; `Photo` when nothing
  is left), with `.jpg`, or `.png` for a source with transparency. "Add numbers" puts `001-`,
  `002-`, … in front in tray order (four digits from 1,000 photos, so the names sort in tray
  order); a name that is taken in the export folder gets `-001`, `-002`, … as it is written,
  never replacing a file. The export folder's name is sanitized too (the fallback is
  "Export 2026-06-04"); a taken name becomes `<name>-001`. Videos are skipped and counted until
  they can be decoded (M15); a file that cannot be read as a picture is skipped with its reason.
- Cancel stops before the next step of the photo being exported (reading, rendering, writing): a
  file is written whole to a temporary name and given its own in one step, so only complete files
  are ever left in the export folder.

**Built (M8.13, `dalica-create` `export`):** `ExportOptions` (size: original or a long side
100–4000; the quality presets; numbers; watermark text; remove location) and the request it makes
of the output pipeline (no EXIF thumbnail; every owned field written, stars as ratings too);
`export::plan` (the location made when missing, the export folder under a free sanitized name,
each photo's stem and number, videos skipped); `export::export_one` (read under 1 GiB, render
through `output::render`, write through `fs::create::new_file`, cancel asked before each step) and
`export::run` (the whole plan on one thread). The command and its job (M8.16) and the dialog (M8.20)
follow.

## 3. Collage (room)

```text
┌ Settings ┐┌ Clips (16) ┐        [Select All] [Select None] [Remove] [Set as Background]
│ [icon] Picture Pile:                  ▾│   ┌────────────────────────────────────────────┐
│        Looks like a pile of pictures   │   │                                            │
│ Picture borders  [none][white][instant]│   │        live collage canvas (page aspect)   │
│ Grid spacing     None ────○──── Max    │   │                                            │
│ Background  (•) Solid color [■][⌖]     │   └────────────────────────────────────────────┘
│             ( ) Use picture            │     [Scramble Collage] [Shuffle Pictures] [View and Edit]
│ Page format [16:9 (HD video) ▾][🗑]    │
│             [▭ landscape][▯ portrait]  │
│ [x] Draw shadows   [ ] Show captions   │
├────────────────────────────────────────┤
│ [Create Collage]                       │
│ [Reset]              [Close]           │
└────────────────────────────────────────┘
```

- **Styles** (dropdown with icon + name + one-line description): **Picture Pile** (default; "Looks
  like a pile of scattered pictures"), **Mosaic** ("Fits pictures into the page automatically"),
  **Frame Mosaic** ("A mosaic around a large center picture"), **Grid** ("Pictures in regular rows
  and columns"), **Contact Sheet** ("Small pictures with their names"), **Multiple Exposure**
  ("Pictures blended on top of one another").
- Options availability by style: Picture borders (Pile, Contact Sheet), Grid spacing (Mosaic, Frame
  Mosaic, Grid), Show captions (Pile), Background (all but Multiple Exposure).
- **Page format**: the crop ratio list (`editing.md` §6.1) plus **Current Display**; orientation
  toggles; custom ratios deletable.
- **Clips tab** "Clips (N)": thumbnails of available pictures, [+] add selected to the collage, [−]
  remove, **Get More** (switches to the Library tab; the tray shows **Back to Collage**). Delete on a
  canvas picture returns it to Clips.
- **Canvas**: click selects (`accent` outline); Pile: drag to move, a rotate/scale wheel handle on
  the selected picture, right-click **Bring to Top** / **Send to Bottom**; Mosaic/Grid/Frame
  Mosaic: drag to swap; **Set as Background** / **Set as Frame Center**; **Scramble Collage** (new
  random arrangement), **Shuffle Pictures** (new assignment of pictures to slots), **View and Edit**.
- **Create Collage** renders a JPEG (long side 3000 px, or the screen size for Current Display) into
  `Pictures/Dalica/Collages` plus a `.dalica-collage` recipe (JSON) next to it, then opens the result
  in the edit view. **Reset** restores defaults.
  **Close** with unsaved changes: [Save Draft] [Discard] [Cancel]. Double-clicking a collage in the
  library shows an **Edit Collage** button in the edit view's top bar that reopens the room.
