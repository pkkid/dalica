//! Drawing the tree pane's items (`ui-layout.md` §4, `design-system.md` §3–§4): a group header is
//! ui-kit's `section_header`; a row is its selection or hover fill, the focus bar, the disclosure
//! triangle (tree mode), the 16 pt icon, the name elided in the middle and the count in `text_dim`;
//! a year divider is the year in `text_dim` 11 pt with a 1 pt rule to the right edge.

use dalica_ui_kit::icons::{self, Icon};
use dalica_ui_kit::{Tokens, fonts};
use egui::{FontId, Painter, Rect, pos2, vec2};

use super::model::Row;

/// Where a row's disclosure triangle is centred and where its icon starts, before the indent.
const DISCLOSURE_X: f32 = 12.0;
const ICON_LEFT: f32 = 24.0;
/// The disclosure triangle's edge; the gap after the icon and between name and count; the space
/// kept free at a row's right end.
const DISCLOSURE: f32 = 12.0;
const ICON_GAP: f32 = 6.0;
const COUNT_GAP: f32 = 4.0;
const RIGHT: f32 = 8.0;
/// The width of the focus bar at a focused list's selected row.
const FOCUS_BAR: f32 = 2.0;
/// Where a year divider's label starts and the gap before its rule.
const YEAR_LEFT: f32 = 12.0;
const YEAR_GAP: f32 = 6.0;

/// How a row is drawn this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub selected: bool,
    pub hovered: bool,
    /// The pane has the keyboard focus.
    pub focused: bool,
    /// Indent per tree level.
    pub indent: f32,
}

/// The rect of a row's disclosure triangle (tree mode), whether or not it has one.
pub fn disclosure_rect(row_rect: Rect, depth: usize, indent: f32) -> Rect {
    let x = row_rect.min.x + DISCLOSURE_X + depth as f32 * indent;
    Rect::from_center_size(pos2(x, row_rect.center().y), vec2(row_rect.height(), row_rect.height()))
}

/// `text` elided in the middle with `…` to fit `max_w` in `font`: the whole of it when it fits.
pub fn elide_middle(painter: &Painter, text: &str, font: &FontId, max_w: f32) -> String {
    let width = |s: &str| painter.layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::WHITE).size().x;
    if width(text) <= max_w {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let cut = |keep: usize| -> String {
        let head = keep.div_ceil(2);
        let tail = keep / 2;
        let mut s: String = chars.iter().take(head).collect();
        s.push('…');
        s.extend(chars.iter().skip(chars.len().saturating_sub(tail)));
        s
    };
    // The most characters kept that still fit, by binary search.
    let (mut lo, mut hi) = (0usize, chars.len().saturating_sub(1));
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if width(&cut(mid)) <= max_w { lo = mid } else { hi = mid - 1 }
    }
    cut(lo)
}

/// Paints `row` in `rect`.
pub fn row(painter: &Painter, t: &Tokens, rect: Rect, row: &Row, look: Look, tree_mode: bool) {
    if look.selected {
        painter.rect_filled(rect, 0, t.canvas);
        if look.focused {
            painter.rect_filled(Rect::from_min_size(rect.min, vec2(FOCUS_BAR, rect.height())), 0, t.accent);
        }
    } else if look.hovered {
        painter.rect_filled(rect, 0, t.hover.gamma_multiply(0.6));
    }
    let indent = if tree_mode { row.depth as f32 * look.indent } else { 0.0 };
    if tree_mode && row.has_children {
        let r = disclosure_rect(rect, row.depth, look.indent);
        let glyph = Rect::from_center_size(r.center(), vec2(DISCLOSURE, DISCLOSURE));
        icons::paint(painter, glyph, if row.expanded { Icon::ChevronDown } else { Icon::ChevronRight }, t.icon);
    }
    let icon_size = t.metrics.icon_menu;
    let icon = Rect::from_min_size(pos2(rect.min.x + ICON_LEFT + indent, rect.center().y - icon_size / 2.0), vec2(icon_size, icon_size));
    icons::paint(painter, icon, Icon::Folder, t.icon);
    // The count right after the name; the name gives way, elided in the middle.
    let count = painter.layout_no_wrap(format!("({})", row.count), fonts::regular(t.count), t.text_dim);
    let x = icon.max.x + ICON_GAP;
    let room = (rect.max.x - RIGHT - x - count.size().x - COUNT_GAP).max(1.0);
    let font = fonts::regular(t.body);
    let name = elide_middle(painter, &row.name, &font, room);
    let color = if look.selected { t.text } else { t.text_label };
    let name = painter.layout_no_wrap(name, font, color);
    let name_w = name.size().x;
    painter.galley(pos2(x, rect.center().y - name.size().y / 2.0).round(), name, color);
    painter.galley(pos2(x + name_w + COUNT_GAP, rect.center().y - count.size().y / 2.0).round(), count, t.text_dim);
}

/// Paints a year divider in `rect`.
pub fn year(painter: &Painter, t: &Tokens, rect: Rect, year: u16) {
    let label = painter.layout_no_wrap(year.to_string(), fonts::regular(t.small), t.text_dim);
    let x = rect.min.x + YEAR_LEFT;
    let label_w = label.size().x;
    painter.galley(pos2(x, rect.center().y - label.size().y / 2.0).round(), label, t.text_dim);
    let rule_left = x + label_w + YEAR_GAP;
    if rule_left < rect.max.x {
        let y = rect.center().y.round();
        painter.rect_filled(Rect::from_min_max(pos2(rule_left, y), pos2(rect.max.x, y + 1.0)), 0, t.divider);
    }
}
