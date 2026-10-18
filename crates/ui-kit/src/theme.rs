//! Design tokens (`docs/design-system.md` §2–§4). Every colour, metric and font size the UI paints
//! with comes from [`Tokens`] (colours and type) and its [`Metrics`] (sizes); widgets never
//! hard-code a colour. [`Tokens::apply`] turns them into egui's style and visuals and stores them in
//! the context, where [`Tokens::get`] finds them. [`FontSize`] and [`UiScale`] are the two
//! Options → Interface factors of §11.4–§11.5.

use egui::{Color32, Context, CornerRadius, FontFamily, FontId, Id, Shadow, Stroke, TextStyle, Vec2};

use crate::fonts;

const fn grey(v: u8) -> Color32 {
    Color32::from_rgb(v, v, v)
}

const fn v2(x: f32, y: f32) -> Vec2 {
    Vec2 { x, y }
}

/// Histogram channel colours (photo content, not chrome: `design-system.md` §2).
pub const HISTOGRAM: [Color32; 3] = [Color32::from_rgb(0xdf, 0x39, 0x39), Color32::from_rgb(0x44, 0xb0, 0x72), Color32::from_rgb(0x3b, 0x6f, 0xe0)];

/// How opaque the histogram's channel areas are (`design-system.md` §6): translucent, so the
/// canvas shows through and the outlines on top read.
pub const HISTOGRAM_FILL_ALPHA: f32 = 0.55;

/// The colour of the histogram where the channels in `mask` overlap (bit 0 red, 1 green, 2 blue):
/// the [`HISTOGRAM`] colours of those channels added together (saturating, so all three make
/// white), at [`HISTOGRAM_FILL_ALPHA`]. Exact, whatever order the channels are painted in.
pub fn histogram_fill(mask: u8) -> Color32 {
    let mut sum = [0u8; 3];
    for (bit, color) in HISTOGRAM.iter().enumerate() {
        if mask & (1 << bit) != 0 {
            for (s, c) in sum.iter_mut().zip([color.r(), color.g(), color.b()]) {
                *s = s.saturating_add(c);
            }
        }
    }
    Color32::from_rgb(sum[0], sum[1], sum[2]).gamma_multiply(HISTOGRAM_FILL_ALPHA)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    /// `#2d2d2d`: toolbar, side panes, bottom tray area, menu bar, dialogs (`panel_fill`, `window_fill`).
    pub chrome: Color32,
    /// `#1c1c1c`: edit-view canvas, filmstrip, folder header blocks, selected tree row, badge plates.
    pub canvas: Color32,
    /// `#0f0f0f`: background of the light table.
    pub grid_bg: Color32,
    /// `#1c1c1c`: thumbnail cell surround.
    pub cell: Color32,
    /// `#2d2d2d`: selected cell surround.
    pub cell_selected: Color32,
    /// `#1c1c1c`: 1 pt separators between regions, panel strokes.
    pub divider: Color32,
    /// `#232323`: sunken rows, tool tiles in the edit panel.
    pub inset: Color32,
    /// `#222222`: text fields, the search box.
    pub field: Color32,
    /// `#3c3c3c`: a text field's border at rest.
    pub field_border: Color32,
    /// `#242424`: button fill at rest.
    pub button: Color32,
    /// `#4c4c4c`: button stroke, window stroke.
    pub button_border: Color32,
    /// `#3a3a3a`: hovered rows and buttons.
    pub hover: Color32,
    /// `#3f3f3f`: pressed or toggled-on buttons.
    pub pressed: Color32,
    /// `#3f3f3f`: the active tool, an active filter toggle.
    pub tool_active: Color32,
    /// `#e2e2e2`: primary text.
    pub text: Color32,
    /// `#bcbcbc`: default text.
    pub text_label: Color32,
    /// `#949494`: counts, captions, hints, placeholders (4.5:1 on `chrome`).
    pub text_dim: Color32,
    /// `#5c5c5c`: disabled controls.
    pub text_disabled: Color32,
    /// `#9a9a9a`: icon glyphs at rest.
    pub icon: Color32,
    /// `#5a5a5a`: slider tracks, the thumbnail-size slider.
    pub track: Color32,
    /// `#a0a0a0`: a slider's ring at rest.
    pub thumb: Color32,
    /// `#e0e0e0`: a slider's ring hovered or dragged; the zoom slider's thumb.
    pub thumb_hover: Color32,
    /// `#0165dd`: focus border, selection, drop targets.
    pub accent: Color32,
    /// `#1a74e0`: a primary button hovered (`accent` 10 % towards white).
    pub accent_hover: Color32,
    /// `#0156bc`: a primary button pressed (`accent` × 0.85).
    pub accent_pressed: Color32,
    /// `#ffffff`: text and glyphs on an `accent` fill (the primary button).
    pub on_accent: Color32,
    /// `#d8d8d8`: the star badge glyph, star buttons.
    pub star: Color32,
    /// `#f0f0f0`: active icon glyph.
    pub pick: Color32,
    /// `#e04a4a`: destructive confirmations, error badges.
    pub danger: Color32,
    /// `#e3a83c`: warnings: an offline folder, a missing or unreadable file.
    pub caution: Color32,
    /// `#c42b1c`: the title bar's Close button hovered (Windows' own red, as in PhotoCraft), with a
    /// white glyph.
    pub caption_close: Color32,
    /// `#1c1c1c` (`canvas`): the header block above each folder's thumbnails.
    pub folder_header: Color32,
    /// `#262626`: the photo-tray well in the bottom area.
    pub tray_bg: Color32,
    /// `accent`: the 1.5 pt outline of a row or the tray while a drag hovers it.
    pub drop_target: Color32,
    /// `accent × 0.15`: the fill under that outline.
    pub drop_target_fill: Color32,
    /// `#2f3b4d`: the "filter active" banner across the top of the light table.
    pub filter_banner: Color32,
    /// `#f0f0f0`: a face rectangle in the edit view (1.5 pt) …
    pub face_box: Color32,
    /// … and its 1 pt outer outline, `black α160`.
    pub face_box_outer: Color32,
    /// `#f0f0f0`: an effect's crosshair on the photo (its cross, circle, handle and line) …
    pub crosshair: Color32,
    /// … and the dark edge under it that keeps it readable on a bright photo, `black α160`.
    pub crosshair_outer: Color32,
    /// White: the 2 pt outline of the focused (anchor) photo.
    pub photo_focus: Color32,
    /// `gray 170`: the 2 pt outline of the other selected photos.
    pub photo_selected: Color32,
    /// `black α130`: the plate behind thumbnail badges.
    pub badge_plate: Color32,
    /// `gray 38`: a thumbnail whose texture has not arrived.
    pub placeholder: Color32,
    /// `black α140`: the backdrop behind a dialog.
    pub backdrop: Color32,
    /// `black α200`: a toast's plate.
    pub toast_bg: Color32,
    /// Pure black: slideshow and photo-viewer background.
    pub viewer_bg: Color32,
    /// `black α210`: the photo viewer's overlay over the app.
    pub viewer_overlay: Color32,
    /// `black α170`: the slideshow's and the photo viewer's control bar (`viewing.md` §1.3).
    pub overlay_bar: Color32,
    /// `black α160`: the plate behind a slideshow caption (`viewing.md` §1.2).
    pub caption_plate: Color32,
    /// White: text on the overlays (labels, the caption, the stepper's value).
    pub overlay_text: Color32,
    /// `#c8c8c8`: an overlay button's glyph at rest (hovered → `overlay_text`).
    pub overlay_icon: Color32,
    /// `#6a6a6a`: a disabled overlay button's glyph and label.
    pub overlay_disabled: Color32,
    /// `white α28`: an overlay button hovered.
    pub overlay_hover: Color32,
    /// `white α48`: an overlay button held or toggled on.
    pub overlay_pressed: Color32,
    /// `black α150`: the dimming over an excluded cell (the import tray).
    pub excluded_dim: Color32,
    /// Popups, menus and tooltips: offset (0, 4), blur 16, `black α140`.
    pub popup_shadow: Shadow,
    /// Windows and dialogs: offset (0, 8), blur 30, `black α160`.
    pub window_shadow: Shadow,
    /// Corner radius of widgets (points).
    pub radius: u8,
    /// Corner radius of windows, menus and dialogs (points).
    pub window_radius: u8,
    /// Corner radius of toasts (points).
    pub toast_radius: u8,
    /// Corner radius of the slideshow's and the viewer's control bar (points).
    pub overlay_radius: u8,
    /// Corner radius of the filter toggles (points).
    pub filter_radius: u8,
    /// Width of the keyboard focus ring and the drop-target outline (points).
    pub focus_width: f32,
    /// Body text, menu items, text fields (points).
    pub body: f32,
    /// Small text: status line, tooltips' secondary line (points).
    pub small: f32,
    /// Dialog, panel and folder-header titles, SemiBold (points).
    pub title: f32,
    /// egui's `Heading` style, SemiBold (points).
    pub heading: f32,
    /// Text-button labels, SemiBold; bottom action-button labels, Regular (points).
    pub button_label: f32,
    /// Thumbnail captions (points).
    pub caption: f32,
    /// Folder-header date, count and description; slider label and value (points).
    pub detail: f32,
    /// Counts in the library tree; edit-panel tool labels (points).
    pub count: f32,
    /// The caption field under the edited photo; toasts (points).
    pub large: f32,
    /// The empty-state title, SemiBold (points).
    pub empty_title: f32,
    /// The UI Font Size factor the sizes above were multiplied by (1 for Small, `design-system.md` §11.5).
    pub font_scale: f32,
    /// Sizes and lengths (`design-system.md` §4).
    pub metrics: Metrics,
}

/// The metrics of `design-system.md` §4, in points. UI Scale multiplies all of them through egui's
/// zoom factor; widgets never multiply by it themselves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// Menu bar (and title bar) row.
    pub menu_bar_h: f32,
    /// Toolbar below the menu bar.
    pub toolbar_h: f32,
    /// Room tab strip and the width of one tab.
    pub room_tabs_h: f32,
    pub room_tab_w: f32,
    /// Library tree: default and minimum width; its maximum is the window width minus `tree_max_gap`.
    pub tree_w: f32,
    pub tree_min_w: f32,
    pub tree_max_gap: f32,
    /// The splitter's width and its grip (`splitter_w × grip_h`).
    pub splitter_w: f32,
    pub grip_h: f32,
    /// Library tree rows: folder/album row, person row, group header, year divider; indentation per level.
    pub tree_row_h: f32,
    pub person_row_h: f32,
    pub group_header_h: f32,
    pub year_divider_h: f32,
    pub tree_indent: f32,
    /// Section header block on the light table, its icon and its button row.
    pub section_h: f32,
    pub section_icon: f32,
    pub section_button_h: f32,
    /// Thumbnail cell edge: minimum, maximum, default.
    pub thumb_min: f32,
    pub thumb_max: f32,
    pub thumb_default: f32,
    /// Grid gaps (horizontal minimum, vertical) and the left/right margin.
    pub grid_gap: Vec2,
    pub grid_margin: f32,
    /// Light-table scrollbar width and its button height.
    pub scrollbar_w: f32,
    pub scroll_button_h: f32,
    /// Side pane width, the large variant (`min(large_frac · W, large_max)`) and its header.
    pub side_pane_w: f32,
    pub side_pane_large_frac: f32,
    pub side_pane_large_max: f32,
    pub side_pane_header_h: f32,
    /// Status line.
    pub status_h: f32,
    /// Bottom area height and the fraction of the window width at which it splits (tray left).
    pub bottom_h: f32,
    pub bottom_split: f32,
    /// Photo-tray box: left inset, gap left before the split, height; its Hold/Clear/Album buttons.
    pub tray_x: f32,
    pub tray_right_gap: f32,
    pub tray_h: f32,
    pub tray_button: Vec2,
    /// Star and rotate buttons.
    pub star_button: Vec2,
    /// Big action button (icon over label), its icon, and the height of the icon's band.
    pub action_button: Vec2,
    pub action_icon: f32,
    pub action_icon_band: f32,
    /// Edit panel width, its tabs (count and size), tool tile, effect tile and its preview.
    pub edit_panel_w: f32,
    pub edit_tabs: u8,
    pub edit_tab: Vec2,
    pub tool_tile: Vec2,
    pub tool_columns: u8,
    pub effect_tile: Vec2,
    pub effect_preview: Vec2,
    /// Filmstrip cells (edge) and how many are shown.
    pub filmstrip_cell: f32,
    pub filmstrip_min: u8,
    pub filmstrip_max: u8,
    /// Slider row and its label row; the slider ring radius and track thickness.
    pub slider_row_h: f32,
    pub slider_label_h: f32,
    pub slider_ring: f32,
    pub slider_track: f32,
    /// Zoom slider (thumbnail size, edit zoom) and its thumb.
    pub zoom_slider: Vec2,
    pub zoom_thumb: Vec2,
    /// Height of a text button, a text field, the search field; egui's interact height.
    pub button_h: f32,
    pub text_field_h: f32,
    pub search_h: f32,
    pub interact_h: f32,
    /// Filter toggle edge; collapsible section-header height.
    pub filter_toggle: f32,
    pub section_header_h: f32,
    /// Icon sizes: thumbnail badges, tree rows and menus, toolbar toggles, edit tools, action buttons.
    pub icon_badge: f32,
    pub icon_menu: f32,
    pub icon_toolbar: f32,
    pub icon_tool: f32,
    pub icon_action: f32,
    /// Dialog inner margin (horizontal, vertical).
    pub dialog_margin: Vec2,
    /// Toast padding (horizontal, vertical) and its distance above the bottom of its area.
    pub toast_padding: Vec2,
    pub toast_bottom: f32,
    /// Minimum and default window size.
    pub min_window: Vec2,
    pub default_window: Vec2,
}

impl Metrics {
    /// The values of `design-system.md` §4.
    pub const DEFAULT: Metrics = Metrics {
        menu_bar_h: 24.0,
        toolbar_h: 40.0,
        room_tabs_h: 28.0,
        room_tab_w: 190.0,
        tree_w: 240.0,
        tree_min_w: 200.0,
        tree_max_gap: 480.0,
        splitter_w: 8.0,
        grip_h: 48.0,
        tree_row_h: 24.0,
        person_row_h: 28.0,
        group_header_h: 30.0,
        year_divider_h: 18.0,
        tree_indent: 16.0,
        section_h: 96.0,
        section_icon: 32.0,
        section_button_h: 22.0,
        thumb_min: 64.0,
        thumb_max: 320.0,
        thumb_default: 144.0,
        grid_gap: v2(12.0, 22.0),
        grid_margin: 24.0,
        scrollbar_w: 16.0,
        scroll_button_h: 24.0,
        side_pane_w: 280.0,
        side_pane_large_frac: 0.30,
        side_pane_large_max: 392.0,
        side_pane_header_h: 30.0,
        status_h: 20.0,
        bottom_h: 96.0,
        bottom_split: 0.365,
        tray_x: 6.0,
        tray_right_gap: 56.0,
        tray_h: 80.0,
        tray_button: v2(34.0, 24.0),
        star_button: v2(36.0, 24.0),
        action_button: v2(64.0, 56.0),
        action_icon: 28.0,
        action_icon_band: 34.0,
        edit_panel_w: 280.0,
        edit_tabs: 5,
        edit_tab: v2(40.0, 32.0),
        tool_tile: v2(44.0, 30.0),
        tool_columns: 3,
        effect_tile: v2(86.0, 69.0),
        effect_preview: v2(78.0, 48.0),
        filmstrip_cell: 56.0,
        filmstrip_min: 7,
        filmstrip_max: 9,
        slider_row_h: 45.0,
        slider_label_h: 18.0,
        slider_ring: 7.0,
        slider_track: 2.0,
        zoom_slider: v2(110.0, 20.0),
        zoom_thumb: v2(3.0, 14.0),
        button_h: 24.0,
        text_field_h: 24.0,
        search_h: 28.0,
        interact_h: 22.0,
        filter_toggle: 26.0,
        section_header_h: 30.0,
        icon_badge: 12.0,
        icon_menu: 16.0,
        icon_toolbar: 18.0,
        icon_tool: 20.0,
        icon_action: 28.0,
        dialog_margin: v2(16.0, 12.0),
        toast_padding: v2(16.0, 10.0),
        toast_bottom: 24.0,
        min_window: v2(900.0, 600.0),
        default_window: v2(1400.0, 900.0),
    };

    /// The library tree's width range in a window `window_w` wide: `tree_min_w ..= W − tree_max_gap`
    /// (never below the minimum). Hostile input (NaN, infinities) gives the minimum.
    pub fn tree_range(&self, window_w: f32) -> (f32, f32) {
        let max = if window_w.is_finite() { (window_w - self.tree_max_gap).max(self.tree_min_w) } else { self.tree_min_w };
        (self.tree_min_w, max)
    }

    /// The large side pane's width in a window `window_w` wide.
    pub fn side_pane_large(&self, window_w: f32) -> f32 {
        if !window_w.is_finite() || window_w <= 0.0 {
            return self.side_pane_w;
        }
        (self.side_pane_large_frac * window_w).min(self.side_pane_large_max).max(self.side_pane_w)
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Metrics::DEFAULT
    }
}

impl Tokens {
    /// The dark theme, the only theme until after 1.0.
    pub const DARK: Tokens = Tokens {
        chrome: grey(0x2d),
        canvas: grey(0x1c),
        grid_bg: grey(0x0f),
        cell: grey(0x1c),
        cell_selected: grey(0x2d),
        divider: grey(0x1c),
        inset: grey(0x23),
        field: grey(0x22),
        field_border: grey(0x3c),
        button: grey(0x24),
        button_border: grey(0x4c),
        hover: grey(0x3a),
        pressed: grey(0x3f),
        tool_active: grey(0x3f),
        text: grey(0xe2),
        text_label: grey(0xbc),
        text_dim: grey(0x94),
        text_disabled: grey(0x5c),
        icon: grey(0x9a),
        track: grey(0x5a),
        thumb: grey(0xa0),
        thumb_hover: grey(0xe0),
        accent: Color32::from_rgb(0x01, 0x65, 0xdd),
        accent_hover: Color32::from_rgb(0x1a, 0x74, 0xe0),
        accent_pressed: Color32::from_rgb(0x01, 0x56, 0xbc),
        on_accent: Color32::WHITE,
        star: grey(0xd8),
        pick: grey(0xf0),
        danger: Color32::from_rgb(0xe0, 0x4a, 0x4a),
        caution: Color32::from_rgb(0xe3, 0xa8, 0x3c),
        caption_close: Color32::from_rgb(0xc4, 0x2b, 0x1c),
        folder_header: grey(0x1c),
        tray_bg: grey(0x26),
        drop_target: Color32::from_rgb(0x01, 0x65, 0xdd),
        // `accent` × 0.15, premultiplied (what `gamma_multiply(0.15)` gives).
        drop_target_fill: Color32::from_rgba_premultiplied(0, 15, 33, 38),
        filter_banner: Color32::from_rgb(0x2f, 0x3b, 0x4d),
        face_box: grey(0xf0),
        face_box_outer: Color32::from_black_alpha(160),
        crosshair: grey(0xf0),
        crosshair_outer: Color32::from_black_alpha(160),
        photo_focus: Color32::WHITE,
        photo_selected: grey(170),
        badge_plate: Color32::from_black_alpha(130),
        placeholder: grey(38),
        backdrop: Color32::from_black_alpha(140),
        toast_bg: Color32::from_black_alpha(200),
        viewer_bg: Color32::BLACK,
        viewer_overlay: Color32::from_black_alpha(210),
        overlay_bar: Color32::from_black_alpha(170),
        caption_plate: Color32::from_black_alpha(160),
        overlay_text: Color32::WHITE,
        overlay_icon: grey(0xc8),
        overlay_disabled: grey(0x6a),
        // `white α28` and `white α48`, premultiplied (what `from_white_alpha` gives).
        overlay_hover: Color32::from_rgba_premultiplied(28, 28, 28, 28),
        overlay_pressed: Color32::from_rgba_premultiplied(48, 48, 48, 48),
        excluded_dim: Color32::from_black_alpha(150),
        popup_shadow: Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(140) },
        window_shadow: Shadow { offset: [0, 8], blur: 30, spread: 0, color: Color32::from_black_alpha(160) },
        radius: 4,
        window_radius: 6,
        toast_radius: 6,
        overlay_radius: 10,
        filter_radius: 5,
        focus_width: 1.5,
        body: 13.0,
        small: 11.0,
        title: 15.0,
        heading: 16.0,
        button_label: 11.5,
        caption: 10.5,
        detail: 12.5,
        count: 12.0,
        large: 14.0,
        empty_title: 18.0,
        font_scale: 1.0,
        metrics: Metrics::DEFAULT,
    };

    /// These tokens with every font size for `size` (`design-system.md` §11.5): the Small sizes
    /// times [`FontSize::factor`]. Applying it twice does not compound.
    pub fn with_font_size(&self, size: FontSize) -> Tokens {
        let base = if self.font_scale.is_finite() && self.font_scale > 0.0 { self.font_scale } else { 1.0 };
        let k = size.factor() / base;
        let mut t = *self;
        for v in [
            &mut t.body,
            &mut t.small,
            &mut t.title,
            &mut t.heading,
            &mut t.button_label,
            &mut t.caption,
            &mut t.detail,
            &mut t.count,
            &mut t.large,
            &mut t.empty_title,
        ] {
            *v *= k;
        }
        t.font_scale = size.factor();
        t
    }

    /// The tokens [`apply`](Self::apply) last stored in `ctx`, or [`Tokens::DARK`].
    pub fn get(ctx: &Context) -> Tokens {
        ctx.data(|d| d.get_temp::<Tokens>(Self::key())).unwrap_or_default()
    }

    fn key() -> Id {
        Id::new("dalica-ui-kit.tokens")
    }

    /// Installs the fonts and this theme's style into `ctx`, and stores the tokens for
    /// [`get`](Self::get). Fonts take effect on the next pass: paint nothing that matters on the
    /// first one (`design-system.md` §3).
    pub fn apply(&self, ctx: &Context) {
        fonts::install(ctx);
        let t = *self;
        ctx.data_mut(|d| d.insert_temp(Self::key(), t));
        ctx.all_styles_mut(move |style| {
            style.text_styles = [
                (TextStyle::Small, FontId::new(t.small, FontFamily::Proportional)),
                (TextStyle::Body, FontId::new(t.body, FontFamily::Proportional)),
                (TextStyle::Button, FontId::new(t.body, FontFamily::Proportional)),
                (TextStyle::Heading, fonts::semibold(t.heading)),
                (TextStyle::Monospace, FontId::new(t.body, FontFamily::Monospace)),
            ]
            .into();
            style.spacing.item_spacing = Vec2::new(6.0, 4.0);
            style.spacing.button_padding = Vec2::new(8.0, 3.0);
            style.spacing.interact_size.y = t.metrics.interact_h;
            style.animation_time = 0.08;
            // Labels are text, not controls: a selectable label takes clicks without an automation
            // id (`control-protocol.md` §7). Text meant to be copied goes in a registered field.
            style.interaction.selectable_labels = false;
            let v = &mut style.visuals;
            v.dark_mode = true;
            v.panel_fill = t.chrome;
            v.window_fill = t.chrome;
            v.window_stroke = Stroke::new(1.0, t.button_border);
            v.window_corner_radius = CornerRadius::same(t.window_radius);
            v.menu_corner_radius = CornerRadius::same(t.window_radius);
            v.window_shadow = t.window_shadow;
            v.popup_shadow = t.popup_shadow;
            v.extreme_bg_color = t.canvas;
            v.text_edit_bg_color = Some(t.field);
            v.faint_bg_color = t.canvas;
            v.override_text_color = Some(t.text_label);
            v.weak_text_color = Some(t.text_dim);
            v.selection.bg_fill = t.accent.gamma_multiply(0.6);
            v.selection.stroke = Stroke::new(1.0, t.text);
            v.hyperlink_color = t.accent;
            v.warn_fg_color = t.caution;
            v.error_fg_color = t.danger;
            let r = CornerRadius::same(t.radius);
            for (w, fill) in [
                (&mut v.widgets.noninteractive, t.chrome),
                (&mut v.widgets.inactive, t.button),
                (&mut v.widgets.hovered, t.hover),
                (&mut v.widgets.active, t.pressed),
                (&mut v.widgets.open, t.pressed),
            ] {
                w.bg_fill = fill;
                w.weak_bg_fill = fill;
                w.corner_radius = r;
            }
            v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.divider);
            v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text_label);
            v.widgets.inactive.bg_stroke = Stroke::new(1.0, t.button_border);
            v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text_label);
            v.widgets.hovered.bg_stroke = Stroke::new(1.0, t.button_border);
            v.widgets.hovered.fg_stroke = Stroke::new(1.0, t.text);
            v.widgets.active.bg_stroke = Stroke::new(1.0, t.button_border);
            v.widgets.active.fg_stroke = Stroke::new(1.0, t.pick);
        });
    }
}

impl Default for Tokens {
    fn default() -> Self {
        Tokens::DARK
    }
}

/// Options → Interface → UI font size (`design-system.md` §11.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontSize {
    /// The sizes of `design-system.md` §3 (body 13).
    #[default]
    Small,
    /// × 15/13 (body 15).
    Medium,
    /// × 17/13 (body 17).
    Large,
}

impl FontSize {
    /// The factor every font size is multiplied by.
    pub fn factor(self) -> f32 {
        match self {
            FontSize::Small => 1.0,
            FontSize::Medium => 15.0 / 13.0,
            FontSize::Large => 17.0 / 13.0,
        }
    }
}

/// Options → Interface → UI scale (`design-system.md` §11.4): the factor the window applies as
/// egui's zoom, multiplying every point size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UiScale {
    /// Follow the operating system's display scale (with the 4K rule).
    #[default]
    Auto,
    /// A fixed scale in percent, 75 – 300 in steps of 25 (other values snap to the nearest step).
    Percent(u16),
}

impl UiScale {
    /// The smallest fixed scale, in percent.
    pub const MIN_PERCENT: u16 = 75;
    /// The largest fixed scale, in percent.
    pub const MAX_PERCENT: u16 = 300;
    /// A monitor this many pixels or more on its long side counts as 4K.
    pub const FOUR_K: u32 = 3840;

    /// The scale factor for a system scale of `system` (1.0 = 100 %) on a monitor whose long side is
    /// `monitor_long_side` physical pixels (`None` when unknown). Auto follows the system, except
    /// that a 4K monitor with no system scaling gets 2.0; a fixed choice ignores both. A system
    /// scale that is not a finite number in 0.25 … 8 counts as 1.0.
    pub fn factor(self, system: f32, monitor_long_side: Option<u32>) -> f32 {
        match self {
            UiScale::Percent(p) => {
                let p = p.clamp(Self::MIN_PERCENT, Self::MAX_PERCENT);
                let snapped = (u32::from(p) + 12) / 25 * 25;
                snapped as f32 / 100.0
            }
            UiScale::Auto => {
                let system = if system.is_finite() && (0.25..=8.0).contains(&system) { system } else { 1.0 };
                let unscaled = (system - 1.0).abs() < 0.01;
                if unscaled && monitor_long_side.is_some_and(|side| side >= Self::FOUR_K) { 2.0 } else { system }
            }
        }
    }
}

/// WCAG 2 contrast ratio of two opaque colours (1 to 21).
pub fn contrast(a: Color32, b: Color32) -> f64 {
    fn channel(v: u8) -> f64 {
        let c = f64::from(v) / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    }
    fn luminance(c: Color32) -> f64 {
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `fg` (premultiplied, possibly translucent) painted over the opaque `bg`, blended as the
/// rasterizer and the GPU do: the colour a translucent plate (toast, backdrop) shows on screen.
pub fn over(fg: Color32, bg: Color32) -> Color32 {
    let inv = 255 - u16::from(fg.a());
    let mix = |f: u8, b: u8| (u16::from(f) + (u16::from(b) * inv + 127) / 255).min(255) as u8;
    Color32::from_rgb(mix(fg.r(), bg.r()), mix(fg.g(), bg.g()), mix(fg.b(), bg.b()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(c: Color32) -> String {
        format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
    }

    #[test]
    fn tokens_match_the_spec() {
        let t = Tokens::default();
        for (got, want) in [
            (t.chrome, "#2d2d2d"),
            (t.canvas, "#1c1c1c"),
            (t.grid_bg, "#0f0f0f"),
            (t.cell, "#1c1c1c"),
            (t.cell_selected, "#2d2d2d"),
            (t.divider, "#1c1c1c"),
            (t.inset, "#232323"),
            (t.field, "#222222"),
            (t.field_border, "#3c3c3c"),
            (t.button, "#242424"),
            (t.button_border, "#4c4c4c"),
            (t.hover, "#3a3a3a"),
            (t.pressed, "#3f3f3f"),
            (t.tool_active, "#3f3f3f"),
            (t.text, "#e2e2e2"),
            (t.text_label, "#bcbcbc"),
            (t.text_dim, "#949494"),
            (t.text_disabled, "#5c5c5c"),
            (t.icon, "#9a9a9a"),
            (t.track, "#5a5a5a"),
            (t.thumb, "#a0a0a0"),
            (t.thumb_hover, "#e0e0e0"),
            (t.accent, "#0165dd"),
            (t.accent_hover, "#1a74e0"),
            (t.accent_pressed, "#0156bc"),
            (t.on_accent, "#ffffff"),
            (t.star, "#d8d8d8"),
            (t.pick, "#f0f0f0"),
            (t.danger, "#e04a4a"),
            (t.caution, "#e3a83c"),
            (t.caption_close, "#c42b1c"),
            (t.folder_header, "#1c1c1c"),
            (t.tray_bg, "#262626"),
            (t.drop_target, "#0165dd"),
            (t.filter_banner, "#2f3b4d"),
            (t.face_box, "#f0f0f0"),
            (t.crosshair, "#f0f0f0"),
            (t.photo_focus, "#ffffff"),
            (t.photo_selected, "#aaaaaa"),
            (t.placeholder, "#262626"),
            (t.viewer_bg, "#000000"),
            (t.overlay_text, "#ffffff"),
            (t.overlay_icon, "#c8c8c8"),
            (t.overlay_disabled, "#6a6a6a"),
        ] {
            assert_eq!(hex(got), want);
        }
        for (got, alpha) in [(t.badge_plate, 130), (t.backdrop, 140), (t.toast_bg, 200), (t.viewer_overlay, 210), (t.face_box_outer, 160)]
            .into_iter()
            .chain([(t.overlay_bar, 170), (t.caption_plate, 160), (t.excluded_dim, 150), (t.crosshair_outer, 160)])
        {
            assert_eq!(got.to_array(), [0, 0, 0, alpha]);
        }
        assert_eq!(t.drop_target_fill, t.accent.gamma_multiply(0.15));
        assert_eq!(t.folder_header, t.canvas);
        // The primary button's states derive from the accent: 10 % towards white, and × 0.85.
        assert_eq!(t.accent_hover, t.accent.lerp_to_gamma(Color32::WHITE, 0.1));
        assert_eq!(t.accent_pressed, Color32::from_rgb(1, (101.0f32 * 0.85).round() as u8, (221.0f32 * 0.85).round() as u8));
        assert_eq!(t.popup_shadow, Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(140) });
        assert_eq!(t.window_shadow, Shadow { offset: [0, 8], blur: 30, spread: 0, color: Color32::from_black_alpha(160) });
        assert_eq!((t.radius, t.window_radius, t.toast_radius, t.filter_radius, t.focus_width), (4, 6, 6, 5, 1.5));
        assert_eq!(t.overlay_radius, 10);
        assert_eq!((t.overlay_hover, t.overlay_pressed), (Color32::from_white_alpha(28), Color32::from_white_alpha(48)));
        assert_eq!(
            [t.body, t.small, t.title, t.heading, t.button_label, t.caption, t.detail, t.count, t.large, t.empty_title],
            [13.0, 11.0, 15.0, 16.0, 11.5, 10.5, 12.5, 12.0, 14.0, 18.0]
        );
        assert_eq!(HISTOGRAM.map(hex), ["#df3939", "#44b072", "#3b6fe0"]);
    }

    #[test]
    fn metrics_match_the_spec() {
        let m = Metrics::default();
        assert_eq!([m.menu_bar_h, m.toolbar_h, m.room_tabs_h, m.room_tab_w], [24.0, 40.0, 28.0, 190.0]);
        assert_eq!([m.tree_w, m.tree_min_w, m.splitter_w, m.grip_h], [240.0, 200.0, 8.0, 48.0]);
        assert_eq!([m.tree_row_h, m.person_row_h, m.group_header_h, m.year_divider_h, m.tree_indent], [24.0, 28.0, 30.0, 18.0, 16.0]);
        assert_eq!([m.section_h, m.section_icon, m.section_button_h], [96.0, 32.0, 22.0]);
        assert_eq!([m.thumb_min, m.thumb_max, m.thumb_default], [64.0, 320.0, 144.0]);
        assert_eq!((m.grid_gap, m.grid_margin, m.scrollbar_w, m.scroll_button_h), (Vec2::new(12.0, 22.0), 24.0, 16.0, 24.0));
        assert_eq!([m.side_pane_w, m.side_pane_large_max, m.side_pane_header_h, m.status_h, m.bottom_h], [280.0, 392.0, 30.0, 20.0, 96.0]);
        assert_eq!((m.bottom_split, m.tray_x, m.tray_right_gap, m.tray_h), (0.365, 6.0, 56.0, 80.0));
        assert_eq!(
            (m.tray_button, m.star_button, m.action_button, m.action_icon),
            (Vec2::new(34.0, 24.0), Vec2::new(36.0, 24.0), Vec2::new(64.0, 56.0), 28.0)
        );
        assert_eq!(
            (m.edit_panel_w, m.edit_tabs, m.edit_tab, m.tool_tile, m.tool_columns),
            (280.0, 5, Vec2::new(40.0, 32.0), Vec2::new(44.0, 30.0), 3)
        );
        assert_eq!(
            (m.effect_tile, m.effect_preview, m.filmstrip_cell, m.filmstrip_min, m.filmstrip_max),
            (Vec2::new(86.0, 69.0), Vec2::new(78.0, 48.0), 56.0, 7, 9)
        );
        assert_eq!((m.slider_row_h, m.slider_label_h, m.zoom_slider, m.zoom_thumb), (45.0, 18.0, Vec2::new(110.0, 20.0), Vec2::new(3.0, 14.0)));
        assert_eq!((m.slider_ring, m.slider_track, m.interact_h, m.button_h), (7.0, 2.0, 22.0, 24.0));
        assert_eq!((m.search_h, m.filter_toggle, m.section_header_h, m.dialog_margin), (28.0, 26.0, 30.0, Vec2::new(16.0, 12.0)));
        assert_eq!([m.icon_badge, m.icon_menu, m.icon_toolbar, m.icon_tool, m.icon_action], [12.0, 16.0, 18.0, 20.0, 28.0]);
        assert_eq!((m.min_window, m.default_window), (Vec2::new(900.0, 600.0), Vec2::new(1400.0, 900.0)));
        // The tree's range follows the window; the large side pane is 30 % up to 392.
        assert_eq!(m.tree_range(1400.0), (200.0, 920.0));
        assert_eq!(m.tree_range(600.0), (200.0, 200.0), "never below the minimum");
        assert_eq!(m.tree_range(f32::NAN), (200.0, 200.0));
        assert_eq!(m.side_pane_large(1000.0), 300.0);
        assert_eq!(m.side_pane_large(2000.0), 392.0);
        assert_eq!(m.side_pane_large(f32::INFINITY), 280.0);
    }

    #[test]
    fn font_size_scales_every_size_and_never_compounds() {
        let t = Tokens::default();
        assert_eq!(t.with_font_size(FontSize::Small), t);
        let m = t.with_font_size(FontSize::Medium);
        assert!((m.body - 15.0).abs() < 1e-4 && (m.title - 15.0 * 15.0 / 13.0).abs() < 1e-4, "{m:?}");
        let l = m.with_font_size(FontSize::Large);
        assert!((l.body - 17.0).abs() < 1e-4 && (l.small - 11.0 * 17.0 / 13.0).abs() < 1e-4);
        let back = l.with_font_size(FontSize::Small);
        assert!((back.body - 13.0).abs() < 1e-4 && (back.empty_title - 18.0).abs() < 1e-4);
        assert_eq!(back.metrics, t.metrics, "metrics are scaled by UI Scale, not the font size");
        let broken = Tokens { font_scale: f32::NAN, ..t };
        assert!((broken.with_font_size(FontSize::Medium).body - 15.0).abs() < 1e-4, "a bad stored factor counts as 1");
    }

    #[test]
    fn ui_scale_follows_the_system_with_the_4k_rule() {
        assert_eq!(UiScale::Auto.factor(1.0, None), 1.0);
        assert_eq!(UiScale::Auto.factor(1.5, Some(1920)), 1.5);
        assert_eq!(UiScale::Auto.factor(1.0, Some(3840)), 2.0, "4K at 100 %");
        assert_eq!(UiScale::Auto.factor(1.0, Some(3839)), 1.0);
        assert_eq!(UiScale::Auto.factor(1.25, Some(3840)), 1.25, "the system already scales");
        assert_eq!(UiScale::Auto.factor(f32::NAN, Some(3840)), 2.0, "a bad system scale counts as 100 %");
        assert_eq!(UiScale::Auto.factor(-3.0, None), 1.0);
        assert_eq!(UiScale::Percent(150).factor(2.0, Some(3840)), 1.5, "a fixed choice ignores the system");
        assert_eq!(UiScale::Percent(0).factor(1.0, None), 0.75);
        assert_eq!(UiScale::Percent(u16::MAX).factor(1.0, None), 3.0);
        assert_eq!(UiScale::Percent(110).factor(1.0, None), 1.0, "snapped to 25 % steps");
        assert_eq!(UiScale::Percent(113).factor(1.0, None), 1.25);
    }

    #[test]
    fn get_returns_the_applied_tokens() {
        let ctx = Context::default();
        assert_eq!(Tokens::get(&ctx), Tokens::DARK);
        let big = Tokens::DARK.with_font_size(FontSize::Large);
        big.apply(&ctx);
        assert_eq!(Tokens::get(&ctx), big);
        assert!(!ctx.global_style().interaction.selectable_labels, "labels take no clicks");
    }

    #[test]
    fn over_blends_like_the_rasterizer() {
        assert_eq!(over(Color32::from_black_alpha(140), Color32::WHITE), Color32::from_gray(115));
        assert_eq!(over(Color32::WHITE, Color32::BLACK), Color32::WHITE);
        assert_eq!(over(Color32::TRANSPARENT, grey(0x2d)), grey(0x2d));
    }

    /// `design-system.md` §9: text on `chrome` (dim text included) and captions on `grid_bg` stay
    /// ≥ 4.5:1 (WCAG AA).
    /// Every other text/background pair is in `tests/contrast.rs`.
    #[test]
    fn text_contrast_is_at_least_4_5_to_1() {
        let t = Tokens::default();
        for (name, fg, bg) in [
            ("text on chrome", t.text, t.chrome),
            ("text_label on chrome", t.text_label, t.chrome),
            ("text_dim on chrome", t.text_dim, t.chrome),
            ("text on canvas", t.text, t.canvas),
            ("text_label on canvas", t.text_label, t.canvas),
            ("text_dim on grid_bg", t.text_dim, t.grid_bg),
            ("text_dim on canvas", t.text_dim, t.canvas),
            ("star on black", t.star, Color32::BLACK),
        ] {
            let c = contrast(fg, bg);
            assert!(c >= 4.5, "{name}: {c:.2}:1");
        }
        // The formula itself: black on white is 21:1, a colour on itself 1:1.
        assert!((contrast(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 1e-9);
        assert!((contrast(t.chrome, t.chrome) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn histogram_overlaps_add_the_channel_colours() {
        for (bit, c) in HISTOGRAM.iter().enumerate() {
            assert_eq!(histogram_fill(1 << bit), c.gamma_multiply(HISTOGRAM_FILL_ALPHA));
        }
        // Red and green add; all three saturate to white.
        let rg = Color32::from_rgb(0xff, 0x39 + 0xb0, 0x39 + 0x72);
        assert_eq!(histogram_fill(3), rg.gamma_multiply(HISTOGRAM_FILL_ALPHA));
        assert_eq!(histogram_fill(7), Color32::WHITE.gamma_multiply(HISTOGRAM_FILL_ALPHA));
        // Bits above the three channels are ignored.
        assert_eq!(histogram_fill(0xf9), histogram_fill(1));
    }
}
