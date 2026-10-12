//! The selection model (`ui-layout.md` §5.4): what each gesture and key does to the light table's
//! [`Selection`], as plain functions of the section's photos in display order and the grid's
//! columns, so the select commands, the light table's input and the tests share one definition.
//!
//! - A selection never crosses a section: every function here works inside one section's photos
//!   ([`Scope`]), and selecting in another section replaces the selection.
//! - The **anchor** is the focused photo (white outline). A Shift range runs from the anchor to the
//!   photo asked for and keeps the anchor; its ids are listed from the anchor to that end, so the
//!   end a Shift+arrow moves is the last id ([`range_end`]).
//! - Arrows, Home and End stop at the section's ends; up and down move by one row of the grid.
//! - The **current section** ([`Table::current`]) is the one the selection is in, else the folder
//!   selected in the tree, else the one at the top of the view, else the first.
//! - **Hidden photos** are not in the sections unless View ▸ Hidden Pictures is on, so no gesture
//!   selects them; photos hidden while selected leave the selection ([`without_hidden`]), so nothing
//!   acts on photos the user cannot see.

use std::collections::HashSet;

use dalica_core::{FolderId, PhotoId};
use dalica_library::Library;

use crate::app::App;
use crate::panels::light_table::layout::Layout;
use crate::panels::light_table::{Sections, tree_options};
use crate::state::{ScrollAnchor, Section, Selection, UiState};

/// One section as the selection sees it: its photos in display order and the grid's columns.
#[derive(Debug, Clone, Copy)]
pub struct Scope<'a> {
    pub section: Section,
    pub folder: FolderId,
    pub photos: &'a [PhotoId],
    /// Columns of the grid, at least 1.
    pub cols: usize,
}

impl Scope<'_> {
    /// Where `id` is in the section.
    pub fn index(&self, id: PhotoId) -> Option<usize> {
        self.photos.iter().position(|&p| p == id)
    }

    /// A selection of the photos at `ids` (in that order) with `anchor`, in this section.
    fn selection(&self, ids: Vec<PhotoId>, anchor: Option<PhotoId>) -> Selection {
        let section = (!ids.is_empty()).then_some(self.section);
        let anchor = if ids.is_empty() { None } else { anchor };
        Selection::new(ids, anchor, section, false)
    }
}

/// An arrow, Home or End.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Left,
    Right,
    Up,
    Down,
    /// Home: the section's first photo.
    First,
    /// End: the section's last photo.
    Last,
}

impl Step {
    pub const ALL: [Step; 6] = [Step::Left, Step::Right, Step::Up, Step::Down, Step::First, Step::Last];

    /// Where this step goes from photo `k` of a section of `count` photos in `cols` columns; it
    /// stops at the section's ends. Down from a row above a shorter last row lands on its last photo.
    pub fn from(self, k: usize, count: usize, cols: usize) -> usize {
        let last = count.saturating_sub(1);
        let cols = cols.max(1);
        let k = k.min(last);
        match self {
            Step::Left => k.saturating_sub(1),
            Step::Right => (k + 1).min(last),
            Step::Up => k.checked_sub(cols).unwrap_or(k),
            Step::Down if k / cols < last / cols => k.saturating_add(cols).min(last),
            Step::Down => k,
            Step::First => 0,
            Step::Last => last,
        }
    }
}

/// The photos from `from` to `to` (both included), listed from `from`; `None` when either is not
/// in `photos`.
pub fn range(photos: &[PhotoId], from: PhotoId, to: PhotoId) -> Option<Vec<PhotoId>> {
    let a = photos.iter().position(|&p| p == from)?;
    let b = photos.iter().position(|&p| p == to)?;
    Some(if a <= b { photos.get(a..=b)?.to_vec() } else { photos.get(b..=a)?.iter().rev().copied().collect() })
}

/// The end a Shift+arrow moves: the last id when the selection is exactly the range from its
/// anchor to that id in this section, else the anchor.
pub fn range_end(sel: &Selection, scope: &Scope<'_>) -> Option<PhotoId> {
    let anchor = sel.anchor.filter(|_| sel.section == Some(scope.section))?;
    let last = sel.ids.last().copied()?;
    match range(scope.photos, anchor, last) {
        Some(r) if r == sel.ids => Some(last),
        _ => Some(anchor),
    }
}

/// Just `id` (a click, a right-click on an unselected photo).
pub fn one(scope: &Scope<'_>, id: PhotoId) -> Selection {
    scope.selection(vec![id], Some(id))
}

/// A Shift-click on `to`: the range from the anchor when the anchor is in this section, else `to`
/// alone. The anchor stays.
pub fn shift_click(sel: &Selection, scope: &Scope<'_>, to: PhotoId) -> Selection {
    let anchor = sel.anchor.filter(|_| sel.section == Some(scope.section));
    match anchor.and_then(|a| Some((a, range(scope.photos, a, to)?))) {
        Some((a, ids)) => scope.selection(ids, Some(a)),
        None => one(scope, to),
    }
}

/// An arrow, Home or End; with `extend` (Shift) the range from the anchor to the moved end. With
/// nothing selected in the section, Home and the arrows select its first photo and End its last.
/// `None` for an empty section.
pub fn moved(sel: &Selection, scope: &Scope<'_>, step: Step, extend: bool) -> Option<(Selection, PhotoId)> {
    let count = scope.photos.len();
    let first = *scope.photos.first()?;
    let here = |id: Option<PhotoId>| id.and_then(|id| scope.index(id));
    let anchor = sel.anchor.filter(|_| sel.section == Some(scope.section));
    let Some(a) = here(anchor) else {
        let id = if step == Step::Last { *scope.photos.last()? } else { first };
        return Some((one(scope, id), id));
    };
    if extend {
        let end = here(range_end(sel, scope)).unwrap_or(a);
        let to = *scope.photos.get(step.from(end, count, scope.cols))?;
        let anchor = *scope.photos.get(a)?;
        let ids = range(scope.photos, anchor, to)?;
        Some((scope.selection(ids, Some(anchor)), to))
    } else {
        let to = *scope.photos.get(step.from(a, count, scope.cols))?;
        Some((one(scope, to), to))
    }
}

/// Every photo of the section (Ctrl+A); the anchor stays when it is in the section, else the first.
pub fn all(sel: &Selection, scope: &Scope<'_>) -> Selection {
    let anchor = sel.anchor.filter(|a| sel.section == Some(scope.section) && scope.index(*a).is_some());
    scope.selection(scope.photos.to_vec(), anchor.or_else(|| scope.photos.first().copied()))
}

/// The section's photos that are not selected (Ctrl+I); a selection elsewhere counts as none.
pub fn invert(sel: &Selection, scope: &Scope<'_>) -> Selection {
    let selected: HashSet<PhotoId> = if sel.section == Some(scope.section) { sel.ids.iter().copied().collect() } else { HashSet::new() };
    let ids: Vec<PhotoId> = scope.photos.iter().copied().filter(|p| !selected.contains(p)).collect();
    let anchor = ids.first().copied();
    scope.selection(ids, anchor)
}

/// The section's starred photos, in display order; `None` when it has none.
pub fn starred(scope: &Scope<'_>, library: &Library) -> Option<Selection> {
    let ids: Vec<PhotoId> = scope.photos.iter().copied().filter(|&id| library.photo(id).is_some_and(|p| p.star)).collect();
    let anchor = ids.first().copied()?;
    Some(scope.selection(ids, Some(anchor)))
}

/// The section as a unit (a tree row, `select.section`): all its photos, marked `whole`.
pub fn whole(scope: &Scope<'_>) -> Selection {
    let anchor = scope.photos.first().copied();
    Selection::new(scope.photos.to_vec(), anchor, Some(scope.section), true)
}

/// The photos of section `index` a rubber band touches: `x` from the light table's left edge, `y`
/// in content coordinates. Only that section's cells are tested, row by row inside the band, so the
/// work is the band's cells, never the library's.
pub fn band(layout: &Layout, index: usize, x: (f32, f32), y: (f64, f64)) -> Vec<usize> {
    let count = layout.count(index);
    let (Some(top), true) = (layout.grid_top(index), count > 0) else { return Vec::new() };
    let grid = layout.grid();
    let (x0, x1) = (x.0.min(x.1), x.0.max(x.1));
    let (y0, y1) = (y.0.min(y.1), y.0.max(y.1));
    if !(x0.is_finite() && x1.is_finite() && y0.is_finite() && y1.is_finite()) {
        return Vec::new();
    }
    let cols: Vec<usize> = (0..grid.cols.max(1)).filter(|&c| grid.x_of(c) < x1 && grid.x_of(c) + grid.cell > x0).collect();
    let (pitch, cell) = (layout.row_pitch(), f64::from(grid.cell));
    let rows = layout.rows(count);
    if cols.is_empty() || pitch <= 0.0 || rows == 0 {
        return Vec::new();
    }
    // The rows whose cells reach into y0..y1: from the first whose bottom is below y0 to the last
    // whose top is above y1.
    let first = ((y0 - top - cell) / pitch).floor().max(-1.0) + 1.0;
    let last = ((y1 - top) / pitch).ceil() - 1.0;
    if last < first || last < 0.0 {
        return Vec::new();
    }
    let (first, last) = (first as usize, (last as usize).min(rows - 1));
    let mut out = Vec::new();
    for r in first..=last {
        let row_top = top + r as f64 * pitch;
        if row_top >= y1 || row_top + cell <= y0 {
            continue;
        }
        out.extend(cols.iter().map(|c| r * grid.cols.max(1) + c).filter(|&k| k < count));
    }
    out
}

/// Where the light table's sections come from: the panels' (the last frame's) or, before the first
/// frame, a fresh build.
enum Source<'a> {
    Panels(&'a Sections),
    Fresh(Box<Sections>),
}

/// The light table as the selection commands see it: the sections, the last frame's layout and
/// view, and the library.
pub struct Table<'a> {
    sections: Source<'a>,
    layout: Option<&'a Layout>,
    /// The view's offset and height, when a frame has been drawn.
    view: Option<(f64, f32)>,
    pub library: &'a Library,
}

impl<'a> Table<'a> {
    /// The light table of `app`; `None` without a library.
    pub fn of(app: &'a App) -> Option<Table<'a>> {
        let session = app.session()?;
        let library = session.library();
        let Some(p) = app.panels() else {
            let mut fresh = Sections::default();
            fresh.refresh(library, session.roots(), &tree_options(app.state()));
            return Some(Table { sections: Source::Fresh(Box::new(fresh)), layout: None, view: None, library });
        };
        let lt = &p.light_table;
        let layout = lt.layout().filter(|l| l.key().sections == p.sections.generation());
        let view = lt.frame_info().area.map(|a| (lt.offset(), a.height()));
        Some(Table { sections: Source::Panels(&p.sections), layout, view, library })
    }

    fn sections(&self) -> &Sections {
        match &self.sections {
            Source::Panels(s) => s,
            Source::Fresh(s) => s,
        }
    }

    /// Section `id` on the light table; `None` when it is not shown.
    pub fn scope(&self, id: Section) -> Option<Scope<'_>> {
        let s = self.sections().get(id)?;
        let cols = self.layout.map_or(1, |l| l.grid().cols.max(1));
        Some(Scope { section: s.id, folder: s.folder, photos: &s.photos, cols })
    }

    /// The section photo `id` is shown in.
    pub fn section_of(&self, id: PhotoId) -> Option<Section> {
        let folder = self.library.photo(id)?.folder;
        let s = Section::Folder(folder);
        self.sections().get(s).is_some_and(|sec| sec.photos.contains(&id)).then_some(s)
    }

    /// The current section: the selection's, else the tree's folder's, else the one at the top of
    /// the view, else the first; `None` when no section is shown.
    pub fn current(&self, state: &UiState) -> Option<Section> {
        let shown = |s: Option<Section>| s.filter(|s| self.sections().get(*s).is_some());
        shown(state.selection.section)
            .or_else(|| shown(state.folder.map(Section::Folder)))
            .or_else(|| shown(state.scroll.section))
            .or_else(|| self.sections().list.first().map(|s| s.id))
    }

    /// The scroll anchor that brings photo `id` of `section` into view, if it is not in view (and a
    /// frame has been drawn): its row just below the pinned header (the section's header for its
    /// first row), or at the bottom of the view.
    pub fn reveal(&self, section: Section, id: PhotoId) -> Option<ScrollAnchor> {
        let (layout, (offset, view_h)) = (self.layout?, self.view?);
        let index = layout.index_of(section)?;
        let k = self.scope(section)?.index(id)?;
        let (_, y) = layout.cell_origin(index, k)?;
        let s = layout.spacing();
        let bottom = y + f64::from(layout.grid().cell) + f64::from(s.row_gap);
        let pinned = f64::from(s.sticky) + f64::from(s.header_gap);
        let view_h = f64::from(view_h);
        let to = if y < offset + pinned {
            if k < layout.grid().cols.max(1) { layout.top(index)? } else { y - pinned }
        } else if bottom > offset + view_h {
            bottom - view_h
        } else {
            return None;
        };
        Some(layout.anchor_at(layout.clamp(to, view_h as f32)))
    }
}

/// Whether photo `id` is out of sight while View ▸ Hidden Pictures is off: hidden itself, or in a
/// hidden folder.
pub fn out_of_sight(library: &Library, id: PhotoId) -> bool {
    library.is_hidden(id) || library.photo(id).is_some_and(|p| library.is_folder_hidden(p.folder))
}

/// The selection without its hidden photos (and the photos of hidden folders), unless View ▸
/// Hidden Pictures (`hidden_pictures`) is on (maintainer decision, M6.24): the anchor moves to the
/// first photo left, and a selection left empty has no section. `None` when nothing would change.
/// One pass over the selection.
pub fn without_hidden(sel: &Selection, hidden_pictures: bool, library: &Library) -> Option<Selection> {
    if hidden_pictures || !sel.ids.iter().any(|&id| out_of_sight(library, id)) {
        return None;
    }
    let ids: Vec<PhotoId> = sel.ids.iter().copied().filter(|&id| !out_of_sight(library, id)).collect();
    let anchor = sel.anchor.filter(|a| ids.contains(a)).or_else(|| ids.first().copied());
    let section = if ids.is_empty() { None } else { sel.section };
    let whole = sel.whole && !ids.is_empty();
    Some(Selection::new(ids, anchor, section, whole))
}

/// Makes `sel` the selection; its section's folder becomes the tree's selected folder.
pub fn commit(state: &mut UiState, sel: Selection) {
    if let Some(Section::Folder(folder)) = sel.section {
        state.folder = Some(folder);
    }
    state.selection = sel;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(n: u64) -> Vec<PhotoId> {
        (1..=n).map(PhotoId::new).collect()
    }

    fn scope(photos: &[PhotoId], cols: usize) -> Scope<'_> {
        Scope { section: Section::Folder(FolderId::new(1)), folder: FolderId::new(1), photos, cols }
    }

    #[test]
    fn steps_stop_at_the_section_ends() {
        // 7 photos in 3 columns: rows 0 1 2 / 3 4 5 / 6.
        for (step, k, want) in [
            (Step::Left, 0, 0),
            (Step::Left, 4, 3),
            (Step::Right, 6, 6),
            (Step::Right, 2, 3),
            (Step::Up, 1, 1),
            (Step::Up, 4, 1),
            (Step::Down, 1, 4),
            (Step::Down, 4, 6),
            (Step::Down, 6, 6),
            (Step::First, 5, 0),
            (Step::Last, 0, 6),
        ] {
            assert_eq!(step.from(k, 7, 3), want, "{step:?} from {k}");
        }
        // Hostile numbers: no columns, an index past the end, an empty section.
        assert_eq!(Step::Down.from(0, 3, 0), 1);
        assert_eq!(Step::Right.from(99, 3, 2), 2);
        for s in Step::ALL {
            assert_eq!(s.from(0, 0, 4), 0);
        }
    }

    #[test]
    fn ranges_run_from_the_anchor() {
        let p = ids(6);
        assert_eq!(range(&p, p[1], p[3]), Some(vec![p[1], p[2], p[3]]));
        assert_eq!(range(&p, p[3], p[1]), Some(vec![p[3], p[2], p[1]]));
        assert_eq!(range(&p, p[2], PhotoId::new(99)), None);
        let s = scope(&p, 3);
        let sel = shift_click(&one(&s, p[4]), &s, p[1]);
        assert_eq!((sel.ids.clone(), sel.anchor), (vec![p[4], p[3], p[2], p[1]], Some(p[4])));
        assert_eq!(range_end(&sel, &s), Some(p[1]));
    }

    #[test]
    fn shift_arrows_grow_and_shrink_the_range() {
        let p = ids(9);
        let s = scope(&p, 3);
        let (sel, _) = moved(&Selection::default(), &s, Step::Right, true).unwrap();
        assert_eq!(sel.ids, vec![p[0]], "nothing selected: the first photo");
        let (sel, _) = moved(&sel, &s, Step::Right, true).unwrap();
        let (sel, end) = moved(&sel, &s, Step::Down, true).unwrap();
        assert_eq!((sel.ids.clone(), sel.anchor, end), (p[0..=4].to_vec(), Some(p[0]), p[4]));
        let (sel, _) = moved(&sel, &s, Step::Left, true).unwrap();
        assert_eq!(sel.ids, p[0..=3].to_vec());
        let (sel, end) = moved(&sel, &s, Step::Right, false).unwrap();
        assert_eq!((sel.ids, end), (vec![p[1]], p[1]), "a plain arrow moves the anchor");
    }

    #[test]
    fn all_invert_and_whole_stay_in_the_section() {
        let p = ids(5);
        let s = scope(&p, 2);
        let sel = one(&s, p[2]);
        let a = all(&sel, &s);
        assert_eq!((a.ids.len(), a.anchor, a.whole), (5, Some(p[2]), false));
        let i = invert(&sel, &s);
        assert_eq!(i.ids, vec![p[0], p[1], p[3], p[4]]);
        assert_eq!(invert(&a, &s), Selection::default(), "inverting everything leaves nothing");
        let elsewhere = Selection::new(sel.ids, sel.anchor, Some(Section::Folder(FolderId::new(2))), sel.whole);
        assert_eq!(invert(&elsewhere, &s).ids, p, "a selection elsewhere counts as none");
        let w = whole(&s);
        assert!(w.whole && w.ids == p);
    }
}
