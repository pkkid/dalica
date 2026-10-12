# dalica-ui-kit

**Layer L5.** The generic look: `Tokens`, widgets, icons drawn in code, the widget registry and the CPU rasterizer for screenshots. Knows nothing about photos or the engine.

- **May depend on:** `core` (+ egui; tests also use `scenes` for fingerprints) (`docs/architecture.md` §2; `cargo xtask layers` enforces it).
- **Public API:**
  - `theme`: `Tokens` (`DARK`, `get`, `apply`, `with_font_size`) + `Metrics` (`DEFAULT`,
    `tree_range`, `side_pane_large`), `FontSize`, `UiScale`, `HISTOGRAM`, `contrast` and `over`
    (WCAG contrast, translucent plates blended as painted).
  - `fonts`: Inter, installed in code (`install`, `definitions`, `regular`, `semibold`,
    `SEMIBOLD_FAMILY`).
  - `icons`: `Icon` (+ `Icon::ALL`, `name`, `from_name`), `paint`, `paint_activity` (the activity
    spinner at a phase), `stroke_width`.
  - `widgets`: `register` + `Target` (the one way to get an automation id), `focus_ring`,
    `label_in`; buttons `text_button`, `icon_button`, `star_toggle`, `primary_button`,
    `action_button`, `toggle_filter`, `segmented` (+ `Segment`, `SegmentLabel`); fields
    `text_field` (+ `FieldOut`, `Ending`, `TextField`, `focus_field`; Esc reverts, Return or a click away commits once, the Cut/Copy/Paste/Select All menu: M5.25), `search_field` (+ `SearchOut`), `autocomplete` (type-ahead list under a field, `Role::Suggestion` rows: M5.25); `slider` (+ `SliderSpec`, `SliderOut`,
    `typed_value`, `nudged`, `edit_slider_value`), `zoom_slider`; `dropdown` (+
    `dropdown_popup_id`); `dialog` (+ `DialogButton`, `ButtonKind`, `DialogResponse`);
    `section_header`; `splitter` (+ `arrow_delta`, `split_position`); `tooltip` (+
    `TOOLTIP_WIDTH`); `progress_row` (+ `ProgressOut`), `toast` (+ `ToastKind`, `toast_opacity`,
    `TOAST_SECS`, `TOAST_ERROR_SECS`), `empty_state`, `tooltip_now`; thumbnail marks `badges` (+ `Badges`,
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
