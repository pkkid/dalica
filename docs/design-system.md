# Design system

Dalica's **layout and interaction model** follow the reference photo organizer described in
[`ui-layout.md`](ui-layout.md). Its **look** does not: Dalica uses the muted, dark, neutral-grey
design language of the Craft apps ([LightCraft](https://github.com/storytold/lightcraft) and
[PhotoCraft](https://github.com/storytold/photocraft)), so it sits naturally beside them. When this
document and a Craft app disagree on a value, this document wins for Dalica; when this document is
silent, match LightCraft's look (study `crates/ui-egui/src/theme.rs`, `widgets.rs`, `icons.rs` at
revision 88ea1a8) and record the value here. Their theme, widget and icon code may be reused
with attribution (`architecture.md` §9).

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

Dark theme (the only theme until after 1.0; switchable themes, light ones included, are planned
for M16 in `roadmap.md`, and this dark theme stays the default).

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
| `text_dim` | `#949494` | counts, captions under thumbnails, hints, placeholders (4.5:1 on `chrome`) |
| `text_disabled` | `#5c5c5c` | disabled controls |
| `icon` | `#9a9a9a` | icon glyphs at rest (hover → `text`, active → `pick`) |
| `track` | `#5a5a5a` | slider tracks, the thumbnail-size slider |
| `thumb` | `#a0a0a0` | slider ring at rest |
| `thumb_hover` | `#e0e0e0` | slider ring hovered / dragged |
| `accent` | `#0165dd` | focus border, text selection (`accent × 0.6` fill), active filter, drop target outline |
| `accent_hover` | `#1a74e0` | primary button hovered (`accent` 10 % towards white) |
| `accent_pressed` | `#0156bc` | primary button pressed (`accent × 0.85`) |
| `on_accent` | `#ffffff` | text on an `accent` fill (the primary button) |
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
| `crosshair` / `crosshair_outer` | `#f0f0f0` over a wider `black α160` edge | the focal effects' crosshair on the photo (M12.4) |
| `caption_close` | `#c42b1c` | the title bar's Close button hovered (Windows' own red, as PhotoCraft's `caption_close`), white glyph |

Other fixed values, each a `Tokens` field too: dialog `backdrop` `black α140`; `popup_shadow`
offset (0,4) blur 16 `black α140`; `window_shadow` (0,8) blur 30 `black α160`; `toast_bg`
`black α200`; slideshow and photo-viewer background (`viewer_bg`) pure black (`viewer_overlay`
`black α210` over the desktop/app); the slideshow's and viewer's control bar `overlay_bar` `black
α170` (radius `overlay_radius` 10) and the slideshow caption's `caption_plate` `black α160`, with
their light-on-dark colours `overlay_text` white, `overlay_icon` `#c8c8c8` (a glyph at rest),
`overlay_disabled` `#6a6a6a`, `overlay_hover` `white α28` and `overlay_pressed` `white α48`
(M11.3; text 4.5:1 and glyphs 3:1 over a white photo); the import tray's `excluded_dim` `black
α150` over an excluded cell; thumbnail `placeholder` `gray 38`; photo selection outlines
`photo_focus` white and `photo_selected` `gray 170`; histogram channels `#df3939 / #44b072 /
#3b6fe0` (`theme::HISTOGRAM`); colour-label palette (only if labels are added later) as
LightCraft's `theme::label_color`.

The §3 sizes are `Tokens` fields (`body`, `small`, `title`, `heading`, `button_label`, `caption`,
`detail`, `count`, `large`, `empty_title`) and the §4 metrics are `Tokens::metrics`
(`theme::Metrics`), so the UI Font Size and the tests read one source.

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
| Menu bar height (the title bar on Windows and Linux, its last point a 1 pt divider; §11.1) | 32 |
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

Sets beside the shell's icons (`Icon`), each with its own sheet and fingerprint so adding one never
moves another's: the Folder Manager's `FolderIcon`, the edit view's `EditIcon` (M7.6) and
`OutputIcon` (M8.5, `icons/output.rs`): **Save**, a disk with its top-right corner cut, the shutter
at the top and the label below (the folder header's Save button), and **Project**, a briefcase
(the case, its handle, the band round it and a clasp; the tree's project rows: export and collage
folders), and `AlbumIcon` (M9.5, `icons/albums.rs`; `paint_album`), the automatic albums of the
tree's Albums group (`organizing.md` §4.1), each the user album's book (outline and spine) with an
emblem on its cover in place of the picture: **Starred** (`starred-album`, a filled star: Starred
Photos), **Recent** (`recent-album`, a clock with its hands at three: Recently Updated) and
**Search** (`search-album`, a magnifier: the temporary Search results album), and `OptionsIcon`
(M10.3, `icons/options.rs`; `paint_options`), the shortcut editor's (`menus-and-shortcuts.md`
§2.0): **Restore** (`restore-default`, ↺: three quarters of a circle from its left over the top
to its bottom, the arrow at its left end pointing down, a dot at its centre for the default it
goes back to; drawn apart from the shell's Rotate Counterclockwise) and **Clear** (`clear-key`, a
× on a key cap pointing left, the Backspace key that also removes a key while recording; drawn
apart from the shell's Close and Clear Search), and `ViewingIcon` (M11.3, `icons/viewing.rs`;
`paint_viewing`), the rooms', slideshow's, photo viewer's, import tray's and compare's: **Library**
(four thumbnails two by two: the Library room tab), **Exit** (an arrow leaving through a door frame
on its right), **Pause** (two bars), **Previous** and **Next** (a solid triangle against a bar,
apart from the filmstrip's bare triangles), **Transition** (a frame wiped half across by a
diagonal, the upper-left half solid), **Captions** (two lines of text low in a screen), **Clock**
(a clock face, hands at three: the import tray's select-this-group button), **Exclude** (a picture
with a "none" sign over its corner, apart from the tray's bare Clear sign), **Actual size** ("1:1"),
**Open in Dalica** (an arrow coming into a frame from its top-right), **Switch Focus** (⇄),
**Switch Layout** (⇅) and **Show Original** (a frame whose left half is solid). Meanings the kit
already draws are not drawn twice: Import is the shell's camera, More its three dots, Close its
cross and Play the edit view's. `EffectIcon` (M12.4, `icons/effects.rs`; `paint_effect`), the
effect tabs' (`editing.md` §8): **Brush** (`brush`, a paint brush from its handle at the top right
to its bristles' tip at the bottom left: Soften's brush), **Eraser** (`eraser`, a block tilted up
to the right, its lower third solid, over a short line), **Crosshair** (`crosshair`, a ring
crossed by four ticks round a dot: the focal effects) and **1-click** (`one-click`, a pointer's
arrow with three short rays at its tip: the tiles' one-click badge); the colour picker's
eyedropper is the edit view's. `Glyph` names a glyph of any set (`paint_glyph`) for the widgets
whose callers mix sets (room tabs, overlay buttons). Each set's sheet
shows every glyph at 12, 16, 20 and 28 in the rest, hover and disabled colours, and a test keeps
every glyph inside its square and apart from its neighbours.

## 6. Widgets

Shared widgets live in `crates/ui-kit/src/widgets.rs`; panels compose them and never re-implement
them. Every widget below registers an automation id and an AccessKit role.

| Widget | Spec |
|---|---|
| `text_button(id, label, active)` | SemiBold 11.5 `text`; `(text_w + 18).max(37) × 24`; radius 4; fill `button` / `hover` / `pressed`; 1 pt `button_border` |
| `primary_button(id, label)` | as `text_button` but fill `accent` (`accent_hover` hovered, `accent_pressed` held), text `on_accent`, no border; one per dialog (the default action). Disabled it takes the plain button look with `text_disabled` text |
| `segmented(segments, active, size)` | **Dalica**: joined segments (each its own id, text or icon, tooltip), one `button_border` outline with 1 pt dividers, rounded at the ends; selected `tool_active` with a `pick` glyph, others `button` / `hover` / `pressed`; registered as toggles. The toolbar's Flat \| Tree pair is two 30 × 28 icon segments |
| `icon_button(id, icon, size, active, enabled, tooltip)` | highlight square `min(32, size)` radius 4: `tool_active` active, `hover × 0.7` hovered; glyph colour `icon` / `text` hover / `pick` active / `text_disabled`; glyph `0.6 × size` |
| `action_button(id, icon, label, active, enabled)` | **Dalica**: big bottom-bar button, 64 × 56 (`ui-layout.md` §6.5), icon 28 centred in the top 34 pt, label below (11.5, `text_label`, elided), hover fill `hover` radius 4 with `text` glyph and label, `pressed` fill and `pick` glyph while held or `active` (More… menu open), disabled when nothing is selected or in the tray |
| `toggle_filter(id, icon, on, tooltip)` | 26 × 26 icon toggle for the filter row, 16 pt glyph; on = `tool_active` fill radius 5 + 1 pt `accent` stroke and `pick` glyph; off = no fill, `hover × 0.7` hovered, `pressed` held |
| `slider(spec, value)` | LightCraft slider, 45 pt row: label left, value right (12.5, click to type: Return or clicking away applies, Esc keeps the old value; the field has the focused-field frame), 2 pt track, hollow ring thumb r 7 stroked 2 pt `thumb` (`thumb_hover` hovered or dragged); drag, Shift = fine ×0.1, a click jumps, double-click resets to the spec default, ↑/↓ nudge (about 1/200 of the range, Shift ×5) while the pointer is on the row or it has the focus; one undo step per drag (`SliderOut::drag_started` / `drag_stopped`). `SliderSpec` is the control spec (id, label, range, default, step, decimals, unit); hostile specs (reversed or infinite ranges, NaN) are ordered and clamped. Registers `slider:<name>` and `slider:<name>/value` |
| `tuning_slider(spec, value)` | **Dalica** (M7.5): the Tuning tab's variant of `slider` (`editing.md` §7): the same 45 pt row, track, ring, drag, click and double-click reset, but no value on the label row; the value (formatted by the spec) shows as a tooltip at once while the pointer is on the track or it is dragged; **+** / **−** (also `=`) nudge it by 2 % of the range (`TUNING_NUDGE`, put on the spec's steps) while the pointer is on the row or it has the focus, ↑ / ↓ still nudge finely; each nudge is one undo step. Registers `slider:<name>` only |
| `zoom_slider(id, value, range, tooltip)` | thin variant for thumbnail size / edit zoom: 110 × 20 (the bottom bar's thumbnail-size slider is 128 wide, `ui-layout.md` §6.4), 2 pt `track` line (`thumb` hovered), 3 × 14 thumb in `thumb_hover` (`pick` dragged); click or drag sets, ←/→ with the focus move a twentieth of the range |
| `age_slider(id, days, width, tooltip)` | **Dalica** (M9.5, `widgets/age_slider.rs`; `organizing.md` §8.2): `zoom_slider`'s thin look, `width` × 20 (the toolbar's is 100 wide); the age is whole days back from today, `None` for every photo. The handle's place (0 … 1) maps to the age on a logarithmic scale, both ways pure functions shared with the UI and the tests: `age_days(position)` is `None` at 0, 20 years (`AGE_MAX_DAYS`, 7305 days) just right of it, down to 1 day at 1, never larger further right; `age_position(days)` is its inverse. An age is always on its label's unit (`age_snapped`: 1 … 6 days, whole weeks below 30 days, whole months below a year, what would read as 12 months a year, whole years up to 20), so the filter is exactly what the label says; `age_label(days)` reads "Pictures up to 1 day old", "… 2 weeks …", "… 3 months …", "… 5 years old", "All pictures" for `None`. Drag or click sets it, ←/→ with the focus move the handle a twentieth of the way; the hover text and the accessible label are the age's label while one is set, else `tooltip` ("Show only recent photos"). Registers `slider:age` (role `other`, active while an age is set) |
| `filter_banner(id, show_all_id, text)` | **Dalica** (M9.5, `widgets/banner.rs`; `ui-layout.md` §9): the full width of the light table, 32 high (`BANNER_H`), `filter_banner` fill, no radius; a `text_button` **Show All** 8 pt from its left (hover text "Leave search"), always whole; the text 12 pt after it in 13 pt `text` on one line, cut from its end with "…" 12 pt short of the right edge (`BannerOut` reports the text as painted and whether it was cut). Registers `banner:search` (role `other`, labelled with the whole text) and `button:search.showAll` |
| `drag_pill(id, pointer, pill)` | **Dalica** (M9.5, `widgets/drag_pill.rs`; `ui-layout.md` §5.5): painted above everything while photos are dragged; the popup frame (`chrome`, 1 pt `button_border`, radius 6, popup shadow), 32 high, at most 360 wide; the first photo's thumbnail letterboxed in a 24 pt square 4 pt from its left (square corners; `placeholder` until it has one), for a copy an `on_accent` + on a 6 pt `accent` disc at the thumbnail's bottom-right corner; then `drag_pill_text` in 13 pt `text`, cut from its end with "…": "Move 3 photos to Garden", "Copy 3 photos to Garden", "Add 3 photos to Holidays" ("… to album" without a name), "Hold 3 photos in the tray", "Reorder 3 photos", or only "3 photos" over nothing that takes them (counts grouped by thousands). Placed by `drag_pill_rect`: 16 pt right of and below the pointer, on its left where it would pass the window's right edge, above it where it would pass the bottom, always 4 pt inside the window. Registers `drag:pill` (role `other`, painted, labelled with the text) |
| `drop_target(rect)` | **Dalica** (M9.5): the `drop_target` highlight, one painter for the tree's folder and album rows and the tray while a drag hovers them: a `drop_target_fill` plate with a 1.5 pt `drop_target` outline inside the rect, radius 4 |
| `insertion_marker(top, height)` | **Dalica** (M9.5): where dragged photos land between two cells of an album: a 2 pt `accent` bar the height of a cell in the gap, with an 8 pt cap across each end (an I-beam) |
| `section_header(id, title, open)` | collapsible header, 30 pt, 12 pt chevron (right closed, down open) + SemiBold 13 title in `text_label` (`text` hovered); no fill at rest, `hover × 0.5` hovered; used in the library tree groups and dialogs; registered as a toggle whose state is `open` |
| `tabs(id, items, active)` | edit-panel tab strip (M7.5): the full width, 32 high, `canvas` behind; icon-only tabs 40 × 32 (`metrics.edit_tab`) with an 18 pt glyph and a tooltip each; the active tab `chrome` with its top corners rounded and no bottom edge, joined to the panel below; the others `hover × 0.7` hovered and `pressed` held (inset 2 × 3, radius 4); glyph `icon` / `text` hovered / `pick` active / `text_disabled`. Too narrow, the tabs shrink to 24 each, then the ones that do not fit go to a menu behind an 18 pt chevron button `<id>/more` at the end of the strip; the active tab always stays on the strip (fitting ported from craft-ui). Tabs register as `<id>/<name>` (role `tab`, active when shown), in the menu too; the accessible role is a tab, selected when shown |
| `text_tabs(id, items, active)` / `tab_page(rect)` | **Dalica** (M10.3, `widgets/tabs.rs`; the Options dialog, §11.3): a dialog's tab strip, the full width × 28 (`room_tabs_h`; taller when the font needs it) on the dialog's `chrome`; each tab its label (13 pt) with 14 pt either side (`TEXT_TAB_PAD`); the open tab filled `inset` with a 1 pt `button_border` outline round its top and sides (top corners radius 4) and its label `text`, joined to the page below: the strip's bottom edge is the page's top line (1 pt `button_border`), open under the open tab; the others `text_label`, no fill, `hover × 0.7` hovered and `pressed` held (inset 2 × 3, radius 4), label `text` hovered; disabled `text_disabled`. Too narrow, the widest tabs shrink first to a common width (`fit`, as the icon tabs), their labels cut from their end with "…"; below 56 each (`TEXT_TAB_MIN_WIDTH`) the tabs that do not fit go to the `<id>/more` menu and the open tab stays on the strip. `TextTabsOut` gives the strip's rect and each tab's label as painted. Tabs register as `<id>/<name>` (role `tab`, active when open, labelled with the whole label). `tab_page` paints the page under the strip: `inset`, a 1 pt `button_border` outline round its sides and bottom, bottom corners radius 4. The Options dialog (720 wide) holds its four tabs whole at every font size; the edit panel's icon `tabs` are unchanged |
| `room_tabs(id, items, active)` | **Dalica** (M11.3, `widgets/tabs.rs`; `ui-layout.md` §8): the rooms' strip between the menu bar and the content, the full width × 28 (`room_tabs_h`) on `canvas`; each tab 190 wide (`room_tab_w`): its 16 pt glyph 10 pt in (`ROOM_TAB_PAD`), its label (13 pt, cut from its end with "…") 6 pt after it and, on a closable tab (every room but Library), a 16 pt close button (`ROOM_TAB_CLOSE`, a 10 pt ×) 6 pt from its right edge: `text_dim` (`icon` on the active tab), `text` on a `hover` plate (radius 3) hovered, `pressed` held. The active tab is `chrome` with its top corners rounded and no bottom edge, joined to the content below, glyph `pick` and label `text`; the others no fill, `hover × 0.7` hovered and `pressed` held (inset 2 × 3, radius 4), glyph `icon` and label `text_label` (`text` hovered); disabled `text_disabled`. Too narrow, the tabs shrink to a common width (`fit`), their labels cut; below 96 each (`ROOM_TAB_MIN_WIDTH`) the tabs that do not fit go to the `<id>/more` menu and the active tab stays on the strip. `RoomTabsOut` reports the tab clicked, the tab whose × was clicked (that click does not open it; closing a room with unsaved work asks first, the caller's part) and each tab as painted. Tabs register as `<id>/<name>` (role `tab`, active when shown, labelled with the whole label), close buttons as `<id>/<name>/close` (role `button`, "Close Import") |
| `overlay_bar(id, rect, opacity, contents)` / `overlay_button(id, glyph, label, active, enabled, tooltip)` | **Dalica** (M11.3, `widgets/overlay_bar.rs`; `viewing.md` §1.3, §3.1): the slideshow's and the viewer's control bar: an `overlay_bar` panel (`black α170`, radius 10) at `rect` (`overlay_bar_rect`: centred along the bottom of its area, 24 pt above it, never larger than it), its controls left to right 12 × 8 in (`OVERLAY_PAD`), centred vertically, 6 pt apart. Panel and controls are drawn at `opacity`; at 0 nothing is drawn, registered or clickable. The panel takes the clicks between its controls, so a click on the bar never reaches the photo. `OverlayFade` turns shown or hidden into that opacity on the clock it is given (the show's, a test's): a whole fade takes 0.2 s (`OVERLAY_FADE_SECS`), one reversed part-way starts from where it was, at the same rate; when to show the bar is the caller's. An `overlay_button` is 32 high (`OVERLAY_BUTTON`), square without a label, else its 18 pt glyph 10 pt in and its label (13 `overlay_text`) 6 pt after it; radius 4, no fill at rest, `overlay_hover` hovered, `overlay_pressed` held or active; glyph `overlay_icon`, `overlay_text` hovered or active, `overlay_disabled` disabled. Registers the bar as `<id>` (role `other`, "Controls") and each button (role `iconButton`, active when active) |
| `stepper(id, value, range, text)` | **Dalica** (M11.3, `widgets/stepper.rs`; the slideshow's display time, `viewing.md` §1.3): `[−] 3 seconds [+]` on an overlay: 22 pt square buttons (`STEPPER_BUTTON`, radius 4, `overlay_hover`, `overlay_pressed` hovered or held, their sign `overlay_text`) and the value (13 `overlay_text`) centred in the width of the widest of the range's ends' texts, 8 pt from each, so the bar never moves as it changes. One step per click; at an end of the range (or disabled) that button has no plate, an `overlay_disabled` sign and takes no clicks; a value outside the range comes in at its end (`stepped`: saturating, a reversed range turned round). Registers `<id>/minus`, `<id>/plus` (role `button`) and `<id>` (role `other`, painted, the value's text) |
| `caption_plate(id, area, bottom, text)` | **Dalica** (M11.3, `widgets/caption_plate.rs`; `viewing.md` §1.2): the slideshow caption: 22 pt Inter (`CAPTION_PLATE_SIZE`, fixed whatever the UI Font Size) in `overlay_text`, centred lines at most 70 % of the area's width (`CAPTION_PLATE_WIDTH`) less the padding, wrapped at word breaks to two lines (`CAPTION_PLATE_LINES`) and then cut short with "…", on a `caption_plate` (`black α160`) with 14 × 6 padding and radius 6, centred across the area, its bottom `bottom` above the area's bottom. A blank caption shows nothing. `CaptionPlateOut` gives the plate and the lines as painted. Registers `<id>` (role `other`, painted, the whole caption) |
| `group_header(id, title, detail, all_selected)` | **Dalica** (M11.3, `widgets/group_header.rs`; the import tray, `organizing.md` §3): one per gap of more than two hours in the capture times: the full width × 30 (`group_header_h`), no fill; a 24 pt clock button (`GROUP_CLOCK`, radius 4, the 16 pt Clock glyph) 4 pt in that selects the group: glyph `icon`, `text` on `hover × 0.7` hovered, `pressed` held, `pick` on `tool_active` while the whole group is selected; the title (SemiBold 13 `text_label`, the times) 6 pt after it and the detail (12 `text_dim`, the count) 10 pt after that, on one line, cut with "…"; disabled `text_disabled`. Registers `<id>` (role `other`, painted, the title) and `<id>/select` (role `button`, active while the group is selected, "Select this group") |
| `excluded_mark(cell, image)` | **Dalica** (M11.3, `widgets/badges.rs`; the import tray, `organizing.md` §3): a duplicate or excluded picture: its cell (photo included) dimmed with `excluded_dim` (`black α150`) and a `danger` ⊘ centred on the photo, a circle 40 % of its shorter side across (16–48 pt) with a stroke a tenth of that (at least 2 pt) and a slash from bottom-left to top-right. Registers nothing (the cell gives its parts their ids) |
| `checkbox_row(id, label, on, hint, disabled)` | **Dalica** (M10.3, `widgets/checkbox.rs`): the settings dialogs' checkbox on `Form` rows, in place of egui's own: M8.5's `checkbox` with its `hint` line under it ("Applies at next launch."), 11 `text_dim`, starting under the label (22 pt in); with `disabled` (the reason, "Arrives with Import") the row is drawn disabled (box, mark, label and hint `text_disabled`), ignores clicks and keys, registers as disabled and shows the reason as its tooltip; its value never changes. An enabled row has no tooltip |
| `key_cell(id, keys, mac, recording, conflict, width)` | **Dalica** (M10.3, `widgets/key_cell.rs`; the shortcut editor, `menus-and-shortcuts.md` §2.0): `width` (160, `KEY_CELL_W`) × 24 (taller when the font needs it), the whole cell clickable; one or two keys (a command's keys in one context; more are not shown) as key caps: 12 pt `text` on a `button` plate, 1 pt `button_border`, radius 3, 6 pt either side of the text, 4 pt apart, a cap too wide cut with "…"; no key: "—" in `text_dim`. `key_text` writes a key as the platform does: `Ctrl+Shift+S` on Windows and Linux (modifiers Ctrl, Alt, Shift; Cmd reads as Ctrl, Option as Alt), `⇧⌘S` on macOS (⌥ ⇧ ⌘ in Apple's order, then the key; Enter ↩, Esc ⎋, Tab ⇥, Backspace ⌫, Delete ⌦, the arrows, PageUp ⇞, PageDown ⇟, Home ↖, End ↘; every glyph is in Inter). Hovered `hover × 0.7`, held `pressed`; recording: `field` with a 1.5 pt `accent` outline and "Press the new shortcut…" in `text`; a conflict: a 1.5 pt `danger` outline round the cell, the caps still shown; disabled (a hold gesture, listed but not editable) caps' text `text_disabled`, no clicks. The caller starts and ends recording on a click. Registered as `id` (role `button`, active while recording, labelled "Ctrl+8, F4", "Press the new shortcut…" or "No shortcut") |
| `list_row(id, label, widths, selected, modified, cells)` / `list_text(cell, text, dim)` | **Dalica** (M10.3, `widgets/list_row.rs`; the shortcut editor's list): the full width × 24 (`LIST_ROW_H`; taller when the font needs it), the whole row clickable; rest no fill, hovered `hover`, selected `pressed` (radius 4), text `text_label` (`text` hovered or selected, `text_disabled` disabled); a modified row has a 6 pt `text` dot in the 14 pt gutter at its left (`LIST_ROW_GUTTER`) and its first column SemiBold. The columns come from `widths` (`list_columns`: after the gutter, one after another; a width that is not a finite positive number takes what is left), the same for every row of a list, so they line up whatever the text or the font size; each cell gets a ui confined to its column less 8 pt (`LIST_COLUMN_GAP`) and may hold text (`list_text`: 13 pt on one line, cut with "…", `text_dim` for a secondary column at rest) or widgets of its own (key cells, a ↺ button), which take their own clicks. Registered as `id` (role `other`, active when selected) |
| `tool_button(id, icon, label, active, enabled, width, tooltip)` | edit-panel tool, the Basic Fixes grid (`editing.md` §6, M7.5): `width` (three to a row: `tool_button_width`) × 52; the 44 × 30 icon tile (`metrics.tool_tile`, `inset`, radius 4) 3 pt from the top with the 20 pt glyph, the label below (12 pt, elided); rest no fill, `icon` glyph, `text_label` label; hovered `hover` fill, `text` glyph and label; held `pressed`; active (its tool open) `tool_active` fill and a `pick` glyph; disabled both `text_disabled`. Registers `tool:<name>` (role `tool`) |
| `histogram(id, rect, bins)` | **Dalica** (M7.5, `editing.md` §5.2): the bins the caller gives (red, green, blue; up to 4096 each) on a `canvas` plate: translucent areas (`theme::histogram_fill`: the `HISTOGRAM` colours of the overlapping channels added together, at α 0.55), exact where they overlap whatever the paint order, and a 1 pt outline per channel in its colour; smoothed for display (a five-bin kernel, edge bins kept); one scale for the three channels, 4 × the mean of the interior bins that hold anything, never above 1.1 × the tallest, so tall spikes clip (an all-black or all-white photo scales by its edges). Mesh painting ported from PhotoCraft. Registers `histogram:<name>` (role `histogram`, painted, no input) |
| `effect_tile(tile)` / `effect_grid(tiles)` | **Dalica** (M12.4, `widgets/effect_tile.rs`; `editing.md` §8.1): 86 × 69 (`metrics.effect_tile`): the 78 × 48 preview (`metrics.effect_preview`, square corners, the texture filling it: the engine renders tiles at that shape) 3 pt from the top, `placeholder` (`gray 38`) until it arrives; the label under it, centred, in SemiBold 11, or, when it does not fit the tile's width less 2 pt (84 pt), stepped down by half points until it does but to no less than 9 pt (`effect_tile_label`, `EFFECT_LABEL_MIN`; "Widescreen Film" draws at 10), and only a label too wide even at 9 pt is cut with "…"; a one-click effect's badge: a 16 pt `badge_plate` square (radius 3) 2 pt inside the preview's top-right corner holding the 12 pt 1-click glyph in `text`. Rest no fill, label `text_label`; hovered `hover` fill (radius 4), label `text`; held `pressed`; disabled the preview dimmed by `excluded_dim`, label and badge glyph `text_disabled`; tooltip the label over the effect's description (shown disabled too). `effect_grid` lays a tab's tiles in reading order three to a row and at most four rows (`EFFECT_GRID_COLUMNS`, `EFFECT_GRID_ROWS`), 4 pt apart (`EFFECT_TILE_GAP`): 266 × 288 for twelve, centred in the panel; it reports the tile clicked. Registers `effect:<name>` (role `tool`) |
| `effect_slider(spec, value, scale)` | **Dalica** (M12.4, `widgets/slider.rs`; `editing.md` §8.1): the `tuning_slider` (45 pt row, no value on the label row, the value as a tooltip, drag, click, Shift = fine, double-click resets) with its label centred above the track, on a `SliderScale`: `Linear`, or `Log` (Glow's Radius), where equal distances along the track multiply the value (a range starting at or below 0 maps as `ln(1 + v − min)`); `scale_fraction` / `scale_value` are the two directions, pure and shared with the tests. **+** / **−** move it 2 % of the track and ↑ / ↓ a quarter of that (Shift × 5), on the spec's steps (at least one step: `scale_nudged`). Registers `slider:effect.<param>` |
| `swatch(id, label, color, armed)` | **Dalica** (M12.4, `widgets/swatch.rs`; `editing.md` §8.1; placement and picker ported from PhotoCraft): a 36 × 22 colour button (radius 3; 1 pt `field_border`, `text_label` hovered, 1.5 pt `accent` while its popup is open or its eyedropper armed; disabled the colour at 40 % with a `text_disabled` border). A click opens or closes its popup (194 × 202; the popup frame: `chrome`, 1 pt `button_border`, radius 6, popup shadow) 4 pt below the swatch, above it without room below, its left edge at the swatch's or pulled in to stay 4 pt inside the window (`swatch_popup_rect`). Inside, 10 pt in: the 150 pt saturation / value field at the colour's hue (saturation across, value down; its marker a 5 pt `crosshair` ring on a `crosshair_outer` one) and 8 pt right of it the 16 pt hue strip (360° at the top down to 0°; two `text` triangles mark the hue); under them the hex field (`text_field`, 84 wide, `#rrggbb`; Return or clicking away takes a valid colour, anything else gives back the old one) and, at the right, the 24 pt eyedropper button (the edit view's eyedropper glyph: `icon`, `text` on `hover`, `pick` on `tool_active` while armed). A click or drag in the field or the strip sets the colour; the picker keeps the colour as HSV so a hue survives black, white and greys. A click outside the popup and the swatch closes it, except while the eyedropper is armed (that click samples the photo; the caller disarms); Esc closes it when the hex field is not being typed in. Registers `color:effect.<param>` (role `button`, active while open) and, while open, `<id>/popup`, `<id>/field`, `<id>/hue`, `<id>/hex` and `<id>/eyedropper` (role `toggle`, active while armed) |
| `crosshair(id, rect, crosshair)` | **Dalica** (M12.4, `widgets/crosshair.rs`; `editing.md` §8.1): over the photo as shown (`rect`): a cross of four arms 10 pt long round a 3.5 pt ring at the centre, a circle of the given radius (a share of `rect`'s shorter side) when it has one, and a second handle (a 5 pt disc joined to the centre by a line; Graduated Tint's start and angle) when it has one; every stroke `crosshair` over a wider `crosshair_outer` edge, the part under the pointer or dragged `accent`. The centre (a 24 pt grab square) and the handle drag with the pointer, keeping where they were grabbed, and stay on `rect`; `CrosshairOut` reports their new places in the crosshair's own normalised coordinates ((0, 0) top-left, (1, 1) bottom-right of `rect`) and the drag's start and end (one undo step per drag); `handle_angle` gives the line's angle as shown. Registers `crosshair:effect` on the centre and `crosshair:effect/handle` (role `other`, active while dragged) |
| `stars(id, rating)` / `star_toggle(id, on)` | Dalica stars are a boolean "starred" flag: one star glyph, filled `star` when on, outline `icon` when off; `star_toggle` is a 36 × 24 button plate with a 16 pt glyph |
| `dropdown(id, text, menu)` | borderless 13 pt text + 12 pt chevron, 22 high; `hover × 0.7` fill hovered, `pressed` while held or open; a click toggles a popup menu anchored below it and kept inside the window |
| `search_field(id, clear_id, text, hint, width)` | 28 high, radius 4, `field` fill (`canvas` focused), border `field_border` (`button_border` hovered, 1.5 pt `accent` focused), 14 pt magnifier + placeholder `text_dim`, clear (×) button `clear_id` when non-empty (clearing keeps the focus); filters live on every keystroke (`SearchOut::changed`); edits as `text_field` does (Esc gives back the text from before the edit, and the filter runs again) |
| `text_field(id, text, hint, width)` | the search field's frame without the magnifier and clear button, 24 high. **Every text field edits the same way** (LightCraft's text field; `TextField` adds options): Esc gives back the text from before the edit; Return, or the focus leaving (a click away, Tab), ends it, reported once (`FieldOut::ending`: `Return`, `Away` or `Escape`; `committed()`, `cancelled()`); switching to another app does not end it; `select_on_focus` selects the whole text when the field takes the focus (a drag still selects what it covers), and `focus_field(id)` gives it the focus from code; a right-click opens Cut, Copy, Paste and Select All (with `Ctrl`/`Cmd` + X, C, V, A beside them; Cut and Copy greyed out with nothing selected; a greyed item ignores clicks; Esc closes only the menu; the selection survives the right-click), registered as `<id>/cut`, `/copy`, `/paste`, `/selectAll` (role `menuItem`). New text boxes use these, never a bare `egui::TextEdit` |
| `autocomplete(id, text, options, field)` | a list under a field (`field` draws the `text_field` or `search_field` registered as `id`): while it has the focus and its text is not empty, the options holding every word typed (any case, any script), those starting with the text first, at most 100; 8 rows of 22 visible, more scroll (wheel, keys; the bar takes no drags); the popup frame (`chrome`, `button_border`, radius 6, popup shadow), as wide as the field, 2 pt below it, above it when there is no room below, never outside the window; rows 13 pt `text_label`, `hover` fill and `text` hovered, `accent` fill and `on_accent` text highlighted. ↓ / ↑ move the highlight (nothing highlighted until ↓; ↑ from the first row goes back to the typed text); Enter picks the highlighted option, or with none commits the typed text; a click picks; a pick puts the option's text in the field and ends the edit once (`AutocompleteOut::picked`); Esc closes the list and keeps the edit (it opens again on the next key; a second Esc reverts). Rows register as `<id>/option/<index>` (role `suggestion`, active while highlighted). For tag entry (M6) and search suggestions (M9) |
| `date_field(id, value, width)` / `time_field(id, value, width)` | **Dalica** (M6.29, `widgets/datetime.rs`): a `text_field` editing a `PickedDate` as `YYYY-MM-DD` or a `PickedTime` as `HH:MM:SS` (24 hours), placeholder the pattern. The value follows the text whenever it reads as a day (or time) that exists; while it does not (`2026-02-31`, half a date) the field is outlined in `caution`, and when the edit ends the text goes back to the last value (a day that does not exist is never stored); Esc gives back the value from before the edit. ↑ / ↓ step the part under the cursor (year, month or day; hour, minute or second) by one and select it: months, days and the time's parts wrap within their range without carrying, years stop at 1 and 9999, a day past its month's end becomes the last day. No locale formats yet: callers show the long date beside it. Date rules ported from LightCraft's date picker; used by Adjust Date and Time |
| `tooltip(response, title, detail)` | title 13 `text`, optional detail 11 `text_dim`, wrapped at 280 pt, in the popup frame (`chrome`, `button_border`, radius 6, popup shadow); shown on disabled controls too, so they can say why |
| `Form` (`label_w`), `Form::row(label, controls)` | **Dalica** (M8.5, `widgets/form.rs`; the Export dialog, `creating.md` §2): a settings form in two columns: the labels (13 `text_label`, `text_disabled` disabled) right-aligned in one column as wide as the widest label (`Form::new` measures them; at most 240, longer cut with "…"), the controls left-aligned 10 pt after it (`FORM_GAP`), top-down; the label sits on the middle of the row's first 24 pt line, and an empty label continues the row above; rows 8 pt apart (`FORM_ROW_GAP`). `hint(text)` is a line under a control (11 `text_dim`, wrapped to the column). `form_group(enabled, rows)` disables rows as a group: labels, controls and hints take their disabled look, ignore clicks and keys, and register as disabled. Labels are text, not controls |
| `checkbox(id, label, on)` / `radio(id, label, checked)` | **Dalica** (M8.5): one 24 pt line, the whole line clickable; a 14 pt box (radius 3) or a circle of radius 7, `field` with a 1 pt outline (`button_border`, `text_label` hovered or held, `text_disabled` disabled), the label 13 `text_label` 22 pt from the left. Checked, the box turns `accent` with an `on_accent` check mark and the circle holds a 3.5 pt `accent` dot (both `text_disabled` disabled). The focus ring circles the mark. Registered as `id` (role `toggle`, active when on). `radio_row(id, label, checked, controls)` is a radio followed on its line by the controls it governs (`(•) Resize to: [1600] pixels ───○───`), enabled only while it is checked |
| `stops_slider(spec, stops, field, track_w)` | **Dalica** (M8.5): the export size's slider on one 24 pt line: with `field`, a 56 pt `text_field` (`<id>/value`, its text selected on focus) and the spec's unit after it, then the `slider`'s 2 pt track and ring thumb with a 1 × 4 pt tick 4 pt under the track for each stop (`track`). A drag or click sets the value and snaps to a stop within 6 pt (`STOP_SNAP`), else to the spec's steps; a typed value (Return or clicking away) is kept exactly, on the steps and clamped, never snapped to a stop; Esc or a value that is not a number keeps the old one; ↑ / ↓ with the pointer on the track or the focus, and ← / → with the focus, go to the next stop (past the last, the end of the range); double-click resets to the default. Without `field` it is the bare track (the Custom quality). Registers `<id>` (role `other`) |
| `dropdown_item(id, title, description, checked)` | **Dalica** (M8.5): a `dropdown` menu row with a description line ("Automatic" over "Keeps the original's quality"): a 24 pt column holding a 12 pt check mark when checked, the title 13 `text_label` and the description 11 `text_dim` under it; 38 high (24 without a description), as wide as the menu (200 to 420); `hover` fill radius 4 (`pressed` held) with a `text` title and mark; disabled all `text_disabled`. A click closes the menu. Registered as `id` (role `menuItem`, active when checked, labelled "title: description") |
| `text_edit`, `combo` | egui defaults under the theme visuals. Labels are not selectable (`selectable_labels` off): a label is text, not a control without an id |
| `thumbnail_cell` | see §7 |
| `folder_header` | see §7 |
| `tray_well` | see `ui-layout.md` → Bottom area |
| `toast(id, area, text, kind, age, action)` | bottom-centre of the light table / canvas, 24 pt above its bottom; three kinds (`ToastKind`): **info** (a confirmation, 1.4 s), **caution** (a warning to read with nothing lost, such as a metadata write the sidecar keeps instead, 6 s, a 4 pt `caution` bar at its left edge) and **error** (a failure, 6 s, a 4 pt `danger` bar at its left edge); the last 0.2 s fading out; radius 6, `toast_bg` (`black α200`), padding 16 × 10, text 14 `text` wrapped to at most three lines (`TOAST_LINES`) at the area's width less the padding and 24 pt each side (40–520 pt), and past three lines cut short in the middle with "…", the first characters the caller names (`ToastText`, e.g. a file name) kept whole; an optional text-button action (`button:toast.showFolder`) |
| `progress_row(id, label, fraction, cancel_id, width)` | for background jobs in the status line and progress dialogs: 20 high, label 11 `text_dim` elided, a 4 pt bar (`track`, `accent` fill; a sliding third when the fraction is unknown), a 16 pt × cancel button |
| `empty_state(id, title, body, action)` | centred SemiBold 18 title + Inter 13 body (wrapped at 360, centred) + optional primary button 16 pt below |
| `splitter(id, rect)` | 8 wide, `chrome` with a grip of two 1 pt lines 48 long (`button_border`; `icon` with a `hover × 0.5` strip hovered; `text` with a `hover` strip dragged); horizontal when wider than tall; the caller adds the drag delta and `arrow_delta` (±8 with the focus) through `split_position`; double-click = reset |
| `dialog(id, title, width, buttons, body)` | dim backdrop `black α140` that takes the clicks (it does not cancel), centred modal window `chrome`, 1 pt `button_border`, radius 6, window shadow, margin 16 × 12, SemiBold 15 title; buttons right-aligned, laid out right-to-left (avoids auto-size growth), the default one a `primary_button`; Esc = Cancel, Enter = default (unless a field keeps the focus, e.g. a multi-line text); registers `dialog:<kind>`, `dialog:<kind>/backdrop` and each button |

The edit view's canvas input (M7.5, `crates/ui-kit/src/input/`, ported from PhotoCraft without its
preferences): `input::wheel::read` classifies one frame's wheel input: a notched mouse wheel zooms
around the pointer, exactly ×1.1 per notch, the frame the notch arrives (egui's smoothed tail is
ignored); a trackpad's two-finger scroll (point units) pans; ⇧ or ⌘/Ctrl + wheel scrolls sideways
and ⌥/Alt + wheel pans (after `wheel::configure`); a pinch zooms (egui's pinch event, or on Windows
the Ctrl + wheel fractions `wheel::fold_legacy_pinch` turns back into one). `input::flick::frame`
glides a pan on after a drag is let go while still moving (faster than 150 pt/s; friction τ 0.18 s;
at most 6000 pt/s), and any click, drag or key stops it.

Every widget has the states rest, hover, pressed, active (toggled, selected or open), disabled and
focused. The keyboard focus ring is a 1.5 pt `accent` outline 1 pt outside the control, so it reads
on every fill (the primary button's included). `crates/ui-kit/tests/widgets.rs` renders each widget
in each state with real input (pointer moved, button held, focus requested) and keeps a fingerprint
per widget. The small marks inside widgets (a chevron, the magnifier, a clear cross) are drawn by
the widget itself; the icons a caller picks are §5's.

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
  Badges are always shown, as in the reference behaviour. Glyphs are `text` (the star `star`); the
  edited mark is a pencil the badge draws itself. When the image is too narrow for the whole row,
  plates are left out from the left, so the star stays longest; the duration pill (`caption` size,
  `text` on `badge_plate`, fully rounded) shows only where it fits between the sprocket strip
  (7 % of the image width, 5–10 pt, `badge_plate` with `text_label` holes) and the row. One ui-kit
  painter draws all of it (`widgets::badges`), fed what the model knows.
- The photo's drop shadow is `black α90`, 2 pt down, 4 pt blur, under the image only.
- Hover tooltip: after the pointer has rested 600 ms on the cell (counted from when it came onto
  the cell or last pressed it), the file name over one `text_dim` line: date taken (else the
  file's date) · pixel size · file size; for a photo that cannot be shown, the reason instead.
- A hidden photo (shown only with View ▸ Hidden Pictures) is drawn at 40 % opacity, its surround,
  outline, badges and caption included.
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
- Minimum contrast: `text_label` on `chrome`, `text_dim` on `chrome` (status line, tooltip detail,
  window title: 4.54:1) and `text_dim` on `grid_bg` must stay ≥ 4.5:1; check with the contrast
  tests in `dalica-ui-kit` (`theme.rs` and `tests/contrast.rs`) when tokens change.
  `tests/contrast.rs` checks every text/background pair the widgets paint at 4.5:1 and every glyph
  at 3:1, translucent plates blended over what lies beneath. `text_dim` on the other surfaces:
  `button` 5.1, `inset` 5.2, `tray_bg` 5.0, `field` 5.2, `canvas` 5.6, `grid_bg` 6.3. Three pairs
  fall short by decision and are pinned so they cannot get worse: `text_dim` on `hover` (3.75:1,
  counts in a hovered row; 4.5:1 would need about `#a8a8a8`, too close to `text_label`), `accent`
  on `chrome` (2.6:1: the focus ring and drop-target outline, below WCAG 2.2's 3:1), and the slider
  `track` (2.0:1, a guide under the 5.3:1 ring).

## 10. Verification

Visual changes are not done until they have been looked at: render with the headless snapshot
(`dalica-cli snapshot …`) or the running app's `ui.screenshot`, at 1400 × 900 (and 900 × 600 when
the layout changes), and
compare against the reference layout notes (see [`process.md`](process.md)). Attach before/after
screenshots to PRs.

## 11. Window chrome, menus, preferences and scaling

Adapted from PhotoCraft `docs/ui-design.md` (revision 59684266) and LightCraft `crates/ui-egui/src/menu_level.rs` (revision 88ea1a8); MIT OR Apache-2.0.

### 11.1 Title bar

- **Windows and Linux (default):** the window has no OS decorations, so there is one bar, not a
  title bar stacked over a menu bar. The 32 pt menu-bar row (`ui-layout.md` §2) *is* the title bar
  (`crates/ui/src/titlebar.rs`), with the metrics of the Craft apps' title bar (PhotoCraft's default
  theme: a 32 pt bar, an 18 pt app mark 10 pt from the left edge, the title in 13 pt `text_dim`,
  Windows-style caption buttons; maintainer request at the M7 review, replacing a 24 pt row with a
  16 pt icon 8 pt in and 14 pt icon glyphs). Its last point is a 1 pt `divider` line between it and
  the toolbar. Left to right: the app icon (18 pt, 10 pt from the left edge, centred in the row;
  the committed render from `assets/app-icon/`, never a glyph drawn in code: the smallest hicolor
  PNG that covers its pixels, so 24 px at 1× and 48 px at 2×, `crates/ui/src/app_icon.rs`), 6 pt,
  the menus (centred in the row above the divider), the window title (Inter Regular 13,
  `text_dim`, "Dalica" or "<folder or album> — Dalica") centred on the window and slid into the
  free gap between the menus and the caption buttons when the centre would bring it within 16 pt
  of either, then Minimize, Maximize/Restore and Close flush in the top-right corner (46 × 31 each,
  above the divider; 10 pt glyphs drawn in 1 pt lines in `icon`: a bar, a square, two offset
  squares for Restore, an X; hover `hover`, pressed `pressed`, Close hover `caption_close` with a
  white glyph and `caption_close × 0.8` while held). The free gap between the menus and the caption buttons drags the
  window, a double-click there maximizes/restores it, and invisible 5 pt edges (12 pt corners)
  resize it. Close runs **File → Exit**, so pending writes and running jobs are handled first: it
  closes the window the way the OS close does, and the library is flushed before the process ends.
  The title is also the OS window's title (task bar, window switcher). The title bar is drawn over
  the "Opening the library…" and "could not be opened" screens too, so the window can always be
  moved and closed.
- **System title bar option:** Options → General → Interface → **Use system title bar** (Windows
  and Linux only, default off) gives the window the OS decorations and draws the menu bar below
  them. It shows "Applies at next launch." beneath the checkbox (before and after Apply); saving it
  never restarts the app or changes the current window. Linux desktops where client-side decorations
  misbehave can use it as the fix. Stored in `settings.json` as `"interface": {"systemTitleBar":
  true}`; the window reads it when it is made.
- **macOS:** native traffic lights over an integrated title strip (the window title centred in it;
  the strip right of the lights, 78 pt in as in the Craft apps, drags the window); the menus live
  in the system menu bar (generated by `muda` from the same menu model), so the in-window menu row
  is not drawn. Its items run exactly
  as the in-window ones do; Hide Dalica and Quit Dalica are the system's own items (Cmd+H, Cmd+Q),
  and the other items carry no key equivalents: Dalica's keymap handles every shortcut on every OS.
- **One rule for renders:** the window decides its chrome once, when it is made, from the host OS
  and the setting. Headless and snapshot renders always draw the Windows/Linux title bar, on every
  OS, so whole-window fingerprints are the same everywhere.
- **Window state:** the position, size and maximized state are saved in the library's
  `settings.json` (`"window"`) once a move or resize has held for half a second, and at exit; a
  stored value out of range is clamped (size 900 × 600 … 16384) or dropped (position) before the
  window opens. The view state of `ui-layout.md` §2 (tree width, side pane and its width) is saved
  under `"view"`.
- **Narrow windows** drop chrome before anything overlaps, in this order: the window title in the
  title bar, then the toolbar's filter caption (`ui-layout.md` §11), then the search field shrinks
  to a 160 pt minimum. Each goes only when it must, and stays gone while the window narrows
  further: the title when it no longer fits its gap with 16 pt to spare on each side (a long folder
  name drops it early; "Dalica" alone fits at every width down to 900), the caption below 1100, the
  search field down to its minimum. Only text that repeats information shown elsewhere may
  disappear; menus, filter toggles and caption buttons never do. Check at 900 × 600 (the minimum
  window size).

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
  dialog open; it is disabled while the draft matches the saved settings (a value changed and put
  back counts as unchanged). **OK** applies (if needed)
  and closes. **Cancel** (and Esc) discards only the changes made since the last successful Apply.
- A failed Apply (for example the settings file could not be written) shows the error in the
  dialog, leaves the draft open for correction and changes nothing.
- Most options take effect on Apply. UI scale and UI font size (and UI language, once it exists)
  preview live while the dialog is open and revert on Cancel. Options that need a restart (Use
  system title bar) say "Applies at next launch." and are never applied to the running window.
- A tab lays out its options on the form rows of §6: each group's on/off options as checkboxes
  under the group's right-aligned label ("Interface:", "Files:"), every other option on a row of
  its own labelled with its name (dropdowns, a folder's path with Browse…), groups divided by a
  rule. An option whose feature does not exist yet is drawn disabled with its reason as the tooltip
  ("Arrives with Import"); an option of another platform is left out. A tab taller than the dialog
  scrolls by the wheel, with a solid scroll bar so the rows below are not missed.
- **Show tooltips** off stops every hover tooltip (egui's tooltip delay becomes infinite). **Use
  animations** off makes eased movements jump to their end: the light table's scroll to a section,
  the edit view's Fit and 1:1 zoom and the pan's release glide, and egui's own fades.
- Settings are written atomically like every other file (`data-model.md` §4); a damaged settings
  file is kept as `.corrupt-<time>` and the defaults are used.

### 11.4 UI Scale and high-DPI displays

Options → General → Interface → **UI scale** previews live in the dialog, is saved on Apply, and
multiplies every point size in this document; photo pixels and the edit-view zoom are independent
of it.

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
- `theme::UiScale::factor(system scale, monitor long side)` computes the factor (fixed choices snap
  to 25 % steps inside 75–300 %; an unknown or nonsensical system scale counts as 100 %); the window
  applies it as egui's zoom (the factor over the system scale the window already applies, read
  every frame with the window's current monitor), so widgets never multiply by it themselves.
  Headless runs have no monitor: the pixels per point the driver is given (`--scale`) are the
  system scale, so Auto there is exactly that scale.

### 11.5 UI Font Size

Options → General → Interface → **UI font size** changes interface text independently of UI Scale:
**Small** (default) uses the sizes in §3, **Medium** multiplies them by 15/13 and **Large** by 17/13
(body text 13 → 15 → 17 pt), preserving the relative sizes of titles, captions and small text.
Control heights grow with the text where a label would otherwise be clipped. The choice previews live
in the dialog, persists across launches, and applies to the CJK fallback fonts too, including fonts
loaded after the change. Photo captions and watermarks drawn into output (collages, exports) are never
affected. `Tokens::with_font_size(FontSize)` gives the tokens for a choice (every §3
size times the factor; applying another choice never compounds).
