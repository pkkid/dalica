# dalica-ui-kit

**Layer L5.** The generic look: `Tokens`, widgets, icons drawn in code, the widget registry and the CPU rasterizer for screenshots. Knows nothing about photos or the engine.

- **May depend on:** `core` (+ egui; tests also use `scenes` for fingerprints) (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
- **Public API:**
  - `theme`: `Tokens` (`DARK`, `get`, `apply`, `with_font_size`) + `Metrics` (`DEFAULT`,
    `tree_range`, `side_pane_large`; the edit panel's `edit_tabs`, `edit_tab`, `tool_tile`),
    `FontSize`, `UiScale`, `HISTOGRAM`, `HISTOGRAM_FILL_ALPHA` and `histogram_fill` (overlapping
    channels added, M7.5), `contrast` and `over` (WCAG contrast, translucent plates blended as
    painted). Among the tokens, `crosshair` and `crosshair_outer` paint an effect's crosshair on
    the photo and its dark outline (M12.4).
  - `fonts`: Inter, installed in code (`install`, `definitions`, `regular`, `semibold`,
    `SEMIBOLD_FAMILY`).
  - `icons`: `Icon` (+ `Icon::ALL`, `name`, `from_name`), `paint`, `paint_activity` (the activity
    spinner at a phase), `stroke_width`; the folder-tree marks `FolderIcon`, `paint_folder`,
    `state_color`, `INHERITED_OPACITY`; the edit view's glyphs `EditIcon` (+ `EditIcon::ALL`,
    `name`, `from_name`: the tabs, the Basic Fixes tools and the edit panel's buttons, M7.6) and
    `paint_edit`; Save's and Export's glyphs `OutputIcon` (`Save`, a disk, and `Project`, a
    briefcase for the tree's project rows; + `OutputIcon::ALL`, `name`, `from_name`, M8.5) and
    `paint_output`; the slideshow's and the viewer's `ViewingIcon` + `paint_viewing`, the albums'
    `AlbumIcon` + `paint_album`, the Options dialog's `OptionsIcon` + `paint_options`, and the
    effect panels' `EffectIcon` (`Brush` and `Eraser` for Soften, `Crosshair`, `OneClick` for the
    "1-click" badge; + `EffectIcon::ALL`, `name`, `from_name`, M12.4) + `paint_effect`, people's and places'
    `PeopleIcon` + `paint_people`, the Collage room's `CollageIcon` (+ `CollageIcon::ALL`, `name`,
    `from_name`, M14.7) + `paint_collage`, the video player's `VideoIcon` (+ `VideoIcon::ALL`,
    `name`, `from_name`, `shared`, `for_volume`, M15.6) + `paint_video` and the system's file
    hand-offs' `OsIcon` (+ `OsIcon::ALL`, `name`, `from_name`, `shared`, M15.6) + `paint_os`;
    `Glyph` (any of these sets) + `paint_glyph`.
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
    a dialog's `text_tabs` (+ `TextTab`, `TextTabsOut`, `text_tabs_height`, `tab_page`), the
    rooms' `room_tabs` (+ `RoomTab`, `ShownRoomTab`, `RoomTabsOut`, `room_tab_close_id`,
    `ROOM_TAB_MIN_WIDTH`, `ROOM_TAB_PAD`, `ROOM_TAB_CLOSE`: icon and label tabs with a close ×,
    M11),
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
    `failed_mark`, `excluded_mark`. The slideshow's and the viewer's overlay (M11): `overlay_bar`
    (+ `overlay_bar_rect`, `overlay_button`, `OverlayFade`: a translucent bar of light-on-dark
    buttons fading over `OVERLAY_FADE_SECS` on the caller's clock), `stepper` (+ `StepperOut`,
    `stepped`, `stepper_ids`: `[−] 3 seconds [+]`) and `caption_plate` (+ `caption_galley`,
    `CaptionPlateOut`: the show's caption on a rounded plate, two lines then "…"). The effect
    panels (M12.4): `effect_tile` and `effect_grid` (+ `EffectTile`, `EffectGridOut`,
    `effect_grid_size`, `effect_tile_id`: a tab's live previews, three to a row, at most four
    rows), `effect_slider` (the tuning slider with its label above, on a `SliderScale`: `Linear`
    or `Log`; + `scale_fraction`, `scale_value`, `scale_nudged`), the colour `swatch` and its picker
    popup (+ `SwatchOut` with `eyedropper` to arm the eyedropper, `open_swatch`, `close_swatch`,
    `swatch_open`, `swatch_popup_rect`, `field_color`, `hue_color` …: a saturation / value field,
    a hue strip and a hex field) and `crosshair` (+ `Crosshair`, `CrosshairOut`,
    `crosshair_handle_id`, `handle_angle`: a draggable centre, an optional radius circle and
    second handle, in coordinates normalised to the photo shown). Also: `age_slider`,
    `filter_banner`, `checkbox_row`, `group_header`, `list_row` (+ `list_columns`, `list_text`),
    `key_cell` (the shortcut editor's), the drag pill (`drag_pill`, `drop_target`,
    `insertion_marker`). The Collage room's (M14.7): `rich_dropdown` and `rich_item` (+ `RichRow`,
    `RichRowText`, `rich_row_text`, `rich_row_min_width`, `RICH_ROW_H`), `toggle_group` (+
    `ImageToggle`, `ToggleGroupOut`, `toggle_step`) and `transform_handle` (+ `TransformHandle`,
    `TransformOut`, `transform_corners`, `transform_handle_pos`, `turn_and_scale`). The video
    player's (M15.6): `scrubber` (+ `Scrubber`, `ScrubberOut`, `scrubber_readout`, `clock_text`,
    `trim_start_to`, `trim_end_to`, `SCRUB_TRACK_ROW`, `SCRUB_RULER_ROW`, `PLAYHEAD_R`,
    `TRIM_GRAB`: the playhead, the trim ruler and the readout in milliseconds) and `volume` (+
    `VolumeOut`, `volume_toggled`, `volume_level`, `VOLUME_BUTTON`, `VOLUME_SLIDER_W`: a speaker
    toggle that mutes and restores, and a short slider).
  - `registry`: `register`, `widgets`, `find`, `unregistered` (+ `Unregistered`), `is_valid_id`,
    `WidgetInfo`, `Role`.
  - `raster`: `Rasterizer`, `Image`, `RasterError`, `MAX_SIDE`, `MAX_PIXELS`; and at the root
    `snapshot`, `raw_input`, `paint` for headless frames.
- **Test it alone:** `cargo test -p dalica-ui-kit` (no window, GPU, network or personal photos needed).
  The gallery, widget-sheet and icon tests write their renders to `target/test-output/ui-kit/`
  (`gallery.png`, `widgets-*.png`, `icons.png`); `DALICA_BLESS=1` stores new fingerprints in
  `tests/fingerprints/` (look at the PNGs first). `tests/contrast.rs` checks every token pair the
  widgets paint against WCAG (`docs/design-system.md` §9).
