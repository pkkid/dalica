# Design system

Dalica's **layout and interaction model** follow the reference photo organizer described in
[`ui-layout.md`](ui-layout.md). Its **look** does not: Dalica uses the muted, dark, neutral-grey
design language of the Craft apps ([LightCraft](https://github.com/storytold/lightcraft) and
[PhotoCraft](https://github.com/storytold/photocraft)), so it sits naturally beside them. When this
document and a Craft app disagree on a value, this document wins for Dalica; when this document is
silent, match LightCraft's look (study `crates/ui-egui/src/theme.rs`, `widgets.rs`, `icons.rs` at
revision 88ea1a8) and record the value here. The Craft apps are reference only: Dalica's theme,
widget and icon code is its own (`architecture.md` §9).

> Rule of thumb: **the chrome is quiet so the photos are loud.** Neutral greys only, one blue
> accent, no gradients on chrome, photos never rounded, never tinted.

## 1. Principles

1. **Tokens, not literals.** Every colour, radius and metric used by a widget comes from
   `theme::Tokens` (read with `Tokens::get(ctx)`). Hard-coding a `Color32` in a panel is a review
   failure. The only exceptions are photo-content colours (histogram channels, colour-label dots)
   which live as named constants in `theme.rs` too.
2. **One accent.** `accent` (#0165dd) marks focus, the selection fill in text, active filters and
   drop targets. It is never used for decoration.
3. **Selection on photos is white, not blue.** The active photo has a 2 pt white outline; other
   selected photos a 2 pt `gray 170` outline. Blue is reserved for focus/active controls.
4. **Hierarchy through weight and grey level**, not colour: titles `text` SemiBold, labels
   `text_label`, secondary information `text_dim`, disabled `text_disabled`.
5. **Everything is hand-painted and addressable.** Custom widgets allocate their rect, paint with
   tokens, and register an automation id (`register(ctx, id, rect)`) so tests and agents can find
   and click them by name (see [`architecture.md`](architecture.md) → Automation).
6. **Dense but calm.** 13 pt body text, 22–24 pt control height, 4 pt corner radii, 80 ms
   animations. No bouncing, no long fades.

## 2. Colour tokens

Dark theme (the only theme until after 1.0; a light variant is a later option, not a goal).

| Token | Hex | Used for |
|---|---|---|
| `chrome` | `#2d2d2d` | toolbar, side panes, bottom tray area, menu bar, dialogs (`panel_fill`, `window_fill`) |
| `canvas` | `#1c1c1c` | edit-view photo canvas, filmstrip, folder header blocks, selected tree row, badges plates |
| `grid_bg` | `#0f0f0f` | background of the light table (thumbnail area) |
| `cell` | `#1c1c1c` | thumbnail cell surround (square cells) |
| `cell_selected` | `#2d2d2d` | selected cell surround |
| `divider` | `#1c1c1c` | 1 pt separators between regions, panel strokes |
| `inset` | `#232323` | sunken rows, tool tiles in the edit panel |
| `field` | `#222222` | text fields, search box |
| `field_border` | `#3c3c3c` | text-field border at rest |
| `button` | `#242424` | button fill at rest |
| `button_border` | `#4c4c4c` | button stroke, window stroke |
| `hover` | `#3a3a3a` | hovered rows and buttons |
| `pressed` | `#3f3f3f` | pressed / toggled-on buttons |
| `tool_active` | `#3f3f3f` | the active edit tool, an active filter toggle |
| `text` | `#e2e2e2` | primary text: titles, values, selected rows |
| `text_label` | `#bcbcbc` | default text (egui `override_text_color`) |
| `text_dim` | `#8e8e8e` | counts, captions under thumbnails, hints, placeholders |
| `text_disabled` | `#5c5c5c` | disabled controls |
| `icon` | `#9a9a9a` | icon glyphs at rest (hover → `text`, active → `pick`) |
| `track` | `#5a5a5a` | slider tracks, the thumbnail-size slider |
| `thumb` | `#a0a0a0` | slider ring at rest |
| `thumb_hover` | `#e0e0e0` | slider ring hovered / dragged |
| `accent` | `#0165dd` | focus border, text selection (`accent × 0.6` fill), active filter, drop target outline |
| `star` | `#d8d8d8` | star badge glyph on thumbnails, star buttons |
| `pick` | `#f0f0f0` | active icon glyph |
| `danger` | `#e04a4a` | destructive confirmations, error badges |
| `caution` | `#e3a83c` | warnings (offline folder, unsaved edit pending, "file missing") |

Dalica additions (not in LightCraft):

| Token | Value | Used for |
|---|---|---|
| `folder_header` | `#1c1c1c` (`canvas`) | the header block above each folder's thumbnails on the light table |
| `tray_bg` | `#262626` | the photo-tray well in the bottom area |
| `drop_target` | `accent` 1.5 pt outline + `accent × 0.15` fill | folder/album rows and tray while a drag hovers them |
| `filter_banner` | `#2f3b4d` | the "filter active" banner across the top of the light table |
| `face_box` | `#f0f0f0` 1.5 pt, `black α160` 1 pt outer | face rectangles in the edit view |

Other fixed values: dialog backdrop `black α140`; popup shadow offset (0,4) blur 16 `black α140`;
window shadow (0,8) blur 30 `black α160`; slideshow and photo-viewer background pure black
(viewer overlay `black α210` over the desktop/app); thumbnail placeholder `gray 38`; histogram
channels `#df3939 / #44b072 / #3b6fe0`; colour-label palette (only if labels are added later) as
LightCraft's `theme::label_color`.

## 3. Typography

Fonts: **Inter** Regular and SemiBold (OFL, `assets/fonts/`, with `OFL-Inter.txt` and an
`ATTRIBUTION.md` row) for everything; **JetBrains Mono** (OFL) only for numeric fields if needed.
CJK fallbacks come from the optional `CRAFT_FONTS_DIR` build input exactly as in LightCraft — never
commit other font files. Install fonts on the first frame and paint nothing until the next frame.

| Use | Face | Size (pt) | Colour |
|---|---|---|---|
| Body, buttons, menu items | Inter | 13 | `text_label` |
| Small (status line, tooltips' secondary line) | Inter | 11 | `text_dim` |
| Dialog / panel titles | Inter SemiBold | 15 | `text` |
| Folder header title on the light table | Inter SemiBold | 15 | `text` |
| Folder header date, count, description | Inter | 12.5 | `text_dim` (description `text_label`) |
| Library tree group header ("Albums", "Folders") | Inter SemiBold | 13 | `text_label` |
| Library tree rows | Inter | 13 | `text_label` (selected `text`); counts 12 `text_dim` |
| Thumbnail caption (filename/date/size) | Inter | 10.5 | `text_dim` |
| Bottom action-button labels | Inter | 11.5 | `text_label` |
| Edit-panel tool button labels | Inter | 12 | `text_label` |
| Slider label and value | Inter | 12.5 | `text_label` |
| Caption field under the edited photo | Inter | 14 | `text` (placeholder `text_dim`) |
| Toast | Inter | 14 | `text` |
| Empty state | SemiBold 18 title + Inter 13 body | | `text_label` / `text_dim` |

egui text styles: `Body 13`, `Button 13`, `Small 11`, `Heading 16 SemiBold`; `item_spacing (6, 4)`,
`button_padding (8, 3)`, `interact_size.y 22`, `animation_time 0.08`.

## 4. Metrics

All in points (egui logical pixels). UI Scale (Options → General) multiplies everything; Auto follows
the OS scale.

| Metric | Value |
|---|---|
| Corner radius: widgets / windows & menus / toasts | 4 / 6 / 6 |
| Corner radius: photos and thumbnails | **0** |
| Menu bar height | 24 |
| Toolbar (below menu bar) height | 40 |
| Room tab strip | 28 (tabs 190 wide) |
| Library tree default / min width | 240 / 200 (max W − 480) |
| Splitter | 8 wide, grip 8 × 48 |
| Library tree row / person row / group header / year divider | 24 / 28 / 30 / 18 |
| Tree indentation per level | 16 |
| Section header block (light table) | ≈ 96 (icon 32, title row, button row 22, description row) |
| Thumbnail cell edge (light table) | 64–320, default 144 (slider); square cells, photo letterboxed |
| Grid gaps / margins | min 12 horizontal (justified), 22 vertical / 24 left & right |
| Light-table scrollbar | 16 wide with ⇑ ▲ … ▼ ⇓ buttons (16 × 24) |
| Side pane | 280 (large: min(30 % W, 392)); header 30 |
| Status line height | 20 |
| Bottom area height | 96; split at 0.365·W (tray left) |
| Photo-tray box | x 6 … 0.365·W − 56, 80 high; Hold/Clear/Album buttons 34 × 24 |
| Star / rotate buttons | 36 × 24 |
| Big action button (icon over label) | 64 × 56, icon 28 |
| Edit panel width (edit view) | 280 fixed; tabs 5 × 40 × 32 |
| Edit tool tile | icon 44 × 30 with label below, 3 columns |
| Effect tile | 86 × 69 (preview 78 × 48 + label), 3 × 4 grid |
| Filmstrip (edit view top bar) | 7–9 cells of 56 × 56 |
| Slider row | 45 (label row 18 + track) |
| Dialog inner margin | 16 × 12 |
| Minimum window size | 900 × 600; default 1400 × 900 |

The values above that are marked Dalica-specific (bottom area, tray, filmstrip at the top, edit
panel) come from the reference layout; tune them only with side-by-side screenshots (see
[`process.md`](process.md) → Visual verification) and record the change here.

## 5. Icons

Original vector icons drawn in code on a 20 × 20 design grid (`crates/ui-kit/src/icons.rs`,
our own drawings, using the LightCraft technique): `paint(&Painter, rect, Icon,
Color32)`, stroke `max(1.5·k, 1.0)` where `k = side / 20`, round caps. No icon fonts, no bitmap
icons. When a glyph is too complex to draw (camera, film reel), use a
[Lucide](https://lucide.dev) SVG (ISC licence) from `assets/icons/` with its licence file and an
`ATTRIBUTION.md` row, tinted at runtime like the drawn icons.

Never trace or copy icons, artwork or bitmaps from the reference application or any other
proprietary product. Imitate *meaning and placement*, not the drawing.

Icon sizes: 16 (tree rows, menus), 18 (toolbar toggles), 20 (edit tools), 28 (big action buttons),
12 (thumbnail badges).

## 6. Widgets

Shared widgets live in `crates/ui-kit/src/widgets.rs`; panels compose them and never re-implement
them. Every widget below registers an automation id and an AccessKit role.

| Widget | Spec |
|---|---|
| `text_button(id, label, active)` | SemiBold 11.5 `text`; `(text_w + 18).max(37) × 24`; radius 4; fill `button` / `hover` / `pressed`; 1 pt `button_border` |
| `primary_button` | as `text_button` but fill `accent`, text white; one per dialog (the default action) |
| `icon_button(id, icon, size, active, enabled, tooltip)` | highlight square `min(32, size)` radius 4: `tool_active` active, `hover × 0.7` hovered; glyph colour `icon` / `text` hover / `pick` active / `text_disabled`; glyph `0.6 × size` |
| `action_button(id, icon, label)` | **Dalica**: big bottom-bar button, 64 × 56 (`ui-layout.md` §6.5), icon 28 centred in the top 34 pt, label below (11.5, `text_label`), hover fill `hover` radius 4, disabled when nothing is selected or in the tray |
| `toggle_filter(id, icon, on)` | 26 × 26 icon toggle for the filter row; on = `tool_active` fill radius 5 + 1 pt `accent` stroke |
| `slider(spec, value)` | LightCraft slider: label left, value right (click to type), 2 pt track, hollow ring thumb r 7 stroked 2 pt `thumb`; drag, Shift = fine ×0.1, double-click resets to the spec default, ↑/↓ nudge; one undo step per drag |
| `zoom_slider(id, value, range)` | thin variant for thumbnail size / edit zoom: 110 × 20, 2 pt `track` line, 3 × 14 thumb in `thumb_hover` |
| `section_header(id, title, open)` | collapsible header, 30 pt, chevron + SemiBold 13 title; used in the library tree groups and dialogs |
| `tabs(id, items, active)` | edit-panel tab strip: icon-only tabs 40 × 32 with tooltips; active tab `chrome` fill joined to the panel, inactive `canvas` |
| `tool_button(id, icon, label)` | edit-panel tool: 124 × 32, icon 20 left, label 12; hover `hover`; active `tool_active` |
| `effect_tile(id, preview_texture, label)` | 86 × 69: a 78 × 48 live preview of the current photo with the effect applied (rendered async, placeholder `gray 38` meanwhile) + label below |
| `stars(id, rating)` / `star_toggle(id, on)` | Dalica stars are a boolean "starred" flag: one star glyph, filled `star` when on, outline `icon` when off; `star_toggle` is a 36 × 24 button plate with a 16 pt glyph |
| `dropdown(id, text)` | borderless text + chevron; opens a popup menu bounded to the window |
| `search_field(id)` | 28 high, radius 4, `field` fill (`canvas` focused), border `field_border` (accent focused), magnifier icon + placeholder `text_dim`, clear (×) button when non-empty; filters live on every keystroke |
| `checkbox`, `radio`, `text_edit`, `combo` | egui defaults under the theme visuals |
| `thumbnail_cell` | see §7 |
| `folder_header` | see §7 |
| `tray_well` | see `ui-layout.md` → Bottom area |
| `toast(text)` | bottom-centre of the light table / canvas, 1.4 s (errors 6 s), radius 6, `black α200` |
| `progress_row(label, fraction, cancel)` | for background jobs in the status line and progress dialogs |
| `empty_state(title, body, action)` | centred SemiBold 18 title + Inter 13 body + optional primary button |
| `dialog(title, body, buttons)` | dim backdrop `black α140`, centred `Window` with margin 16 × 12, buttons right-aligned, laid out right-to-left (avoids auto-size growth); Esc = Cancel, Enter = default |

## 7. Light-table elements

### Thumbnail cell

- Square cell of edge `thumb_size`; the photo is letterboxed to its aspect inside the cell, never
  cropped, never rounded; cell surround is `grid_bg` (no visible cell box at rest).
- Selected: 2 pt outline around the image (white for the focused/anchor photo, `gray 170` for the
  rest of the selection) plus `cell_selected` behind the letterbox.
- Hover: `hover × 0.5` surround; tooltip after 600 ms with file name, dimensions, size, date.
- Caption (optional, View → Thumbnail Caption): one line under the image, 10.5 pt `text_dim`,
  elided in the middle, choices None / Filename / Caption / Date / Resolution / Tags.
- **Badges**, 12 pt glyphs on a `black α130` rounded plate (18 × 18 pt, radius 3, 4 pt in from the
  image's corner, plates 2 pt apart), drawn inside the image:
  - **bottom-right row**, right-aligned, in the order edited (✎, unsaved edit recipe) · geotagged
    (pin) · held in tray (pin-tack) · starred (★ in `star`, always rightmost);
  - **videos:** a film-sprocket strip along the image's left edge and a duration pill "1:23" at its
    bottom-left.
  Badges are always shown, as in the reference behaviour.
- Until the right size arrives a cell draws the next smaller cached size scaled up, or the photo's
  colour sketch (`architecture.md` §6); the placeholder `gray 38` shows only for a photo not yet
  scanned. The outline and badges always draw. A missing or
  unreadable file shows a `caution` "!" centred and a tooltip with the reason.

### Section header (one per folder/album section on the light table)

```
┌──────────────────────────────────────────────────────────────────────────────────┐
│ [📁]  Folder Name                                            3 edited  [Save]      │  SemiBold 15 `text`
│       Thursday, June 4, 2026 · Lisbon                                              │  12.5 `text_dim`
│ [▶][▦] │ [★] │ [💾] [Export ▾]                                                     │  26 × 22 icon buttons
│ Add a description                                                                  │  12.5, click to edit inline
└──────────────────────────────────────────────────────────────────────────────────┘
```

Fill `folder_header`, full light-table width, radius 0, 12 pt left padding. Full behaviour in
[`ui-layout.md`](ui-layout.md) §5.2. The header of the selected section has a 2 pt `accent` bar at
its left edge. Headers are **sticky** (collapsed to their first line while their photos scroll
underneath, pushed up by the next header).

## 8. Motion and feedback

- `animation_time` 0.08 s for hover/press; scroll-to-folder animates 0.2 s ease-out; zoom-to-fit /
  1:1 in the edit view animates 0.22 s (LightCraft `animated_rect`).
- Every user action that has no visible result in place shows a toast ("Starred 12 photos",
  "Moved 3 photos to Holidays").
- Long work never blocks: progress in the status line with a Cancel affordance; modal progress
  dialogs only for operations the user must wait for (export, save, collage render).

## 9. Accessibility and scaling

- AccessKit roles and labels on every custom widget; keyboard focus ring = 1.5 pt `accent`.
- UI Scale (Auto, 75–300 %) and UI Font Size (Small/Medium/Large) as in PhotoCraft
  (`docs/ui-design.md` there).
- Minimum contrast: `text_label` on `chrome` and `text_dim` on `grid_bg` must stay ≥ 4.5:1; check
  with the contrast test in `dalica-ui-kit` (`theme.rs`) when tokens change.

## 10. Verification

Visual changes are not done until they have been looked at: render with the headless snapshot
(`dalica-cli snapshot …`) or the running app's `ui.screenshot`, at 1400 × 900 and 900 × 600, and
compare against the reference layout notes (see [`process.md`](process.md)). Attach before/after
screenshots to PRs.

## 11. Window chrome, menus, preferences and scaling

Adapted from PhotoCraft `docs/ui-design.md` (revision 59684266) and LightCraft `crates/ui-egui/src/menu_level.rs` (revision 88ea1a8); MIT OR Apache-2.0.

### 11.1 Title bar

- **Windows and Linux (default):** the window has no OS decorations, so there is one bar, not a
  title bar stacked over a menu bar. The 24 pt menu-bar row (`ui-layout.md` §2) *is* the title bar
  (`crates/ui/src/titlebar.rs`): the app mark (16 pt, drawn in code), the menus, the window title
  centred in the free space (`text_dim`, "Dalica" or "<folder or album> — Dalica"), then Minimize,
  Maximize/Restore and Close flush in the top-right corner (46 × 24 each; hover `hover`, Close hover
  `danger` with a white glyph). The free gap between the menus and the caption buttons drags the
  window, a double-click there maximizes/restores it, and invisible 5 pt edges (12 pt corners)
  resize it. Close runs **File → Exit**, so pending writes and running jobs are handled first.
- **System title bar option:** Options → General → Interface → **Use system title bar** (Windows
  and Linux only, default off) gives the window the OS decorations and draws the menu bar below
  them. It shows "Applies at next launch." beneath the checkbox (before and after Apply); saving it
  never restarts the app or changes the current window. Linux desktops where client-side decorations
  misbehave can use it as the fix.
- **Until M3 builds the title bar,** the window keeps the OS decorations on every platform (decided
  at the M0 review, 2026-10-09). On GNOME (Wayland, no server-side decorations) winit then draws a
  plain bar without the title text; that is accepted until M3.
- **macOS:** native traffic lights over an integrated title strip; the menus live in the system menu
  bar (generated by `muda` from the same menu model), so the in-window menu row is not drawn.
- **Narrow windows** drop chrome before anything overlaps, in this order: the window title in the
  title bar, then the toolbar's filter caption (`ui-layout.md` §11), then the search field shrinks
  to a 160 pt minimum. Only text that repeats information shown elsewhere may disappear; menus,
  filter toggles and caption buttons never do. Check at 900 × 600 (the minimum window size).

### 11.2 Menus never run off the window

- Every menu, submenu, context menu and dropdown is bounded by the window's content rect. A menu
  that would cross an edge opens on the other side (submenus to the left; menus near the bottom
  move up), and one taller than the window scrolls: arrow rows at the top and bottom (hover or click
  to scroll, wheel too), the highlighted item kept visible during keyboard navigation (the LightCraft
  `menu_level.rs` technique).
- Long generated menus (the people list in **Move to Person ▸**, album and tag lists) use the same
  scrolling container.
- Disabled items are shown greyed, never hidden (`menus-and-shortcuts.md`). Check new menus with a
  headless snapshot at 900 × 600.

### 11.3 Options dialog: Apply, OK, Cancel

Tools → Options… (macOS Dalica → Preferences…, `app.preferences`) has **OK**, **Cancel** and
**Apply** (`menus-and-shortcuts.md` §5):

- The dialog edits a draft copy of the settings. **Apply** saves the edited values and keeps the
  dialog open; it is disabled while the draft matches the saved settings. **OK** applies (if needed)
  and closes. **Cancel** (and Esc) discards only the changes made since the last successful Apply.
- A failed Apply (for example the settings file could not be written) shows the error in the
  dialog, leaves the draft open for correction and changes nothing.
- Most options take effect on Apply. UI language and UI font size preview live while the dialog is
  open and revert on Cancel. Options that need a restart (Use system title bar) say "Applies at next
  launch." and are never applied to the running window.
- Settings are written atomically like every other file (`data-model.md` §4); a damaged settings
  file is kept as `.corrupt-<time>` and the defaults are used.

### 11.4 UI Scale and high-DPI displays

Options → General → Interface → **UI scale** applies at once (on Apply) and multiplies every point
size in this document; photo pixels and the edit-view zoom are independent of it.

- **Auto** (default) follows the operating system's display scale, including fractional scales
  (125 %, 150 %, 175 %; on Linux, Wayland's fractional scale or X11's `Xft.dpi`).
- **4K rule:** only when the system reports no scaling (100 %) on a monitor of 3840 px or more on its
  long side does Auto use 200 %, so text stays readable. Detection uses the window's current monitor
  (portrait monitors included) and updates when the window moves to another monitor; if the monitor
  size is unknown, Auto follows the system DPI.
- **Fixed choices** 75 %–300 % (in 25 % steps) set an absolute scale, so a large 4K display can use
  smaller controls on purpose.
- Thumbnail decode sizes follow physical pixels (cell edge × scale), so thumbnails stay sharp at
  every scale.

### 11.5 UI Font Size

Options → General → Interface → **UI font size** changes interface text independently of UI Scale:
**Small** (default) uses the sizes in §3, **Medium** multiplies them by 15/13 and **Large** by 17/13
(body text 13 → 15 → 17 pt), preserving the relative sizes of titles, captions and small text.
Control heights grow with the text where a label would otherwise be clipped. The choice previews live
in the dialog, persists across launches, and applies to the CJK fallback fonts too, including fonts
loaded after the change. Photo captions and watermarks drawn into output (collages, exports) are never
affected.
