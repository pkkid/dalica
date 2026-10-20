//! List rows (`design-system.md` §6, `menus-and-shortcuts.md` §2.0): one row of a list with
//! columns, such as the shortcut editor's (the command's menu path and label, its context, its key
//! cells, the ↺ button). Every row of a list is given the same column widths, so the columns line
//! up whatever their text and whatever the font size.

use egui::{Align, Color32, CornerRadius, Id, Layout, Rect, Response, Sense, Ui, UiBuilder, WidgetType, pos2, vec2};

use super::{Target, access_row, focus_ring, one_line, register};
use crate::Tokens;
use crate::fonts;
use crate::registry::Role;

/// A list row's height at the Small font size; it grows with a larger font.
pub const LIST_ROW_H: f32 = 24.0;
/// The gutter at a row's left that holds the modified mark; the first column starts after it.
pub const LIST_ROW_GUTTER: f32 = 14.0;
/// The space between two columns (the text of one column ends this far before the next).
pub const LIST_COLUMN_GAP: f32 = 8.0;
/// The modified mark's diameter.
pub const MODIFIED_DOT: f32 = 6.0;

/// One cell of a [`list_row`], as given to the caller's cell painter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ListCell {
    /// The column's index.
    pub column: usize,
    /// The column's rect within the row (its full height).
    pub rect: Rect,
    /// The colour of the row's text in its state: `text_label` at rest, `text` hovered or selected,
    /// `text_disabled` disabled.
    pub color: Color32,
    /// The row is modified: its first column is drawn SemiBold.
    pub strong: bool,
}

/// The height of a list row: [`LIST_ROW_H`], taller when the body font needs it.
pub fn list_row_height(t: &Tokens) -> f32 {
    LIST_ROW_H.max((t.body * 1.25 + 6.0).ceil())
}

/// The rects of `widths`' columns in a row at `rect`: after the [`LIST_ROW_GUTTER`], one after
/// another; a width that is not a finite positive number takes what is left of the row (so give
/// it to the last column), and no column passes the row's right edge.
pub fn list_columns(rect: Rect, widths: &[f32]) -> Vec<Rect> {
    let mut x = rect.min.x + LIST_ROW_GUTTER;
    widths
        .iter()
        .map(|&w| {
            let left = (rect.max.x - x).max(0.0);
            let w = if w.is_finite() && w > 0.0 { w.min(left) } else { left };
            let col = Rect::from_min_size(pos2(x, rect.min.y), vec2(w, rect.height()));
            x += w;
            col
        })
        .collect()
}

/// One row of a list (`design-system.md` §6): the full width of `ui` × [`list_row_height`], the
/// whole row clickable (it registers `id`, role `other`, labelled `label`, active when
/// `selected`). Rest no fill; hovered `hover`; selected `pressed` (both radius 4). A modified row
/// has a 6 pt `text` dot centred in the [`LIST_ROW_GUTTER`] and its first column SemiBold. The
/// columns are laid out by [`list_columns`]; `cell` is called for each with a ui confined to it
/// (left to right, centred vertically) and the cell's colour, and may add text ([`list_text`]) or
/// widgets of its own (a key cell, a button), which take their own clicks before the row's.
#[allow(clippy::too_many_arguments)] // the row's look (id, label, columns, selected, modified) plus `ui`, the tokens and the cells
pub fn list_row(
    ui: &mut Ui,
    t: &Tokens,
    id: &str,
    label: &str,
    widths: &[f32],
    selected: bool,
    modified: bool,
    mut cell: impl FnMut(&mut Ui, &ListCell),
) -> Response {
    let enabled = ui.is_enabled();
    let width = ui.available_width();
    let width = if width.is_finite() { width.max(0.0) } else { 0.0 };
    let (_, rect) = ui.allocate_space(vec2(width, list_row_height(t)));
    let r = ui.interact(rect, Id::new(id), if enabled { Sense::click() } else { Sense::hover() });
    let hot = enabled && r.contains_pointer();
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, CornerRadius::same(t.radius), t.pressed);
    } else if hot {
        p.rect_filled(rect, CornerRadius::same(t.radius), t.hover);
    }
    let color = if !enabled {
        t.text_disabled
    } else if selected || hot {
        t.text
    } else {
        t.text_label
    };
    if modified {
        p.circle_filled(
            pos2(rect.min.x + LIST_ROW_GUTTER / 2.0, rect.center().y),
            MODIFIED_DOT / 2.0,
            if enabled { t.text } else { t.text_disabled },
        );
    }
    for (column, col) in list_columns(rect, widths).into_iter().enumerate() {
        if col.width() <= 0.0 {
            continue;
        }
        let info = ListCell { column, rect: col, color, strong: modified && column == 0 };
        let inner = Rect::from_min_max(col.min, pos2((col.max.x - LIST_COLUMN_GAP).max(col.min.x), col.max.y));
        // A child that leaves the row's own layout alone: the row has its height already.
        let mut child = ui.new_child(UiBuilder::new().max_rect(inner).layout(Layout::left_to_right(Align::Center)));
        child.set_clip_rect(inner.intersect(ui.clip_rect()));
        cell(&mut child, &info);
    }
    focus_ring(ui.painter(), t, &r, rect.shrink(1.0), t.radius);
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::SelectableLabel, enabled, selected, label));
    access_row(&r, egui::accesskit::Role::ListBoxOption, selected, None, None);
    register(ui.ctx(), id, Role::Other, label, Target::Widget(&r), selected);
    r
}

/// `text` in a list cell: 13 pt (SemiBold when the cell is `strong`), in the cell's colour or
/// `text_dim` when `dim` (and not hovered or selected), on one line, cut short with "…" at the
/// width left. Text, not a control. Returns its rect and whether it was cut.
pub fn list_text(ui: &mut Ui, t: &Tokens, cell: &ListCell, text: &str, dim: bool) -> (Rect, bool) {
    let font = if cell.strong { fonts::semibold(t.body) } else { fonts::regular(t.body) };
    let color = if dim && cell.color == t.text_label { t.text_dim } else { cell.color };
    let max = ui.available_width();
    let galley = one_line(ui.painter(), text, font, color, if max.is_finite() { max } else { cell.rect.width() });
    let elided = galley.elided;
    let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
    ui.painter().galley(rect.min.round(), galley, color);
    (rect, elided)
}
