//! The light table's geometry (`ui-layout.md` §5.1–§5.3, `design-system.md` §4) as plain numbers:
//! no egui widget, no library, no painting, so it is tested alone and benched at any size.
//!
//! The content is one column of sections in tree order. Each section is its **header block**
//! ([`Spacing::header`], ≈ 96), a gap, then a **justified grid** of square cells: as many columns as
//! fit with at least [`Spacing::min_gap`] between them, the rest of the width spread evenly so the
//! columns reach both margins ([`Spacing::margin`]); rows [`Spacing::row_gap`] apart (with captions
//! on, the caption line sits in that gap, directly under its cell); the last row is followed by the
//! [`Spacing::gutter`] before the next header.
//!
//! Offsets are prefix-summed once per [`Layout`] (in `f64`: a 100 k-photo library is millions of
//! points tall), the section at an offset is found by binary search, and the rows on screen by
//! arithmetic, so a frame visits only what it shows whatever the library's size. A [`Layout`] is
//! built for one [`LayoutKey`] (the sections' generation, the width, the cell size, captions) and
//! reused while the key holds.
//!
//! Scroll positions are kept by content ([`ScrollAnchor`]: a section and how far below its top),
//! converted with [`Layout::anchor_at`] and [`Layout::offset_of`], so a resize or a new cell size
//! keeps the same photos in view.

use std::collections::HashMap;
use std::ops::Range;

use dalica_ui_kit::theme::Metrics;

use crate::state::{self, ScrollAnchor, Section};

/// The gap between a header block and its first row (points).
pub const HEADER_GAP: f32 = 12.0;
/// The gutter between a grid's last row and the next header (`ui-layout.md` §5.1).
pub const GUTTER: f32 = 24.0;
/// The pinned (sticky) header's height: its first line only (`ui-layout.md` §5.2).
pub const STICKY: f32 = 32.0;
/// The caption line under a cell when captions are on (10.5 pt text; it sits in the row gap).
pub const CAPTION: f32 = 16.0;

/// The light table's spacing, in points. [`Spacing::from_metrics`] takes the `design-system.md` §4
/// values from the theme; the rest are this module's constants.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spacing {
    /// The header block's height.
    pub header: f32,
    /// Between the header block and the first row.
    pub header_gap: f32,
    /// Left and right margins of the grid.
    pub margin: f32,
    /// The smallest horizontal gap between two columns.
    pub min_gap: f32,
    /// The vertical gap between two rows (room for a caption line).
    pub row_gap: f32,
    /// After a grid's last row.
    pub gutter: f32,
    /// The sticky header's height.
    pub sticky: f32,
    /// The caption line's height, inside the row gap.
    pub caption: f32,
}

impl Spacing {
    pub fn from_metrics(m: &Metrics) -> Spacing {
        Spacing {
            header: m.section_h,
            header_gap: HEADER_GAP,
            margin: m.grid_margin,
            min_gap: m.grid_gap.x,
            row_gap: m.grid_gap.y,
            gutter: GUTTER,
            sticky: STICKY,
            caption: CAPTION.min(m.grid_gap.y),
        }
    }
}

impl Default for Spacing {
    fn default() -> Spacing {
        Spacing::from_metrics(&dalica_ui_kit::Tokens::default().metrics)
    }
}

/// The columns of a grid `width` points wide.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    /// Columns, at least 1.
    pub cols: usize,
    /// The cell edge.
    pub cell: f32,
    /// The horizontal gap between columns (0 with one column).
    pub gap: f32,
    /// The left margin.
    pub margin: f32,
}

impl Grid {
    /// Justified columns of `cell`-edged cells across `width`. Hostile numbers are made safe first:
    /// the cell clamped to its range ([`state::thumb_size`]), a width that is not a finite, positive
    /// number taken as none (one column).
    pub fn new(width: f32, cell: f32, s: &Spacing) -> Grid {
        let cell = state::thumb_size(cell);
        let width = if width.is_finite() { width.max(0.0) } else { 0.0 };
        let inner = (width - 2.0 * s.margin).max(cell);
        let cols = (((inner + s.min_gap) / (cell + s.min_gap)).floor() as usize).max(1);
        let gap = if cols > 1 { (inner - cols as f32 * cell) / (cols - 1) as f32 } else { 0.0 };
        Grid { cols, cell, gap, margin: s.margin }
    }

    /// The left edge of column `col`, from the light table's left edge.
    pub fn x_of(&self, col: usize) -> f32 {
        self.margin + col as f32 * (self.cell + self.gap)
    }
}

/// What a [`Layout`] was built for; a layout is rebuilt when any of it changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutKey {
    /// The sections' generation (bumped each time they are rebuilt: library revision, tree options).
    pub sections: u64,
    /// `f32::to_bits` of the width and the cell edge.
    pub width: u32,
    pub cell: u32,
    pub caption: bool,
}

impl LayoutKey {
    pub fn new(sections: u64, width: f32, cell: f32, caption: bool) -> LayoutKey {
        LayoutKey { sections, width: width.to_bits(), cell: state::thumb_size(cell).to_bits(), caption }
    }
}

/// A section on screen: its index, its top in content coordinates, and the range of its cells
/// (photo indices in the section) whose rows are on screen.
#[derive(Debug, Clone, PartialEq)]
pub struct Visible {
    pub index: usize,
    pub section: Section,
    pub top: f64,
    pub cells: Range<usize>,
}

/// The pinned header: the section scrolled through, and its top relative to the view's top (0
/// while pinned, negative while the next header pushes it up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sticky {
    pub index: usize,
    pub section: Section,
    pub y: f64,
}

/// The geometry of every section for one [`LayoutKey`].
#[derive(Debug, Clone)]
pub struct Layout {
    key: LayoutKey,
    spacing: Spacing,
    grid: Grid,
    caption: bool,
    sections: Vec<Section>,
    counts: Vec<usize>,
    /// `tops[i]` is section `i`'s top; `tops[n]` the content's height.
    tops: Vec<f64>,
    index: HashMap<Section, usize>,
}

impl Layout {
    /// Lays out `sections` (each with its photo count, in order) `width` points wide with
    /// `cell`-edged cells. `generation` names the sections for [`LayoutKey`].
    pub fn new(
        sections: impl IntoIterator<Item = (Section, usize)>,
        generation: u64,
        width: f32,
        cell: f32,
        caption: bool,
        spacing: Spacing,
    ) -> Layout {
        let grid = Grid::new(width, cell, &spacing);
        let mut out = Layout {
            key: LayoutKey::new(generation, width, cell, caption),
            spacing,
            grid,
            caption,
            sections: Vec::new(),
            counts: Vec::new(),
            tops: vec![0.0],
            index: HashMap::new(),
        };
        let mut y = 0.0;
        for (section, count) in sections {
            // A section listed twice keeps its first place.
            if out.index.contains_key(&section) {
                continue;
            }
            out.index.insert(section, out.sections.len());
            out.sections.push(section);
            out.counts.push(count);
            y += out.section_height(count);
            out.tops.push(y);
        }
        out
    }

    pub fn key(&self) -> LayoutKey {
        self.key
    }

    pub fn grid(&self) -> Grid {
        self.grid
    }

    pub fn spacing(&self) -> Spacing {
        self.spacing
    }

    /// Whether captions are on (each cell has a caption line under it).
    pub fn caption(&self) -> bool {
        self.caption
    }

    pub fn len(&self) -> usize {
        self.sections.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }

    /// The content's height.
    pub fn height(&self) -> f64 {
        self.tops.last().copied().unwrap_or(0.0)
    }

    pub fn section(&self, index: usize) -> Option<Section> {
        self.sections.get(index).copied()
    }

    /// Photos in section `index`.
    pub fn count(&self, index: usize) -> usize {
        self.counts.get(index).copied().unwrap_or(0)
    }

    /// Section `index`'s top (its header's top).
    pub fn top(&self, index: usize) -> Option<f64> {
        if index < self.sections.len() { self.tops.get(index).copied() } else { None }
    }

    pub fn index_of(&self, section: Section) -> Option<usize> {
        self.index.get(&section).copied()
    }

    /// Rows of a section of `count` photos.
    pub fn rows(&self, count: usize) -> usize {
        count.div_ceil(self.grid.cols.max(1))
    }

    /// From one row's top to the next.
    pub fn row_pitch(&self) -> f64 {
        f64::from(self.grid.cell) + f64::from(self.spacing.row_gap)
    }

    /// A section's height: header, gap, rows, gutter.
    pub fn section_height(&self, count: usize) -> f64 {
        let s = &self.spacing;
        let rows = self.rows(count) as f64;
        let grid = if rows > 0.0 { f64::from(s.header_gap) + rows * f64::from(self.grid.cell) + (rows - 1.0) * f64::from(s.row_gap) } else { 0.0 };
        f64::from(s.header) + grid + f64::from(s.gutter)
    }

    /// Where section `index`'s first row starts.
    pub fn grid_top(&self, index: usize) -> Option<f64> {
        self.top(index).map(|t| t + f64::from(self.spacing.header) + f64::from(self.spacing.header_gap))
    }

    /// The top-left corner of photo `k` of section `index`: x from the light table's left edge, y in
    /// content coordinates. `None` past the section's photos.
    pub fn cell_origin(&self, index: usize, k: usize) -> Option<(f32, f64)> {
        if k >= self.count(index) {
            return None;
        }
        let cols = self.grid.cols.max(1);
        let (row, col) = (k / cols, k % cols);
        Some((self.grid.x_of(col), self.grid_top(index)? + row as f64 * self.row_pitch()))
    }

    /// The section whose span holds content offset `y` (the first for `y` above the top, the last
    /// for `y` past the bottom); `None` when there are no sections.
    pub fn index_at(&self, y: f64) -> Option<usize> {
        if self.sections.is_empty() {
            return None;
        }
        let y = if y.is_nan() { 0.0 } else { y };
        let n = self.tops.partition_point(|t| *t <= y);
        Some(n.saturating_sub(1).min(self.sections.len() - 1))
    }

    /// The farthest the view `view_h` high scrolls.
    pub fn max_offset(&self, view_h: f32) -> f64 {
        (self.height() - f64::from(view_h.max(0.0))).max(0.0)
    }

    /// `offset` inside `0 ..= max_offset(view_h)`; 0 for a number that is not finite.
    pub fn clamp(&self, offset: f64, view_h: f32) -> f64 {
        if offset.is_finite() { offset.clamp(0.0, self.max_offset(view_h)) } else { 0.0 }
    }

    /// The scroll anchor of content offset `offset`: the section at the top of the view and how far
    /// below its top the view starts; the top of the library (`section: None`) at offset 0.
    pub fn anchor_at(&self, offset: f64) -> ScrollAnchor {
        if !(offset.is_finite() && offset > 0.0) {
            return ScrollAnchor::default();
        }
        match self.index_at(offset).and_then(|i| Some((self.section(i)?, self.top(i)?))) {
            Some((section, top)) => ScrollAnchor { section: Some(section), offset: ((offset - top).max(0.0) as f32).min(state::SCROLL_OFFSET_MAX) },
            None => ScrollAnchor::default(),
        }
    }

    /// The content offset of `anchor`; `None` when its section is not laid out (gone, or in a
    /// collapsed group).
    pub fn offset_of(&self, anchor: &ScrollAnchor) -> Option<f64> {
        let below = if anchor.offset.is_finite() { f64::from(anchor.offset.max(0.0)) } else { 0.0 };
        match anchor.section {
            None => Some(below),
            Some(s) => self.index_of(s).and_then(|i| self.top(i)).map(|top| top + below),
        }
    }

    /// The sections on a view `view_h` high scrolled to `offset`, with the cells of their rows on
    /// screen. Work is proportional to what is on screen, never to the library.
    pub fn visible(&self, offset: f64, view_h: f32) -> Vec<Visible> {
        let mut out = Vec::new();
        let Some(first) = self.index_at(offset) else { return out };
        let offset = if offset.is_finite() { offset } else { 0.0 };
        let bottom = offset + f64::from(view_h.max(0.0));
        let pitch = self.row_pitch();
        // A row is on screen while its cell (and caption line) is.
        let extent = f64::from(self.grid.cell) + if self.caption { f64::from(self.spacing.caption) } else { 0.0 };
        let cols = self.grid.cols.max(1);
        for index in first..self.sections.len() {
            let (Some(top), Some(section), Some(grid_top)) = (self.top(index), self.section(index), self.grid_top(index)) else { break };
            if top >= bottom {
                break;
            }
            let count = self.count(index);
            let rows = self.rows(count);
            // First row whose bottom is below the view's top; first row whose top is below its bottom.
            let from = (((offset - grid_top - extent) / pitch).floor() + 1.0).max(0.0) as usize;
            let to = ((bottom - grid_top) / pitch).ceil().max(0.0) as usize;
            let (from, to) = (from.min(rows), to.min(rows).max(from.min(rows)));
            let cells = (from * cols).min(count)..(to * cols).min(count);
            out.push(Visible { index, section, top, cells });
        }
        out
    }

    /// The pinned header at `offset`: the section whose header has scrolled above the view's top,
    /// pinned at the top ([`Spacing::sticky`] high) and pushed up by the next section's header.
    pub fn sticky(&self, offset: f64) -> Option<Sticky> {
        if !(offset.is_finite() && offset > 0.0) {
            return None;
        }
        let index = self.index_at(offset)?;
        let top = self.top(index)?;
        if top >= offset {
            return None;
        }
        let sticky = f64::from(self.spacing.sticky);
        let y = match self.top(index + 1) {
            Some(next) => (next - offset - sticky).min(0.0),
            None => 0.0,
        };
        if y <= -sticky {
            return None;
        }
        Some(Sticky { index, section: self.section(index)?, y })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dalica_core::FolderId;

    fn folder(n: u64) -> Section {
        Section::Folder(FolderId::new(n))
    }

    #[test]
    fn heights_add_up_and_empty_layouts_are_safe() {
        let s = Spacing::default();
        assert_eq!((s.header, s.margin, s.min_gap, s.row_gap, s.gutter), (96.0, 24.0, 12.0, 22.0, 24.0));
        let l = Layout::new([(folder(1), 4), (folder(2), 0), (folder(1), 9)], 0, 1100.0, 144.0, false, s);
        assert_eq!(l.len(), 2, "a repeated section keeps its first place");
        let cols = l.grid().cols;
        assert_eq!(cols, 6);
        assert_eq!(l.section_height(4), 96.0 + 12.0 + 144.0 + 24.0);
        assert_eq!(l.section_height(7), 96.0 + 12.0 + 2.0 * 144.0 + 22.0 + 24.0);
        assert_eq!(l.section_height(0), 96.0 + 24.0);
        assert_eq!(l.height(), l.section_height(4) + l.section_height(0));
        let empty = Layout::new([], 0, 800.0, 144.0, false, s);
        assert_eq!((empty.height(), empty.index_at(10.0), empty.sticky(10.0), empty.visible(0.0, 500.0)), (0.0, None, None, vec![]));
        assert_eq!(empty.anchor_at(50.0), ScrollAnchor::default());
        assert_eq!(l.index_at(f64::NAN), Some(0));
        assert_eq!(l.clamp(f64::INFINITY, 100.0), 0.0);
    }
}
