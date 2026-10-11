//! A thumbnail cell (`ui-layout.md` §5.3–§5.5, `design-system.md` §7): a square of the cell size
//! with the photo letterboxed in it (never cropped, never rounded) over its drop shadow; the
//! selection's `cell_selected` surround and 2 pt outline (`photo_focus` on the anchor,
//! `photo_selected` on the rest); the hover surround; the badges and video marks
//! ([`widgets::badges`], fed what the model knows); the caption line under the cell, middle-elided;
//! the `caution` "!" of a photo that cannot be shown; and, after the pointer has rested on the cell
//! for [`TOOLTIP_SECONDS`], the tooltip (file name; date taken, pixel size, file size; or why the
//! photo cannot be shown). A hidden photo is drawn at [`HIDDEN_OPACITY`] when View ▸ Hidden
//! Pictures shows it ([`opacity`]).
//!
//! **Boundary.** The parent calls [`show`] once per cell on screen, after the section's header,
//! with the frame, this part's state ([`Cells`]), the [`Thumbnails`] and a [`CellIn`]: the photo,
//! its section, the square on screen and (captions on) the caption line under it. The parent has
//! already left out what is not shown (small pictures unless View ▸ Small Pictures). The cell senses
//! clicks, registers `thumb:<PhotoId>` and its star badge `thumb:<PhotoId>/star`, asks
//! [`Thumbnails::paint`] for the image and returns a [`CellOut`] (its response) for
//! [`super::input`] to act on. What the last pass drew is kept for tests and inspection
//! ([`Cells::shown`], [`Cells::tooltip`]).

use std::collections::{HashMap, HashSet};

use dalica_core::PhotoId;
use dalica_library::Photo;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Badge, Badges, Target};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align2, Id, Painter, Rect, Response, Sense, Stroke, StrokeKind, Ui, vec2};

use super::{Frame, long_date};
use crate::panels::status;
use crate::panels::thumbnails::{Drawn, PhotoRef, Thumbnails};
use crate::state::{self, Caption};

/// How long the pointer rests on a cell before its tooltip shows (seconds, `ui-layout.md` §5.5).
pub const TOOLTIP_SECONDS: f64 = 0.6;
/// The opacity of a hidden photo shown by View ▸ Hidden Pictures (`ui-layout.md` §5.3).
pub const HIDDEN_OPACITY: f32 = 0.4;
/// Lists of ids (the tray's held photos, the selection) up to this long are searched as they are;
/// longer ones are put in a set once a pass, so a cell's lookup stays O(1).
const SCAN: usize = 32;
/// The most fitted captions kept between passes (a screenful is far fewer).
const FITTED_CAP: usize = 4096;

/// The tooltip a cell shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tip {
    /// The file name.
    pub title: String,
    /// Date taken, pixel size and file size; or why the photo cannot be shown.
    pub detail: String,
}

/// What one cell drew in the last pass that drew cells.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    pub photo: PhotoId,
    /// The caption line as drawn (middle-elided); `None` with captions off.
    pub caption: Option<String>,
    /// Drawn as selected (the surround and the outline).
    pub selected: bool,
    /// 1, or [`HIDDEN_OPACITY`] for a hidden photo.
    pub opacity: f32,
    /// What the badges were fed.
    pub badges: Badges,
    /// What [`Thumbnails::paint`] drew.
    pub drawn: Drawn,
    /// The tooltip, when it was showing.
    pub tooltip: Option<Tip>,
}

/// The cell under the pointer and since when.
#[derive(Debug, Clone, Copy)]
struct Hover {
    photo: PhotoId,
    /// The frame clock when the pointer came onto it (or last pressed it).
    since: f64,
    /// The pass that last saw it hovered.
    pass: u64,
}

/// What the cells keep between frames.
#[derive(Debug, Default)]
pub struct Cells {
    /// The egui pass the records below belong to.
    pass: Option<u64>,
    hover: Option<Hover>,
    shown: Vec<Shown>,
    /// The tray's held photos and the selection, as sets when they are long.
    held: IdSet,
    selected: IdSet,
    /// Fitted captions: photo → (full text, width bits, fitted text).
    fitted: HashMap<PhotoId, (String, u32, String)>,
}

impl Cells {
    /// What each cell drew in the last pass that drew cells, in drawing order.
    pub fn shown(&self) -> &[Shown] {
        &self.shown
    }

    /// What `photo`'s cell drew in that pass.
    pub fn get(&self, photo: PhotoId) -> Option<&Shown> {
        self.shown.iter().find(|s| s.photo == photo)
    }

    /// The tooltip showing in that pass, with its photo.
    pub fn tooltip(&self) -> Option<(PhotoId, &Tip)> {
        self.shown.iter().find_map(|s| s.tooltip.as_ref().map(|t| (s.photo, t)))
    }

    /// Starts a new pass on the first cell of one: forgets the last pass's records and refreshes
    /// the held and selected sets.
    fn begin(&mut self, pass: u64, held: &[PhotoId], selected: &[PhotoId]) {
        if self.pass == Some(pass) {
            return;
        }
        self.pass = Some(pass);
        self.shown.clear();
        self.held.refresh(held);
        self.selected.refresh(selected);
        if self.fitted.len() > FITTED_CAP {
            self.fitted.clear();
        }
    }

    /// Whether the pointer has rested on `photo` for [`TOOLTIP_SECONDS`], counting from when it
    /// came onto the cell or last pressed it. Asks for a repaint when the tooltip is still to come.
    fn rested(&mut self, ui: &Ui, photo: PhotoId, response: &Response, pass: u64, now: f64) -> bool {
        if !response.hovered() {
            return false;
        }
        let pressed = response.is_pointer_button_down_on() || response.clicked() || response.dragged();
        let since = match self.hover {
            Some(h) if h.photo == photo && h.pass.saturating_add(1) >= pass && !pressed => h.since,
            _ => now,
        };
        self.hover = Some(Hover { photo, since, pass });
        let waited = now - since;
        if waited.is_finite() && waited + 1e-9 >= TOOLTIP_SECONDS {
            return true;
        }
        let left = if waited.is_finite() { (TOOLTIP_SECONDS - waited).max(0.0) } else { TOOLTIP_SECONDS };
        ui.ctx().request_repaint_after_secs(left as f32);
        false
    }

    /// `text` fitted to `max` points in `font`, middle-elided, remembered per photo.
    fn fit(&mut self, painter: &Painter, photo: PhotoId, text: &str, max: f32, font: &egui::FontId) -> String {
        let bits = max.to_bits();
        if let Some((full, w, fitted)) = self.fitted.get(&photo)
            && full == text
            && *w == bits
        {
            return fitted.clone();
        }
        let fitted = fit_middle(text, max, |s| painter.layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::WHITE).size().x);
        self.fitted.insert(photo, (text.to_owned(), bits, fitted.clone()));
        fitted
    }
}

/// A list of photo ids looked up per cell: searched as it is when short, else through a set
/// rebuilt once a pass when the list changed.
#[derive(Debug, Default)]
struct IdSet {
    list: Vec<PhotoId>,
    set: HashSet<PhotoId>,
}

impl IdSet {
    fn refresh(&mut self, list: &[PhotoId]) {
        if list.len() > SCAN && self.list != list {
            self.list = list.to_vec();
            self.set = list.iter().copied().collect();
        }
    }

    /// Whether `list` (the one last refreshed with) holds `id`.
    fn contains(&self, list: &[PhotoId], id: PhotoId) -> bool {
        if list.len() > SCAN { self.set.contains(&id) } else { list.contains(&id) }
    }
}

/// One cell to paint.
pub struct CellIn<'a> {
    pub photo: &'a Photo,
    pub section: state::Section,
    /// The square on screen.
    pub rect: Rect,
    /// The caption line under it, when captions are on.
    pub caption: Option<Rect>,
}

/// What a cell reports.
pub struct CellOut {
    pub photo: PhotoId,
    pub section: state::Section,
    /// The square on screen.
    pub rect: Rect,
    /// The letterboxed image inside it.
    pub image: Rect,
    pub response: Response,
    pub drawn: Drawn,
}

/// The image's rect inside a square `cell`: letterboxed to its aspect (`width × height`; unknown
/// sizes fill the square).
pub fn letterbox(cell: Rect, width: u32, height: u32) -> Rect {
    let aspect = if width > 0 && height > 0 { width as f32 / height as f32 } else { 1.0 };
    let size = if aspect >= 1.0 { vec2(cell.width(), cell.width() / aspect) } else { vec2(cell.height() * aspect, cell.height()) };
    Rect::from_center_size(cell.center(), size)
}

/// The hidden-photo rule (`ui-layout.md` §5.3): a photo that is not hidden is drawn opaque; a
/// hidden one at [`HIDDEN_OPACITY`] when View ▸ Hidden Pictures is on, and not at all when it is
/// off (`None`).
pub fn opacity(hidden: bool, hidden_pictures: bool) -> Option<f32> {
    match (hidden, hidden_pictures) {
        (false, _) => Some(1.0),
        (true, true) => Some(HIDDEN_OPACITY),
        (true, false) => None,
    }
}

/// Whether `photo` is hidden. The photo model gains its hidden flag in M6 (`data-model.md` §3);
/// until then no photo is.
pub fn is_hidden(photo: &Photo) -> bool {
    let _ = photo;
    false
}

/// What `photo`'s badges show: what the model knows. Starred and held now; edited, geotagged and
/// videos once the model carries them.
pub fn badges_of(photo: &Photo, held: bool) -> Badges {
    Badges { starred: photo.star, held, ..Badges::default() }
}

/// The caption line's text for `mode` (`ui-layout.md` §5.3), before fitting: the file name; the
/// date taken (else the file's date); the pixel size; empty for Caption and Tags until the model
/// carries them (M6) and for a size or date that is not known. `None` with captions off.
pub fn caption_text(photo: &Photo, mode: Caption) -> Option<String> {
    Some(match mode {
        Caption::None => return None,
        Caption::Filename => photo.name.clone(),
        Caption::Date => photo.sort_date().map(|d| long_date(d.date())).unwrap_or_default(),
        Caption::Resolution => resolution(photo).unwrap_or_default(),
        Caption::Caption | Caption::Tags => String::new(),
    })
}

/// `4000 × 3000`, when the size is known.
fn resolution(photo: &Photo) -> Option<String> {
    (photo.width > 0 && photo.height > 0).then(|| format!("{} × {}", photo.width, photo.height))
}

/// The hover tooltip (`ui-layout.md` §5.5): the file name over the date taken (else the file's
/// date), the pixel size and the file size; for a photo that cannot be shown, why.
pub fn tooltip_text(photo: &Photo, failure: Option<&str>) -> Tip {
    let detail = match failure {
        Some(why) => format!("Cannot show this photo: {why}"),
        None => {
            let mut parts: Vec<String> = Vec::with_capacity(3);
            parts.extend(photo.sort_date().map(status::date_time));
            parts.extend(resolution(photo));
            parts.push(status::size(photo.size));
            parts.join(" · ")
        }
    };
    Tip { title: photo.name.clone(), detail }
}

/// `text` cut in the middle with "…" until `width` of it is at most `max` (the most characters
/// that fit); unchanged when it fits; empty when not even "…" fits.
pub fn fit_middle(text: &str, max: f32, width: impl Fn(&str) -> f32) -> String {
    if width(text) <= max {
        return text.to_owned();
    }
    let n = text.chars().count();
    if n == 0 || width("…") > max || !max.is_finite() {
        return String::new();
    }
    // The most characters kept that still fit, by bisection (width grows with what is kept).
    let (mut lo, mut hi) = (0usize, n - 1);
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if width(&status::elide_middle(text, mid)) <= max {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    status::elide_middle(text, lo)
}

/// How a cell looks, apart from its pixels and caption.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// The square on screen.
    pub cell: Rect,
    /// The letterboxed image in it.
    pub image: Rect,
    pub selected: bool,
    /// The selection's anchor (the focused photo).
    pub anchor: bool,
    pub hovered: bool,
    pub badges: Badges,
}

/// Paints one cell (see the module's boundary).
pub fn show(ui: &mut Ui, f: &Frame<'_>, cells: &mut Cells, thumbs: &mut Thumbnails, c: &CellIn<'_>) -> CellOut {
    let t = f.t;
    let id = c.photo.id;
    let pass = ui.ctx().cumulative_pass_nr();
    let held = &f.state.tray.held;
    let selection = &f.state.selection;
    cells.begin(pass, held, &selection.ids);
    let image = letterbox(c.rect, c.photo.width, c.photo.height);
    let Some(alpha) = opacity(is_hidden(c.photo), f.state.hidden_pictures) else {
        // Not shown: the parent leaves hidden photos out; this is the cell's own guard.
        let response = ui.interact(c.rect, Id::new(("thumb", id.get())), Sense::hover());
        return CellOut { photo: id, section: c.section, rect: c.rect, image, response, drawn: Drawn::Placeholder };
    };
    let response = ui.interact(c.rect, Id::new(("thumb", id.get())), Sense::click());
    let selected = cells.selected.contains(&selection.ids, id);
    let mut painter = ui.painter().clone();
    painter.multiply_opacity(alpha);
    let look = Look {
        cell: c.rect,
        image,
        selected,
        anchor: selection.anchor == Some(id),
        hovered: response.hovered(),
        badges: badges_of(c.photo, cells.held.contains(held, id)),
    };
    paint_under(&painter, t, &look);
    let device_px = image.width().max(image.height()) * f.pixels_per_point;
    let drawn = thumbs.paint(&painter, PhotoRef { session: f.session, photo: c.photo }, image, device_px);
    let marks = paint_over(&painter, t, &look, drawn == Drawn::Failed);
    let caption = match (c.caption, caption_text(c.photo, f.state.caption)) {
        (Some(line), Some(text)) => {
            let font = fonts::regular(t.caption);
            let fitted = cells.fit(&painter, id, &text, (line.width() - 4.0).max(0.0), &font);
            painter.text(line.center(), Align2::CENTER_CENTER, &fitted, font, t.text_dim);
            Some(fitted)
        }
        _ => None,
    };
    if let Some(star) = marks.get(Badge::Starred) {
        let target = Target::Painted { rect: star, clip: painter.clip_rect() };
        widgets::register(ui.ctx(), &format!("thumb:{}/star", id.get()), Role::Badge, Badge::Starred.label(), target, true);
    }
    widgets::register(ui.ctx(), &format!("thumb:{}", id.get()), Role::Thumbnail, &c.photo.name, Target::Widget(&response), selected);
    let tooltip = cells.rested(ui, id, &response, pass, f.now).then(|| {
        let tip = tooltip_text(c.photo, if drawn == Drawn::Failed { thumbs.failure(id) } else { None });
        widgets::tooltip_now(&response, t, &tip.title, Some(&tip.detail));
        tip
    });
    cells.shown.push(Shown { photo: id, caption, selected, opacity: alpha, badges: look.badges, drawn, tooltip });
    CellOut { photo: id, section: c.section, rect: c.rect, image, response, drawn }
}

/// What goes under the image: the selected or hovered surround, and the photo's drop shadow.
pub fn paint_under(painter: &Painter, t: &Tokens, look: &Look) {
    if look.selected {
        painter.rect_filled(look.cell, 0, t.cell_selected);
    } else if look.hovered {
        painter.rect_filled(look.cell, 0, t.hover.gamma_multiply(0.5));
    }
    widgets::photo_shadow(painter, look.image);
}

/// What goes over the image: the selection outline, the "!" of a photo that cannot be shown, the
/// badges and video marks. Returns where the marks went.
pub fn paint_over(painter: &Painter, t: &Tokens, look: &Look, failed: bool) -> widgets::BadgeRects {
    if look.selected {
        let color = if look.anchor { t.photo_focus } else { t.photo_selected };
        painter.rect_stroke(look.image, 0, Stroke::new(2.0, color), StrokeKind::Outside);
    }
    if failed {
        widgets::failed_mark(painter, t, look.image);
    }
    widgets::badges(painter, t, look.image, &look.badges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    #[test]
    fn photos_are_letterboxed_never_cropped() {
        let cell = Rect::from_min_size(pos2(0.0, 0.0), vec2(144.0, 144.0));
        assert_eq!(letterbox(cell, 480, 320).size(), vec2(144.0, 96.0));
        assert_eq!(letterbox(cell, 320, 480).size(), vec2(96.0, 144.0));
        assert_eq!(letterbox(cell, 0, 0).size(), vec2(144.0, 144.0));
    }

    #[test]
    fn middle_elision_keeps_the_most_that_fits() {
        // One point per character.
        let w = |s: &str| s.chars().count() as f32;
        assert_eq!(fit_middle("IMG_0361.jpg", 12.0, w), "IMG_0361.jpg");
        assert_eq!(fit_middle("IMG_0361.jpg", 8.0, w), "IMG_…jpg");
        assert_eq!(fit_middle("IMG_0361.jpg", 1.0, w), "…");
        assert_eq!(fit_middle("IMG_0361.jpg", 0.5, w), "");
        assert_eq!(fit_middle("", 0.0, w), "");
        assert_eq!(fit_middle("ñandú.jpg", 5.0, w), "ña…pg", "characters, never bytes");
        assert_eq!(fit_middle("abc", f32::NAN, w), "");
    }
}
