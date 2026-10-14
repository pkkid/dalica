# dalica-ui-kit

**Layer L5.** The generic look: `Tokens`, widgets, icons drawn in code, the widget registry and the CPU rasterizer for screenshots. Knows nothing about photos or the engine.

- **May depend on:** `core` (+ egui; tests also use `scenes` for fingerprints) (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
- **Public API:**
  - `theme`: `Tokens` (`DARK`, `get`, `apply`, `with_font_size`) + `Metrics` (`DEFAULT`,
    `tree_range`, `side_pane_large`; the edit panel's `edit_tabs`, `edit_tab`, `tool_tile`),
    `FontSize`, `UiScale`, `HISTOGRAM`, `HISTOGRAM_FILL_ALPHA` and `histogram_fill` (overlapping
    channels added, M7.5), `contrast` and `over` (WCAG contrast, translucent plates blended as
    painted).
  - `fonts`: Inter, installed in code (`install`, `definitions`, `regular`, `semibold`,
    `SEMIBOLD_FAMILY`).
  - `icons`: `Icon` (+ `Icon::ALL`, `name`, `from_name`), `paint`, `paint_activity` (the activity
    spinner at a phase), `stroke_width`; the folder-tree marks `FolderIcon`, `paint_folder`,
    `state_color`, `INHERITED_OPACITY`; the edit view's glyphs `EditIcon` (+ `EditIcon::ALL`,
    `name`, `from_name`: the tabs, the Basic Fixes tools and the edit panel's buttons, M7.6) and
    `paint_edit`; Save's and Export's glyphs `OutputIcon` (`Save`, a disk, and `Project`, a
    briefcase for the tree's project rows; + `OutputIcon::ALL`, `name`, `from_name`, M8.5) and
    `paint_output`.
  - `input` (M7.5, the edit canvas's pointer input, ported from PhotoCraft): `wheel` (`Wheel`,
    `read` for one frame's wheel input: a notch zooms ×`NOTCH` around the pointer, a trackpad
    pans, a pinch zooms; `configure`, `fold_legacy_pinch` / `legacy_pinch_to_zoom` for Windows'
    Ctrl + wheel pinch, `classify` + `Input` for tests) and `flick` (`Flick`, `frame`: a pan that
    glides on after a fast drag, `FLICK_TAU`, `FLICK_MIN_START`, `FLICK_MAX_SPEED`,
    `FLICK_MAX_DT`).
  - `widgets`: `register` + `Target` (the one way to get an automation id), `focus_ring`,
    `label_in`; buttons `text_button`, `icon_button`, `star_toggle`, `primary_button`,
    `action_button`, `toggle_filter`, `segmented` (+ `Segment`, `SegmentLabel`); fields
    `text_field` (+ `FieldOut`, `Ending`, `TextField`, `focus_field`; Esc reverts, Return or a click away commits once, the Cut/Copy/Paste/Select All menu: M5.25), `search_field` (+ `SearchOut`), `autocomplete` (type-ahead list under a field, `Role::Suggestion` rows: M5.25); `slider` (+ `SliderSpec`, `SliderOut`,
    `typed_value`, `nudged`, `edit_slider_value`), `stops_slider` (+ `StopsSlider`,
    `snap_to_stop`, `next_stop`, `STOP_SNAP`, `STOPS_FIELD_W`, `STOPS_TRACK_MIN`: the export
    size's slider with a linked number field and ticks, snapping near its stops, M8.5), `tuning_slider` (the Tuning tab's variant, value
    as a tooltip, + / − nudges: + `tuning_nudged`, `TUNING_NUDGE`, M7.5), `zoom_slider`; the edit
    panel's `tabs` (+ `Tab`, `TabItem`, `TabsOut`, `fit_tabs` / `TabFit`, `tabs_overflow_id`,
    `TAB_MIN_WIDTH`, `TAB_OVERFLOW_WIDTH`: icon tabs that shrink, then overflow into a menu, M7.5),
    `tool_button` (+ `tool_button_width`, `TOOL_BUTTON_H`: the Basic Fixes grid, M7.5) and
    `histogram` (+ `paint_histogram`, `histogram_curves`, `histogram_mesh`, `MAX_BINS`,
    `HISTOGRAM_HEADROOM`, M7.5); `date_field` / `time_field` (+ `PickedDate`, `PickedTime`,
    `PickerOut`, `DatePart`, `TimePart`, `days_in_month`, M6.29); `dropdown` (+
    `dropdown_popup_id`; `dropdown_item` + `DROPDOWN_ITEM_MIN_W` / `DROPDOWN_ITEM_MAX_W`: a menu
    row with a description line and a check mark, M8.5); the form of settings dialogs (M8.5):
    `Form` (`new`, `row`: labels right-aligned in one column, controls left-aligned `FORM_GAP`
    after it, rows `FORM_ROW_GAP` apart, `FORM_LABEL_MAX`), `hint`, `form_group` (rows disabled
    together), `checkbox`, `radio`, `radio_row` (a radio and the controls it enables; +
    `CHECK_BOX`, `RADIO_R`, `CHOICE_LABEL_X`); `dialog` (+ `DialogButton`, `ButtonKind`, `DialogResponse`);
    `section_header`; `splitter` (+ `arrow_delta`, `split_position`); `tooltip` (+
    `TOOLTIP_WIDTH`); `progress_row` (+ `ProgressOut`), `toast` (+ `ToastKind` `Info` / `Caution` / `Error`,
    `toast_opacity`, `TOAST_SECS`, `TOAST_ERROR_SECS`; M7.27: `ToastText` (the text and how many
    leading characters stay whole), `toast_galley` (wrapped to `TOAST_LINES`, then cut short in
    the middle), `elide_middle`, `toast_text_width`), `empty_state`, `tooltip_now`; thumbnail marks `badges` (+ `Badges`,
    `Badge`, `Video`, `BadgeRects`, `duration_text`), `badge_row`, `star_badge`, `photo_shadow`,
    `failed_mark`.
  - `registry`: `register`, `widgets`, `find`, `unregistered` (+ `Unregistered`), `is_valid_id`,
    `WidgetInfo`, `Role`.
  - `raster`: `Rasterizer`, `Image`, `RasterError`, `MAX_SIDE`, `MAX_PIXELS`; and at the root
    `snapshot`, `raw_input`, `paint` for headless frames.
- **Test it alone:** `cargo test -p dalica-ui-kit` (no window, GPU, network or personal photos needed).
  The gallery, widget-sheet and icon tests write their renders to `target/test-output/ui-kit/`
  (`gallery.png`, `widgets-*.png`, `icons.png`); `DALICA_BLESS=1` stores new fingerprints in
  `tests/fingerprints/` (look at the PNGs first). `tests/contrast.rs` checks every token pair the
  widgets paint against WCAG (`docs/design-system.md` §9).
