//! A damaged folder's caution banner (`data-model.md` §3, `ui-layout.md` §5.2): when the folder's
//! sidecar could not be read, Dalica kept it aside and writes nothing into the folder; this banner
//! says so at the top of the folder's section, in its header block, with [Reset folder data].
//!
//! ```text
//! [📁 32]  Lisbon Weekend
//!          Thursday, June 4, 2026   ┃ This folder's data file was damaged and kept aside.   [Reset folder data]
//! [▶] [▦] │ [★] │ [💾] [Export ▾]  ┃ Nothing is written here until you reset it.
//! Add a description
//! ```
//!
//! **Boundary.** The parent calls [`show`] after a folder section's header block ([`Form::Block`]
//! only, never the pinned line) when the session lists the folder as damaged
//! (`Session::damaged_folder`, in memory), with the frame, the block's rect and the top of the
//! view below the pinned header; it paints right-aligned in the block's date and button rows
//! (clear of the description row), and returns whether the button was clicked. The parent hands
//! that to [`apply`] once the library is no longer borrowed. Hidden while any of it would lie under
//! the pinned header, so nothing covered takes a click.
//!
//! **Look** (tokens and `ui-kit` widgets only): `filter_banner` fill with radius `radius`, a
//! [`BAR_W`] `caution` bar at its left edge, two lines (13 pt `text`, then 13 pt `text_dim`, each cut
//! short with "…" where it would reach the button) and the `ui-kit` text button
//! [`RESET_LABEL`] at its right.
//!
//! **Ids** (`control-protocol.md` §7): the banner `banner:folderData/<FolderId>` (role `other`,
//! labelled with its first line) and its button `button:folder.resetData/<FolderId>`, which runs
//! `folder.resetData {folder}`: the command asks first through the dialog host
//! (`crate::dialogs::confirm`, `dialog:resetFolderData` with [Reset] [Cancel]); Cancel changes
//! nothing, Reset runs it with `"confirm": true` and the banner goes once the engine lifts the
//! folder.

use dalica_core::FolderId;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use dalica_ui_kit::{Tokens, fonts};
use egui::text::{LayoutJob, TextWrapping};
use egui::{Align, CornerRadius, Galley, Rect, Ui, UiBuilder, pos2, vec2};
use serde_json::json;

use super::Frame;
use super::header::PADDING;
use crate::app::App;
use crate::tl;

/// What the banner says, in English (drawn through the catalogs).
pub const TITLE: &str = "This folder's data file was damaged and kept aside.";
pub const DETAIL: &str = "Nothing is written here until you reset it or restore the file.";
/// Its button's label.
pub const RESET_LABEL: &str = "Reset folder data";
/// The banner's top in the header block (the date row's), and its height (to just above the
/// description row).
const TOP: f32 = 28.0;
const HEIGHT: f32 = 42.0;
/// Its width: half the block, within these bounds (points).
const MIN_W: f32 = 300.0;
const MAX_W: f32 = 600.0;
/// The `caution` bar at its left edge, the padding inside it and the gap before the button.
pub const BAR_W: f32 = 3.0;
const PAD: f32 = 10.0;
const GAP: f32 = 12.0;

/// The banner's automation id.
pub fn banner_id(folder: FolderId) -> String {
    format!("banner:folderData/{folder}")
}

/// Its button's automation id.
pub fn button_id(folder: FolderId) -> String {
    format!("button:folder.resetData/{folder}")
}

/// The banner's rect in the header block `block`.
pub fn rect(block: Rect) -> Rect {
    let width = (block.width() * 0.5).clamp(MIN_W, MAX_W).min((block.width() - 2.0 * PADDING).max(0.0));
    Rect::from_min_size(pos2(block.max.x - PADDING - width, block.min.y + TOP), vec2(width, HEIGHT))
}

/// What the banner asked for: the folder whose [Reset folder data] was clicked.
#[derive(Debug, Default)]
pub struct BannerOut {
    pub reset: Option<FolderId>,
}

/// One line of `text` at most `width` wide, cut with "…".
fn line(ui: &Ui, t: &Tokens, text: &str, color: egui::Color32, width: f32) -> std::sync::Arc<Galley> {
    let mut job = LayoutJob::simple(text.to_owned(), fonts::regular(t.body), color, width.max(1.0));
    job.wrap = TextWrapping { max_width: width.max(1.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    ui.painter().layout_job(job)
}

/// Paints `folder`'s banner in its header block `block`, when it lies wholly below `visible_top`
/// (the bottom of the pinned header) and inside the view.
pub fn show(ui: &mut Ui, f: &Frame<'_>, folder: FolderId, block: Rect, visible_top: f32) -> BannerOut {
    let t = f.t;
    let r = rect(block);
    if r.width() < 1.0 || r.min.y < visible_top || r.max.y > f.area.max.y {
        return BannerOut::default();
    }
    let p = ui.painter();
    p.rect_filled(r, CornerRadius::same(t.radius), t.filter_banner);
    p.rect_filled(Rect::from_min_size(r.min, vec2(BAR_W, r.height())), CornerRadius::ZERO, t.caution);
    // The button first, at the right, so the text knows how much room it has.
    let (reset, title, detail) = (
        tl!("Reset folder data"),
        tl!("This folder's data file was damaged and kept aside."),
        tl!("Nothing is written here until you reset it or restore the file."),
    );
    let label_w = ui.painter().layout_no_wrap(reset.clone(), fonts::semibold(t.button_label), t.text).size().x;
    let button_w = (label_w + 18.0).max(37.0).round();
    let slot =
        Rect::from_min_size(pos2(r.max.x - PAD - button_w, r.center().y - t.metrics.button_h / 2.0).round(), vec2(button_w, t.metrics.button_h));
    let clicked = ui
        .scope_builder(UiBuilder::new().max_rect(slot).layout(egui::Layout::left_to_right(Align::Center)), |ui| {
            widgets::text_button(ui, t, &button_id(folder), &reset, false).on_hover_text(tl!("Write a new data file for this folder"))
        })
        .inner
        .clicked();
    let left = r.min.x + BAR_W + PAD;
    let width = slot.min.x - GAP - left;
    if width >= 1.0 {
        let title = line(ui, t, &title, t.text, width);
        let detail = line(ui, t, &detail, t.text_dim, width);
        let h = title.size().y + 2.0 + detail.size().y;
        let y = (r.center().y - h / 2.0).round();
        let detail_y = y + title.size().y + 2.0;
        let p = ui.painter();
        p.galley(pos2(left, y), title, t.text);
        p.galley(pos2(left, detail_y), detail, t.text_dim);
    }
    widgets::register(ui.ctx(), &banner_id(folder), Role::Other, &title, Target::Painted { rect: r, clip: ui.clip_rect() }, false);
    BannerOut { reset: clicked.then_some(folder) }
}

/// Runs what the banner asked for: `folder.resetData`, which asks first through the dialog host.
pub fn apply(app: &mut App, out: BannerOut) {
    if let Some(folder) = out.reset {
        let _ = app.run("folder.resetData", &json!({ "folder": folder }));
    }
}
