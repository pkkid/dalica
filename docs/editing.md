# Editing: the edit view, tools and effects

Behaviour spec for single-photo viewing and editing. Storage of edits (the recipe), Save, Undo
Save and Revert are in [`data-model.md`](data-model.md) §3.1 and §6. Region sizes and colours are in
[`ui-layout.md`](ui-layout.md) and [`design-system.md`](design-system.md).

Naming rule: tool and effect names below are Dalica's. Where a familiar name is a trademark or a
person's name, we use a descriptive name instead (e.g. **Auto Fix**). Never rename these to match another product.

## 1. Entering and leaving

- Enter: double-click a thumbnail; select and press **Enter** or **Ctrl+3**; Picture → **View and
  Edit**; context menu → View and Edit (bold default item); from the photo viewer (**E**).
- Leave: the **Back to Library** button (top-left), **Esc** (if a tool panel is open, Esc first
  cancels that panel), View → Library (**Ctrl+1**, an edit-view binding; in the library Ctrl+1 is Small Thumbnails, see
  `menus-and-shortcuts.md` §2.0). Leaving never prompts: edits are already stored.
- The library keeps its scroll position and selection; the photo last viewed becomes the focused
  thumbnail.

## 2. Layout

```text
┌─────────────────────────────────────────────────────────────────────────────────────────────────┐
│ menu bar                                                                                         │
├─────────────────────────────────────────────────────────────────────────────────────────────────┤
│ [← Back to Library]   [▶ Play]   ◀ [t][t][t][▣][t][t][t] ▶          [ A | A│B | A│A ] [⇄][⇅]       │ edit top bar 40
├───────────────────────────┬─────────────────────────────────────────────────────┬───────────────┤
│ [🔧][☀][🖌][🖌][🖌]  tabs   │                                                     │ (optional     │
│ ─────────────────────────│                                                     │  side pane:    │
│  tab content             │                 PHOTO (fit to area,                 │  People /      │
│  (tool grid, tuning      │                  canvas background)                 │  Places /      │
│   sliders, effect tiles, │                                                     │  Tags /        │
│   or an open tool panel) │                                                     │  Properties)   │
│                          │                                                     │               │
│ [ Undo Crop ][  Redo  ]  │                                                     │               │
│ ┌ Histogram & camera ──┐ │       [◉]        Add a caption               [🗑]      │               │
│ │ RGB histogram        │ │                                                     │               │
│ │ Model  1/250s f/2.8  │ │                                                     │               │
│ └──────────────────────┘ │                                                     │               │
├───────────────────────────┴─────────────────────────────────────────────────────┴───────────────┤
│ « Folder › IMG_3440.jpg   2026-06-04 19:42   4000×3000   3.2 MB   (242 of 440)   tags…   ★ ✎     │ status line 20
├─────────────────────────────────────────────────────────────────────────────────────────────────┤
│ [tray]  [Hold][Clear][Album▾] │ ★  ↺  ↻ │ [Fit|1:1] 🔍───○─── │  [People][Places][Tags][Info]     │ bottom area
│                               │ [Export] ┆ [Collage] [Slideshow]                                    │
└─────────────────────────────────────────────────────────────────────────────────────────────────┘
```

- **Edit top bar** (40 pt, `chrome`): Back to Library; **Play** (starts a slideshow from this
  photo); the **filmstrip**: ◀ button, 7–9 square-cropped thumbnails (56 pt) of the current
  folder/album/search with the current photo centred and outlined white, ▶ button (click a
  thumbnail to open it; arrows step); the **compare toggle** (three segments: single "A", two
  different photos "A│B", the same photo twice "A│A"; tooltips "Show one photo", "Compare two
  photos", "Compare two versions of this photo"); in compare modes, **Switch Focus** (⇄) and
  **Switch Layout** (⇅, side by side ↔ above/below).
- **Edit panel** (left, 280 pt fixed, `chrome`; collapsible with Ctrl+9 or View → Edit Controls):
  five icon tabs (40 × 32, tooltips below), the tab content, the **Undo/Redo** pair (two buttons
  filling the width), and the **Histogram & camera info** box at the bottom.
- **Photo area** (`canvas`): the photo fitted with 24 pt padding, never upscaled beyond 100 % in
  Fit; a subtle shadow. The **caption bar** directly under the photo: left toggle (show captions in
  slideshow/viewer), centred field with placeholder **"Add a caption"**, right trash button.
- **Side pane** (right, optional, 260 pt): the same People / Places / Tags / Properties panes as in
  the library, toggled from the bottom bar.
- **Status line**: « (collapse the tray area), "Folder › file", date taken, pixel size, file size,
  "(n of N)", tags, and status badges (starred, edited, geotagged).
- **Bottom area**: same as the library (tray, star, rotate buttons, action buttons) except the
  thumbnail-size slider is replaced by **Fit** / **1:1** segmented buttons and the **zoom slider**.

## 3. Zoom and pan

- Zoom slider value v ∈ [0,1]: v = 0 → **fit**; v = 0.5 → **100 %** (always the slider midpoint);
  v = 1 → **400 %**. Lower half: scale = 1 + (2^(2v) − 1)·(r − 1); upper half: scale = r·2^(4(v −
  0.5)), where r = actual size ÷ fitted size and scale is relative to fit. Never smaller than fit.
- **Fit** and **1:1** buttons animate (0.22 s). Mouse wheel / Ctrl+= / Ctrl+− step without
  animation, anchored at the pointer, and snap exactly to 100 % when crossing it.
- **Click and hold** on the photo (when it is fitted) zooms to 100 % at that point; it stays at
  100 % after release, and a single click returns to fit. **Double-click** the photo returns to the
  library (Options → "Single-click to leave the edit view" makes one click do it, and disables
  click-to-zoom). A "Zoomed to 200 %" read-out shows at the lower right of the photo area while
  zoomed. When the photo is larger than the area, drag pans (hand cursor), bounded to the photo; a
  navigator thumbnail appears at the bottom-right while zoomed (Dalica addition).
- Previews are rendered at screen resolution for the visible region; zoomed views render the
  visible window at full resolution in the background ("refining" without blocking).

## 4. Compare modes

- **A│B**: two different photos. The focused side has a grey **"Selected"** pill (`#666`, white
  SemiBold 14, in the margin at the divider end). Click a side to focus it, then pick its photo from
  the filmstrip. Edits apply to the focused side.
- **A│A**: the same photo twice, for trying two different edit variants. Each side has its own
  recipe while the mode is open.
- Switching focus while a tool panel has unapplied changes asks **"Apply changes to the current
  photo?"** [Apply] [Discard] [Cancel] + "Don't ask again, always apply".
- Leaving A│A when the two sides differ asks **"Keep which version?"** — "This photo has two
  different sets of edits. Which one do you want to keep?" [Left]/[Right] (or [Top]/[Bottom]) [Cancel]
  + "Don't ask again".
- **Before/after** (Dalica addition; the reference used A│A for this): hold the **\** key (or press and hold the "Show original" button in the edit
  panel header) to show the unedited photo in place while held. (Holding **Ctrl+Alt** is a different
  feature: a full-screen preview of the current photo.)

## 5. The edit panel tabs

| # | Icon | Tooltip | Contents |
|---|---|---|---|
| 1 | wrench | "Basic fixes" | Crop, Straighten, Retouch, Auto Fix, Auto Contrast, Auto Color, Fill Light slider |
| 2 | half sun | "Lighting and color tuning" | Fill Light, Highlights, Shadows, Color Temperature, Neutral Color Picker |
| 3 | brush | "Effects" | 12 classic effects |
| 4 | brush + landscape | "More effects" | 7 effects |
| 5 | brush + sky | "Even more effects" | 8 effects |

While a tool or effect panel is open, clicking another tab is allowed, but the open panel stays and
the rest of the edit panel is drawn disabled until Apply/Cancel. Esc = Cancel.

### 5.1 Undo / Redo

- Two buttons under every tab. Undo reads **"Undo " + the last edit's name** ("Undo Crop", "Undo
  Tuning", "Undo Sepia", "Undo Save"); Redo reads **"Redo " + the next edit's name** when enabled,
  "Redo" when disabled. Tooltip "Remove the latest edit".
- History is **linear** per photo and persists across sessions (stored in the sidecar). In the edit
  view, Ctrl+Z / Ctrl+Y (Cmd+Z / Cmd+Shift+Z) and Edit → Undo / Redo walk **this photo's history**,
  never the library undo stack; back in the library they walk the library stack again
  (`architecture.md` §4). Rotations and flips made in the edit view are steps of this history
  ("Undo Rotate").
- Picture → **Undo All Edits** (also in the library for a selection): confirmation "Remove all
  edits from %d pictures? You can't undo this." [Remove Edits] [Cancel].
- Exception to linear history: **Crop** can be re-opened at any time; it shows the whole photo with
  the current rectangle and replaces the earlier crop without undoing later edits.

### 5.2 Histogram & camera info

Box at the bottom of the edit panel (≈ 248 × 150), titled **"Histogram & Camera Info"**: an RGB
histogram (R, G, B as translucent overlapping areas, additive where they overlap, normalised to the
mean so tall spikes clip), computed from the displayed preview and updated live while editing;
below it two columns: camera make/model, focal length (+ 35 mm equivalent) | shutter, aperture, ISO.
No EXIF: "No camera information".

## 6. Tab 1 — Basic fixes

Grid of tool buttons, three columns × two rows (icon 44 × 30 above a label), then the Fill Light
slider row, then Undo/Redo:

```text
Crop          Straighten     Retouch
Auto Fix      Auto Contrast  Auto Color
[☀] Fill Light ─────○─────────
```

### 6.1 Crop

The tab content is replaced by the **Crop** panel:
- Title "Crop Photo" with icon; instruction "Choose a size below, then drag on the photo to select
  the part you want to keep."
- **Size dropdown**, first row live: "Manual: 1210 × 1719"; then "Current ratio"; photo sizes per
  locale (US: "4 × 6", "5 × 7", "8 × 10", "8.5 × 11 (Letter)"; metric: "10 × 15", "13 × 18",
  "20 × 25", "A4"); "Square (1:1)"; "4:3 (Standard screen)"; "16:10 (Widescreen)"; "16:9 (HD video)";
  "5:3 (Photo frame)"; a section header "Custom ratios", the user's ratios, and **"Add Custom
  Ratio…"** (dialog: Width × Height, Name, live example line, [OK] disabled until valid, [Cancel]).
  A trash button next to the dropdown deletes the selected custom ratio. The last used ratio is
  remembered.
- **Three suggested crops**: 80 × 50 thumbnails of proposed rectangles in the chosen ratio (computed
  from detected faces, saliency and horizon); clicking one draws its rectangle.
- Buttons: **Rotate** (portrait ↔ landscape; disabled until a rectangle exists) | **Preview** (shows
  the cropped result while held/toggled); **Reset**; **Apply** (primary) | **Cancel**.
- On the photo: drag to draw (outside dimmed `black α140`); drag edges/corners to resize (ratio
  locked unless Manual); drag inside to move; Enter = Apply; Esc = Cancel.

### 6.2 Straighten

Clicking Straighten overlays a **dashed white grid** on the photo and floats a dark translucent bar
(`black α180`, radius 6) 10 pt above the photo's bottom edge, centred: a wide slider (thumb centred =
0°), then **Apply** and **Cancel** buttons. Range ±10°; the photo rotates live and is scaled up so no
empty corners show. Arrow keys nudge 0.1°. Esc = Cancel.

### 6.3 Auto Fix, Auto Contrast, Auto Color

One click, no panel, one undo step each:
- **Auto Fix** (Ctrl+Shift+E): auto contrast + auto colour + a little fill light, computed from the
  current (cropped) image.
- **Auto Contrast**: sets black/white points and midtones (per-channel-linked levels).
- **Auto Color**: removes colour casts (grey-world / white-patch balance with clamping).
Each result depends on the current image, so cropping first changes the outcome (as users expect).

### 6.4 Retouch

Panel "Retouch": instruction "Click a blemish, then click a clean area to copy from."; **Brush size**
slider; **Undo Patch** | **Redo Patch**; **Reset**; **Apply** | **Cancel**. Workflow: click the
target (a brush-size circle stays there); move the pointer — the target previews the texture under
the pointer; click to choose the source. Each patch is one step inside the tool; Apply commits all
patches as one "Retouch" edit. **+** / **−** resize the brush; the wheel zooms.

### 6.5 Fill Light (slider)

Brightens shadows and the foreground. 0–1, default 0. Shares its value with Tuning → Fill Light
(they are the same parameter of the Tuning operation).

## 7. Tab 2 — Tuning

| Control | Range (default) | Effect |
|---|---|---|
| **Fill Light** | 0–1 (0) | lifts shadows/foreground |
| **Highlights** | 0–0.5 (0) | brightens bright areas only |
| **Shadows** | 0–0.5 (0) | darkens dark areas only |
| ↳ magic wand **"Auto lighting"** | — | sets the three above automatically |
| **Color Temperature** | −1…+1 (0, centred) | left cooler/blue, right warmer/yellow |
| **Neutral Color Picker** | swatch (none) + eyedropper | click a neutral grey/white area to remove a cast; tooltip "Pick a grey or white area of the photo to remove a color cast" |
| ↳ magic wand **"Auto color"** | — | picks a neutral colour automatically |

All five controls are **one "Tuning" edit**, updated live while dragging (one undo step per tab
session). Sliders show no numbers (as in the reference), but hovering shows the value as a tooltip.
**+**/**−** nudge the hovered slider by 2 %; double-click resets it.

## 8. Effects (tabs 3–5)

### 8.1 Common behaviour

- Each tab is a fixed grid of tiles, three columns and at most four rows (no scrolling): a live 78 × 48 preview of the current
  photo with the effect at default settings, label below (SemiBold 11), tooltip describing it.
  Tiles render asynchronously (placeholder `gray 38`).
- **One-click effects** (Sepia, B&W, Warmify, Invert Colors) apply immediately and show a small
  "1-click" corner badge on their tile.
- **Parameter effects** replace the grid with a panel: title (effect name), up to 4 sliders with the
  label centred above each, optional checkboxes, colour swatches (with eyedropper), a crosshair on
  the photo for focal effects, **Apply** | **Cancel**. The photo previews live.
- **Fade** (0–100, default 0) on most newer effects blends the result back toward the input
  (alpha = 1 − Fade/100).
- **Crosshair effects** (Soft Focus, Focal B&W, Graduated Tint, Focal Zoom)
  show a draggable crosshair on the photo to set the centre / gradient start and angle.
- Effects stack in the order applied; the same effect may be applied several times.
- Effects that change the canvas size (frames, widescreen crop) say so in their tooltip.

### 8.2 Tab 3 — Effects

Grid order: Sharpen, Sepia, B&W / Warmify, Film Grain, Tint / Saturation, Soft Focus, Glow /
Filtered B&W, Focal B&W, Graduated Tint.

| Effect | Controls: label min–max (default) | Result |
|---|---|---|
| **Sharpen** | Amount 0–3 (0.6) | unsharp mask; accurate at 1:1 |
| **Sepia** | one-click | warm brown monochrome (fixed curves) |
| **B&W** (Ctrl+Shift+B) | one-click | luminance greyscale (Rec.601 weights) |
| **Warmify** | one-click | warmer, golden skin tones |
| **Film Grain** | Grain 0–50 (10), Lighten checkbox (off) | monochrome noise |
| **Tint** | Tint color (#80CFFF), Fade 0–100 (0) | desaturate then colourise |
| **Saturation** | Amount −1…+1 (+0.16) | toward B&W ↔ vivid |
| **Soft Focus** | crosshair, Size, Amount | blur outside a sharp circle |
| **Glow** | Intensity 0–1 (0.65), Radius (log scale, 3.0) | highlights bloom |
| **Filtered B&W** | Filter color | B&W weighted by a coloured filter |
| **Focal B&W** | crosshair, Size, Sharpness | colour inside a circle, B&W outside |
| **Graduated Tint** | crosshair (drag sets extent and angle), Color (light blue), Feather 0–1 (0.25), Shade 0–1 (0.25) | coloured gradient over the sky; Shade deepens colour, whites stay white |

### 8.3 Tab 4 — More effects

Grid: Infrared Film, Plastic Lens, HDR Look / Widescreen Film, Dream Glow, Invert Colors /
Duo-Tone.

| Effect | Controls (default) | Result |
|---|---|---|
| **Infrared Film** | Fade (0) | B&W infrared: foliage bright, skies dark |
| **Plastic Lens** | Blur Edges 0–100 (70), Grain 0–100 (30), Fade (0) | soft, grainy, vignetted plastic-lens look |
| **HDR Look** | Radius 1.3–80 (20), Strength 1–7 (3), Fade (0) | large-radius local contrast (out = in + C·(in − blur(r))) |
| **Widescreen Film** | Letterbox checkbox (on) | crop to 1.7:1, muted filmic colour, grain, 15 % black bars top and bottom; changes size |
| **Dream Glow** | Bloom 0–50 (25), Brightness 0–100 (50), Fade (0) | blurred copy blended in overlay + midtone lift |
| **Invert Colors** | one-click | negative (turns scanned negatives positive) |
| **Duo-Tone** | First color (#004488), Second color (#FFFF00), Brightness −95…95 (0), Contrast 0–100 (20), Fade (0) | tone mapped onto a two-colour gradient |

### 8.4 Tab 5 — Even more effects

Grid: Boost, Soften, Vignette / Pixelate, Focal Zoom, Border / Drop Shadow, Museum Matte.

| Effect | Controls (default) | Result |
|---|---|---|
| **Boost** | Strength 0–100 (50) | stronger colour and contrast |
| **Soften** | Softness 0–100 (50), Fade (50), paintable mask + eraser | soft glow blur, paint where it applies |
| **Vignette** | Size 0–50 (35), Strength 1–2 (1.4), Color (black; pick from photo), Fade (0) | darkened (or coloured) edges; repeatable |
| **Pixelate** | Pixel Size 2–150 (20), Blend mode (Normal), Fade (0) | mosaic |
| **Focal Zoom** | crosshair, Zoom Amount 1–100 (50), Focal Size, Edge Hardness 0–100 (50), Fade (0) | radial zoom blur outside a sharp circle |
| **Border** | Outer color (black), Outer thickness 0–100 (20), Inner color (white), Inner thickness 0–100 (5), Corner radius (0; max = oval), Caption height (0) | two-band frame; enlarges canvas |
| **Drop Shadow** | Shadow color (black), Distance 0–30 (4), Angle 0–360 (90), Blur 0–100 (10), Fade (30), Background (white) | photo floating over a background; enlarges canvas |
| **Museum Matte** | Outer color (#1A0E03), Outer thickness 0–100 (25), Inner color (#F0EAE4), Inner thickness 0–100 (40) | double mat with inner shadow; enlarges canvas; repeatable |

Algorithms are implemented from the effect descriptions above and our own design (no reference
code). Each effect has a golden-image test on a procedural scene so its look is pinned.

## 9. Edit menu and batch editing

- Edit → **Copy All Edits** (one photo selected): copies the photo's whole recipe. Edit → **Paste
  All Edits**: **replaces** the recipe of the open photo or every selected photo (one undo step;
  videos skipped with a toast).
- Picture → **Batch Edit ▸** (applies to the selection or tray; appends to each photo's recipe; one
  undo step for the whole batch):
  Rename… (F2) · — · Rotate Clockwise · Rotate Counterclockwise · — · Auto Contrast · Auto Color ·
  Auto Fix · — · Sepia · Sharpen · Warmify · Film Grain · Black and White.
- Library thumbnails always show photos with their edits applied and the *edited* badge.

## 10. Rotation

- Ctrl+R / Ctrl+Shift+R and the bottom-bar buttons rotate 90° CW/CCW (stored as a `rotate(…)` operation
  in the recipe; consecutive rotations merge, `data-model.md` §3.1). Undo follows the view: a rotation made in the library is a
  library undo step; one made in the edit view is a step of the photo's history. Ctrl+Shift+H / Ctrl+Shift+V flip horizontally/vertically
  (recorded as edits).
- EXIF orientation is applied on read; Save normalises it (`data-model.md` §6).

## 11. Video in the edit view (M9)

- A video opens in the same view; under it a control bar: play/pause, scrubber with time readout,
  volume, a **trim ruler** with start/end handles, **Set Start**, **Set End**, **Full Screen**.
- The edit panel shows a video tab instead of tabs 1–5: **Save Frame as Picture** (writes a JPEG
  into `Pictures/Dalica/Video Frames`), **Export Clip** (trimmed copy), rotate buttons.
- Keys: **/** or Space play/pause, **,** / **.** frame step.
- Trim and rotation are recipe entries (`trim(start,end)`); there is no colour editing of video.

## 12. Keyboard summary (edit view)

| Key | Action |
|---|---|
| Esc | cancel open tool; otherwise back to library |
| Ctrl+1 | back to library |
| ← / → (also PgUp/PgDn) | previous / next photo |
| Home / End | first / last photo |
| Ctrl+R / Ctrl+Shift+R | rotate CW / CCW |
| Ctrl+Shift+H / Ctrl+Shift+V | flip horizontal / vertical |
| Ctrl+Shift+E / Ctrl+Shift+B | Auto Fix / B&W |
| Ctrl+8, Space | toggle star |
| Ctrl+Z / Ctrl+Y | undo / redo in this photo's history |
| Ctrl+S | Save |
| Ctrl+Shift+S | Export |
| Ctrl+4 | slideshow from this photo |
| Ctrl+9 / Ctrl+0 | toggle edit panel / side pane |
| \ (hold) | show original |
| Ctrl+Alt (hold) | full-screen preview |
| F11 | full-screen window |
| + / − | nudge hovered slider; resize retouch brush |
| Wheel / Ctrl+= / Ctrl+− | zoom |
| Click-and-hold photo / double-click photo | zoom to 100 % / back to library |
