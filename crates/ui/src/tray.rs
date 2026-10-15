//! The Photo Tray's model (`ui-layout.md` §6.2–§6.3), as plain functions of the view state so the
//! tray commands, the bottom area and the tests share one definition.
//!
//! - **What the tray holds**: the photos held by Hold ([`Tray::held`](crate::state::Tray::held), in
//!   the order held) and the light table's selection, each photo once ([`photos`], [`count`]).
//! - **How it shows them**: a section selected as a unit is one **token** ("Folder selected — 48
//!   photos", [`token`], [`token_label`]) unless all its photos are held; every other photo is a
//!   square, centre-cropped **mini thumbnail**, held photos first ([`thumbs`]). The minis shrink as
//!   their count grows ([`minis`]): 54 pt in one row, then 48, 42, 36 and 30, 28 in two rows, 22 and
//!   18 in three; what does not fit is counted on the last place ("+12").
//! - **The tray's own selection** ([`Tray::selected`](crate::state::Tray::selected)) is a subset of
//!   what it shows; [`prune`] drops what it no longer shows.
//!
//! The functions the bottom area calls each frame look photos up in O(1) ([`Selection::contains`],
//! [`Tray::holds`]) and walk no more of the selection than the minis shown; [`photos`] lists them
//! all, for an action.
//!
//! [`Selection::contains`]: crate::state::Selection::contains
//! [`Tray::holds`]: crate::state::Tray::holds

use dalica_core::PhotoId;
use egui::{Rect, Vec2, pos2, vec2};

use crate::state::{Section, UiState};

/// The automation ids of the token, of a mini thumbnail (`tray:item:<PhotoId>`) and of the
/// overflow count (`control-protocol.md` §7).
pub const TOKEN_ID: &str = "tray:token";
pub const MORE_ID: &str = "tray:more";

/// The automation id of the mini thumbnail of photo `id`.
pub fn item_id(id: PhotoId) -> String {
    format!("tray:item:{}", id.get())
}

/// The status-line message of an action that needs photos while the tray is empty.
pub const EMPTY: &str = "Select some photos first.";

/// The question Clear asks before it empties a tray holding held photos, and its buttons.
pub const CLEAR_QUESTION: &str = "Clear all photos from the tray?";
pub const CLEAR_TITLE: &str = "Clear Tray";

/// Whether Clear with no photos named asks first: it would empty a tray that holds held photos
/// (none of the photos it shows is tray-selected, so everything goes). `tray.clear` asks through the
/// dialog host (`crate::dialogs`): the Clear button and an unconfirmed run open the question,
/// `engine.execute` answers `needs_confirm` (`control-protocol.md` §5.5).
pub fn clear_asks(state: &UiState) -> bool {
    !state.tray.held.is_empty() && !state.tray.selected.iter().any(|id| shows(state, *id))
}

/// Inside the tray box: the margin around the minis and the gap between them (points).
pub const PAD: f32 = 4.0;
pub const GAP: f32 = 3.0;

/// One step of the mini-thumbnail ladder: the edge, the rows it may use, and the count from which
/// it is the first choice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    pub size: f32,
    pub rows: usize,
    pub from: usize,
}

/// The ladder (`ui-layout.md` §6.2), largest first.
pub const LADDER: [Step; 8] = [
    Step { size: 54.0, rows: 1, from: 0 },
    Step { size: 48.0, rows: 1, from: 12 },
    Step { size: 42.0, rows: 1, from: 14 },
    Step { size: 36.0, rows: 1, from: 16 },
    Step { size: 30.0, rows: 1, from: 18 },
    Step { size: 28.0, rows: 2, from: 20 },
    Step { size: 22.0, rows: 3, from: 40 },
    Step { size: 18.0, rows: 3, from: 67 },
];

/// How the minis are laid out in an area: their edge, rows and columns, and how many places that
/// gives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Minis {
    pub size: f32,
    pub rows: usize,
    pub cols: usize,
}

impl Minis {
    /// The places: rows × columns.
    pub fn capacity(&self) -> usize {
        self.rows.saturating_mul(self.cols)
    }

    /// The square of place `k` (row by row) in `area`, the rows centred vertically.
    pub fn place(&self, area: Rect, k: usize) -> Option<Rect> {
        let cols = self.cols.max(1);
        if k >= self.capacity() {
            return None;
        }
        let (row, col) = (k / cols, k % cols);
        let pitch = self.size + GAP;
        let height = self.rows as f32 * pitch - GAP;
        let top = area.min.y + ((area.height() - height) / 2.0).max(0.0);
        let min = pos2(area.min.x + PAD + col as f32 * pitch, top + row as f32 * pitch);
        Some(Rect::from_min_size(pos2(min.x.round(), min.y.round()), Vec2::splat(self.size)))
    }
}

/// The columns of minis `size` wide that fit in `width` (with the margins).
fn cols(width: f32, size: f32) -> usize {
    let room = width - 2.0 * PAD + GAP;
    if !room.is_finite() || room <= 0.0 { 0 } else { (room / (size + GAP)).floor() as usize }
}

/// The rows of minis `size` high that fit in `height` (with the margins), at most `rows`.
fn rows(height: f32, size: f32, rows: usize) -> usize {
    let room = height - 2.0 * PAD + GAP;
    if !room.is_finite() || room <= 0.0 { 0 } else { ((room / (size + GAP)).floor() as usize).min(rows) }
}

/// The layout of `count` minis in an area `area` big: the ladder's step for the count, or a smaller
/// one when it does not hold them all, down to the smallest (whose places then hold as many as
/// fit). Only the rows the minis need are used, so they stay centred in the box.
pub fn minis(count: usize, area: Vec2) -> Minis {
    let first = LADDER.iter().rposition(|s| count >= s.from).unwrap_or(0);
    let fit = |s: &Step| Minis { size: s.size, rows: rows(area.y, s.size, s.rows), cols: cols(area.x, s.size) };
    let mut last = Minis { size: 18.0, rows: 0, cols: 0 };
    for step in LADDER.iter().skip(first) {
        last = fit(step);
        if last.capacity() >= count {
            let needed = count.div_ceil(last.cols.max(1)).max(1);
            return Minis { rows: last.rows.min(needed), ..last };
        }
    }
    last
}

/// The section shown as one token: the selection's, when it was selected as a unit and holds a
/// photo that is not held.
pub fn token(state: &UiState) -> Option<Section> {
    let sel = &state.selection;
    let section = sel.section.filter(|_| sel.whole && !sel.ids.is_empty())?;
    sel.ids.iter().any(|id| !state.tray.holds(*id)).then_some(section)
}

/// The token's pill: "Folder selected — 48 photos" (one photo: "1 photo").
pub fn token_label(section: Section, count: usize) -> String {
    let kind = match section {
        Section::Folder(_) => "Folder",
        Section::Album(_) => "Album",
    };
    let noun = if count == 1 { "photo" } else { "photos" };
    format!("{kind} selected — {count} {noun}")
}

/// How many photos the tray holds: the held ones and the selection's, each once. The shorter list
/// is looked up in the other.
pub fn count(state: &UiState) -> usize {
    let (tray, sel) = (&state.tray, &state.selection);
    let both = if tray.held.len() <= sel.ids.len() {
        tray.held.iter().filter(|id| sel.contains(**id)).count()
    } else {
        sel.ids.iter().filter(|id| tray.holds(**id)).count()
    };
    tray.held.len().saturating_add(sel.ids.len()).saturating_sub(both)
}

/// Whether the tray is empty.
pub fn is_empty(state: &UiState) -> bool {
    state.tray.held.is_empty() && state.selection.ids.is_empty()
}

/// Every photo in the tray, held ones first, each once: what the tray's actions act on.
pub fn photos(state: &UiState) -> Vec<PhotoId> {
    let mut out = Vec::with_capacity(count(state));
    out.extend_from_slice(&state.tray.held);
    out.extend(state.selection.ids.iter().copied().filter(|id| !state.tray.holds(*id)));
    out
}

/// Whether the tray shows photo `id` (as a mini thumbnail or inside the token).
pub fn shows(state: &UiState, id: PhotoId) -> bool {
    state.tray.holds(id) || state.selection.contains(id)
}

/// The photos shown as mini thumbnails, held first: at most `limit` of them, and how many there
/// are in all.
pub fn thumbs(state: &UiState, limit: usize) -> (Vec<PhotoId>, usize) {
    let mut out: Vec<PhotoId> = state.tray.held.iter().copied().take(limit).collect();
    if token(state).is_some() {
        return (out, state.tray.held.len());
    }
    let room = limit.saturating_sub(out.len());
    out.extend(state.selection.ids.iter().copied().filter(|id| !state.tray.holds(*id)).take(room));
    (out, count(state))
}

/// Drops from the tray's own selection what the tray no longer shows.
pub fn prune(state: &mut UiState) {
    if state.tray.selected.is_empty() {
        return;
    }
    let (tray, sel) = (&mut state.tray, &state.selection);
    let keep: Vec<PhotoId> = tray.selected.iter().copied().filter(|id| tray.holds(*id) || sel.contains(*id)).collect();
    tray.selected = keep;
}

/// The tray box's parts: the token (cover and pill) at its left when there is one, and the area
/// of the minis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Parts {
    /// The token's cover square and its pill.
    pub token: Option<(Rect, Rect)>,
    pub minis: Rect,
}

/// The token's cover edge and the gap between it and its pill.
pub const COVER: f32 = 54.0;
const PILL_GAP: f32 = 6.0;
pub const PILL_H: f32 = 20.0;

/// Splits the tray box: a token of pill width `pill_w` at its left, the minis to its right.
pub fn parts(tray: Rect, pill_w: Option<f32>) -> Parts {
    let Some(pill_w) = pill_w.filter(|w| w.is_finite()) else { return Parts { token: None, minis: tray } };
    let cover = Rect::from_min_size(pos2(tray.min.x + PAD, (tray.center().y - COVER / 2.0).round()), Vec2::splat(COVER));
    let pill_w = pill_w.clamp(0.0, (tray.max.x - PAD - cover.max.x - PILL_GAP).max(0.0));
    let pill = Rect::from_min_size(pos2(cover.max.x + PILL_GAP, (tray.center().y - PILL_H / 2.0).round()), vec2(pill_w, PILL_H));
    let minis = Rect::from_min_max(pos2((pill.max.x + PILL_GAP - PAD).min(tray.max.x), tray.min.y), tray.max);
    Parts { token: Some((cover, pill)), minis }
}

/// The image rect that fills the square `cell` with a photo `width × height`, centred: drawn
/// clipped to the cell, it is the square centre crop. Unknown sizes fill the square.
pub fn crop(cell: Rect, width: u32, height: u32) -> Rect {
    let aspect = if width > 0 && height > 0 { width as f32 / height as f32 } else { 1.0 };
    let size = if aspect >= 1.0 { vec2(cell.height() * aspect, cell.height()) } else { vec2(cell.width(), cell.width() / aspect) };
    Rect::from_center_size(cell.center(), size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Selection;
    use dalica_core::FolderId;

    fn ids(r: std::ops::RangeInclusive<u64>) -> Vec<PhotoId> {
        r.map(PhotoId::new).collect()
    }

    fn state(held: Vec<PhotoId>, sel: Vec<PhotoId>, whole: bool) -> UiState {
        let mut s = UiState::default();
        s.tray.set_held(held);
        let section = (!sel.is_empty()).then_some(Section::Folder(FolderId::new(1)));
        s.selection = Selection::new(sel, None, section, whole);
        s
    }

    #[test]
    fn the_tray_holds_held_photos_and_the_selection_each_once() {
        let s = state(ids(1..=2), ids(2..=4), false);
        assert_eq!(count(&s), 4);
        assert_eq!(photos(&s), ids(1..=4));
        assert_eq!(thumbs(&s, 3), (ids(1..=3), 4));
        assert!(shows(&s, PhotoId::new(4)) && !shows(&s, PhotoId::new(5)));
        assert!(is_empty(&UiState::default()) && count(&UiState::default()) == 0);
    }

    #[test]
    fn a_whole_section_is_one_token_unless_all_its_photos_are_held() {
        let s = state(ids(99..=99), ids(1..=48), true);
        assert_eq!(token(&s), Some(Section::Folder(FolderId::new(1))));
        assert_eq!(thumbs(&s, 10), (ids(99..=99), 1), "the token's photos are not minis");
        assert_eq!(count(&s), 49);
        let all_held = state(ids(1..=48), ids(1..=48), true);
        assert_eq!(token(&all_held), None);
        assert_eq!(token(&state(vec![], ids(1..=3), false)), None);
        assert_eq!(token_label(Section::Folder(FolderId::new(1)), 48), "Folder selected — 48 photos");
        assert_eq!(token_label(Section::Folder(FolderId::new(1)), 1), "Folder selected — 1 photo");
        let album = Section::Album(crate::state::AlbumSection::Starred);
        assert_eq!(token_label(album, 40), "Album selected — 40 photos");
        let s = UiState { selection: Selection::new(ids(1..=40), None, Some(album), true), ..UiState::default() };
        assert_eq!(token(&s), Some(album), "an album section selected as a unit is one token");
    }

    #[test]
    fn clear_asks_only_when_it_would_drop_held_photos() {
        assert!(!clear_asks(&UiState::default()));
        assert!(!clear_asks(&state(vec![], ids(1..=3), false)), "nothing held");
        let mut s = state(ids(1..=1), ids(2..=3), false);
        assert!(clear_asks(&s));
        s.tray.selected = vec![PhotoId::new(3)];
        assert!(!clear_asks(&s), "only the tray-selected photo goes");
        s.tray.selected = vec![PhotoId::new(9)];
        assert!(clear_asks(&s), "a stale tray selection does not count");
    }

    #[test]
    fn pruning_keeps_only_what_the_tray_shows() {
        let mut s = state(ids(1..=1), ids(5..=6), false);
        s.tray.selected = vec![PhotoId::new(1), PhotoId::new(3), PhotoId::new(6)];
        prune(&mut s);
        assert_eq!(s.tray.selected, [PhotoId::new(1), PhotoId::new(6)]);
    }

    #[test]
    fn minis_shrink_with_the_count_and_never_overflow_the_box() {
        // A box wide enough for the reference's sizes: 54 for 1 … 11, then the ladder.
        let wide = vec2(800.0, 80.0);
        for (n, size, rows) in
            [(1, 54.0, 1), (11, 54.0, 1), (12, 48.0, 1), (14, 42.0, 1), (20, 28.0, 2), (39, 28.0, 2), (40, 22.0, 3), (100, 18.0, 3)]
        {
            let m = minis(n, wide);
            assert_eq!(m.size, size, "{n}");
            assert!(m.rows <= rows && m.rows == n.div_ceil(m.cols).min(rows), "{n}: {m:?}");
            assert!(m.capacity() >= n || size == 18.0, "{n}: {m:?}");
        }
        // Only the rows needed: one mini in a three-row step (a narrow strip beside a token) sits
        // in one row, centred.
        let m = minis(1, vec2(30.0, 80.0));
        assert_eq!((m.size, m.rows, m.cols), (22.0, 1, 1));
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(30.0, 80.0));
        assert_eq!(m.place(area, 0).unwrap().center().y.round(), 40.0);
        // A narrow box steps down until they fit.
        let narrow = vec2(266.0, 80.0);
        assert_eq!(minis(4, narrow).size, 54.0);
        assert_eq!(minis(5, narrow).size, 48.0);
        let m = minis(1000, narrow);
        assert_eq!(m.size, 18.0);
        let area = Rect::from_min_size(pos2(0.0, 0.0), narrow);
        let last = m.place(area, m.capacity() - 1).unwrap();
        assert!(area.contains_rect(last), "{last:?}");
        assert!(m.place(area, m.capacity()).is_none());
        // Hostile areas: no places, never a panic.
        for bad in [vec2(0.0, 0.0), vec2(f32::NAN, 80.0), vec2(-5.0, f32::INFINITY)] {
            assert_eq!(minis(3, bad).capacity(), 0, "{bad:?}");
        }
    }

    #[test]
    fn crops_fill_the_square() {
        let cell = Rect::from_min_size(pos2(0.0, 0.0), vec2(54.0, 54.0));
        assert_eq!(crop(cell, 600, 400).size(), vec2(81.0, 54.0));
        assert_eq!(crop(cell, 400, 600).size(), vec2(54.0, 81.0));
        assert_eq!(crop(cell, 0, 7).size(), vec2(54.0, 54.0));
    }
}
