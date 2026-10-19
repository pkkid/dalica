//! The collage document (`creating.md` §3): pure data, everything a collage is made of. The
//! layouts ([`super::layout`]) turn it into placements; the recipe ([`super::recipe`]) stores it.

use std::collections::HashSet;

use dalica_core::PhotoId;
use serde::{Deserialize, Serialize};

use super::page::Page;
use super::seed::Rng;

/// The most pictures a collage's Clips hold.
pub const MAX_CLIPS: usize = 1000;
/// The widest grid spacing, as a fraction of the page's short side.
pub const MAX_SPACING: f64 = 0.05;
/// The default grid spacing.
pub const DEFAULT_SPACING: f64 = 0.01;
/// The smallest and largest scale the user may give a pile's picture.
pub const MIN_SCALE: f64 = 0.05;
pub const MAX_SCALE: f64 = 20.0;
/// A turned picture's angle lies in `-MAX_ANGLE ..= MAX_ANGLE` degrees.
pub const MAX_ANGLE: f64 = 180.0;
/// How far a picture may be raised or lowered (Bring to Top, Send to Bottom).
pub const MAX_LAYER: i64 = 1_000_000;
/// How far past the page's edges a moved picture's centre may go, as a fraction of the page.
pub const CENTRE_MARGIN: f64 = 0.5;

/// Why a document is refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DocError {
    #[error("{0}")]
    Range(String),
    #[error("more than {MAX_CLIPS} {0}")]
    TooMany(&'static str),
    #[error("picture {0} is in the clips twice")]
    Duplicate(PhotoId),
    #[error("{what} is picture {id}, which is not in the clips")]
    NotAClip { what: &'static str, id: PhotoId },
}

/// The collage's style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Style {
    #[default]
    Pile,
    Mosaic,
    FrameMosaic,
    Grid,
    ContactSheet,
    MultipleExposure,
}

impl Style {
    /// Every style, in the list's order.
    pub const ALL: [Style; 6] = [Style::Pile, Style::Mosaic, Style::FrameMosaic, Style::Grid, Style::ContactSheet, Style::MultipleExposure];

    /// The id commands and recipes use.
    pub fn id(self) -> &'static str {
        match self {
            Style::Pile => "pile",
            Style::Mosaic => "mosaic",
            Style::FrameMosaic => "frameMosaic",
            Style::Grid => "grid",
            Style::ContactSheet => "contactSheet",
            Style::MultipleExposure => "multipleExposure",
        }
    }

    pub fn from_id(id: &str) -> Option<Style> {
        Style::ALL.into_iter().find(|s| s.id() == id)
    }

    /// The name in the list.
    pub fn label(self) -> &'static str {
        match self {
            Style::Pile => "Picture Pile",
            Style::Mosaic => "Mosaic",
            Style::FrameMosaic => "Frame Mosaic",
            Style::Grid => "Grid",
            Style::ContactSheet => "Contact Sheet",
            Style::MultipleExposure => "Multiple Exposure",
        }
    }

    /// The line under the name.
    pub fn description(self) -> &'static str {
        match self {
            Style::Pile => "Looks like a pile of scattered pictures",
            Style::Mosaic => "Fits pictures into the page automatically",
            Style::FrameMosaic => "A mosaic around a large center picture",
            Style::Grid => "Pictures in regular rows and columns",
            Style::ContactSheet => "Small pictures with their names",
            Style::MultipleExposure => "Pictures blended on top of one another",
        }
    }

    /// Whether Picture borders applies.
    pub fn has_borders(self) -> bool {
        matches!(self, Style::Pile | Style::ContactSheet)
    }

    /// Whether Grid spacing applies.
    pub fn has_spacing(self) -> bool {
        matches!(self, Style::Mosaic | Style::FrameMosaic | Style::Grid)
    }

    /// Whether Show captions applies.
    pub fn has_captions(self) -> bool {
        self == Style::Pile
    }

    /// Whether the page has a background (Multiple Exposure covers it with the pictures).
    pub fn has_background(self) -> bool {
        self != Style::MultipleExposure
    }

    /// Whether the user moves, turns and scales pictures (the others swap them).
    pub fn is_free(self) -> bool {
        self == Style::Pile
    }
}

/// The border drawn round each picture (Pile, Contact Sheet).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Border {
    #[default]
    None,
    White,
    Instant,
}

impl Border {
    /// The border's width on the top, left and right, as a fraction of the picture's short side.
    pub fn sides(self) -> f64 {
        match self {
            Border::None => 0.0,
            Border::White => 0.03,
            Border::Instant => 0.05,
        }
    }

    /// The border's width at the bottom, as a fraction of the picture's short side.
    pub fn bottom(self) -> f64 {
        match self {
            Border::None => 0.0,
            Border::White => 0.03,
            Border::Instant => 0.20,
        }
    }
}

/// The style's options.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub border: Border,
    /// Grid spacing as a fraction of the page's short side, 0 ..= [`MAX_SPACING`].
    pub spacing: f64,
    pub shadows: bool,
    pub captions: bool,
}

impl Default for Settings {
    /// No border, a little spacing, shadows on, captions off.
    fn default() -> Settings {
        Settings { border: Border::None, spacing: DEFAULT_SPACING, shadows: true, captions: false }
    }
}

/// The page's background: a solid colour, or one of the clips covering the page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Background {
    /// The solid colour (`#rrggbb` in a recipe), kept while a picture is used.
    #[serde(with = "hex_colour")]
    pub colour: [u8; 3],
    /// Use picture: the clip that covers the page.
    pub picture: Option<PhotoId>,
}

/// One picture of the Clips.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub id: PhotoId,
    /// Whether the picture is on the page (Remove takes it off; it stays in the Clips).
    pub on_page: bool,
}

/// Where the user put one picture of a pile, over what the layout gave it. `None` keeps the
/// layout's own.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Move {
    pub id: PhotoId,
    /// The picture's centre as a fraction of the page's width and height.
    pub centre: Option<[f64; 2]>,
    /// The picture's size over the layout's size for it.
    pub scale: Option<f64>,
    /// The picture's angle in degrees, clockwise.
    pub angle: Option<f64>,
    /// Raised (above 0, Bring to Top) or lowered (below 0, Send to Bottom) in the stack.
    pub layer: i64,
}

/// A collage.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collage {
    pub style: Style,
    pub settings: Settings,
    pub page: Page,
    /// The arrangement's seed (Scramble Collage draws another).
    #[serde(with = "hex_u64")]
    pub seed: u64,
    /// The assignment of pictures to the arrangement's slots: 0 in clip order, anything else a
    /// seeded order (Shuffle Pictures draws another).
    #[serde(with = "hex_u64")]
    pub shuffle: u64,
    pub clips: Vec<Clip>,
    pub background: Background,
    /// Frame Mosaic's centre picture (the first picture on the page when `None`).
    pub frame_centre: Option<PhotoId>,
    /// The user's placements of a pile's pictures, at most one per picture.
    pub moves: Vec<Move>,
    /// Two pictures' slots exchanged by a drag (mosaics, grid), applied in order.
    pub swaps: Vec<[PhotoId; 2]>,
}

impl Collage {
    /// A collage of `ids` (in order, all on the page) with the defaults and `seed`. Ids after the
    /// first [`MAX_CLIPS`] and repeated ids are left out.
    pub fn new(ids: impl IntoIterator<Item = PhotoId>, seed: u64) -> Collage {
        let mut c = Collage { seed, ..Collage::default() };
        c.add_clips(ids);
        c
    }

    /// Adds `ids` to the Clips and the page, leaving out those already there and any past
    /// [`MAX_CLIPS`]; answers how many were added.
    pub fn add_clips(&mut self, ids: impl IntoIterator<Item = PhotoId>) -> usize {
        let mut seen: HashSet<PhotoId> = self.clips.iter().map(|c| c.id).collect();
        let before = self.clips.len();
        for id in ids {
            if self.clips.len() >= MAX_CLIPS {
                break;
            }
            if seen.insert(id) {
                self.clips.push(Clip { id, on_page: true });
            }
        }
        self.clips.len() - before
    }

    /// The position of `id` in the Clips.
    pub fn clip_index(&self, id: PhotoId) -> Option<usize> {
        self.clips.iter().position(|c| c.id == id)
    }

    /// The clips on the page, by their position in the Clips.
    pub fn on_page(&self) -> Vec<usize> {
        self.clips.iter().enumerate().filter(|(_, c)| c.on_page).map(|(i, _)| i).collect()
    }

    /// The pictures on the page in slot order (positions in the Clips): the clip order, put in
    /// the shuffle's order, then the swaps applied.
    pub fn assignment(&self) -> Vec<usize> {
        let mut order = self.shuffled(self.shuffle);
        for [a, b] in &self.swaps {
            let pos = |id: PhotoId| order.iter().position(|&i| self.clips.get(i).is_some_and(|c| c.id == id));
            if let (Some(x), Some(y)) = (pos(*a), pos(*b)) {
                order.swap(x, y);
            }
        }
        order
    }

    /// The pictures on the page in the order `shuffle` puts them, before the swaps.
    fn shuffled(&self, shuffle: u64) -> Vec<usize> {
        let mut order = self.on_page();
        if shuffle != 0 {
            Rng::new(shuffle).shuffle(&mut order);
        }
        order
    }

    /// Scramble Collage: a new arrangement. Draws another seed from this one and drops the user's
    /// placements (moves and swaps); the assignment of pictures stays.
    pub fn scramble(&mut self) {
        let mut rng = Rng::stream(self.seed, 0x5c4a);
        let mut seed = rng.next_u64();
        while seed == self.seed {
            seed = rng.next_u64();
        }
        self.seed = seed;
        self.moves.clear();
        self.swaps.clear();
    }

    /// Shuffle Pictures: the same slots, another assignment of pictures to them. Draws another
    /// shuffle that changes the order whenever two or more pictures are on the page, and drops the
    /// swaps (they named slots of the old assignment); a pile's moves stay with their pictures.
    pub fn shuffle_pictures(&mut self) {
        let before = self.assignment();
        let mut rng = Rng::stream(self.shuffle ^ self.seed, 0x5f1e);
        let mut candidate = self.shuffle;
        // Two pictures: each draw changes the order with a chance of one half; 64 draws all
        // failing never happens in practice, and even then the shuffle still moves on.
        for _ in 0..64 {
            candidate = rng.next_u64().max(1);
            if self.shuffled(candidate) != before || before.len() < 2 {
                break;
            }
        }
        self.shuffle = candidate;
        self.swaps.clear();
    }

    /// The user's move of `id`, if any.
    pub fn move_of(&self, id: PhotoId) -> Option<&Move> {
        self.moves.iter().find(|m| m.id == id)
    }

    /// Every number finite and in range, at most [`MAX_CLIPS`] clips, moves and swaps, no clip
    /// twice, the background picture, the frame centre and every moved or swapped picture among
    /// the clips.
    pub fn check(&self) -> Result<(), DocError> {
        let s = &self.settings;
        if !(s.spacing.is_finite() && (0.0..=MAX_SPACING).contains(&s.spacing)) {
            return Err(DocError::Range(format!("the grid spacing is 0 to {MAX_SPACING}")));
        }
        self.page.format.check()?;
        if self.clips.len() > MAX_CLIPS {
            return Err(DocError::TooMany("clips"));
        }
        if self.moves.len() > MAX_CLIPS {
            return Err(DocError::TooMany("moves"));
        }
        if self.swaps.len() > MAX_CLIPS {
            return Err(DocError::TooMany("swaps"));
        }
        let mut ids = HashSet::with_capacity(self.clips.len());
        for c in &self.clips {
            if !ids.insert(c.id) {
                return Err(DocError::Duplicate(c.id));
            }
        }
        let known = |what: &'static str, id: PhotoId| if ids.contains(&id) { Ok(()) } else { Err(DocError::NotAClip { what, id }) };
        if let Some(id) = self.background.picture {
            known("the background", id)?;
        }
        if let Some(id) = self.frame_centre {
            known("the frame centre", id)?;
        }
        let mut moved = HashSet::with_capacity(self.moves.len());
        for m in &self.moves {
            known("a moved picture", m.id)?;
            if !moved.insert(m.id) {
                return Err(DocError::Range(format!("picture {} is moved twice", m.id)));
            }
            check_move(m)?;
        }
        for [a, b] in &self.swaps {
            known("a swapped picture", *a)?;
            known("a swapped picture", *b)?;
        }
        Ok(())
    }
}

fn check_move(m: &Move) -> Result<(), DocError> {
    let centre = (-CENTRE_MARGIN)..=(1.0 + CENTRE_MARGIN);
    if let Some([x, y]) = m.centre
        && !(x.is_finite() && y.is_finite() && centre.contains(&x) && centre.contains(&y))
    {
        return Err(DocError::Range(format!("a moved picture's centre is {} to {}", -CENTRE_MARGIN, 1.0 + CENTRE_MARGIN)));
    }
    if let Some(scale) = m.scale
        && !(scale.is_finite() && (MIN_SCALE..=MAX_SCALE).contains(&scale))
    {
        return Err(DocError::Range(format!("a picture's scale is {MIN_SCALE} to {MAX_SCALE}")));
    }
    if let Some(angle) = m.angle
        && !(angle.is_finite() && (-MAX_ANGLE..=MAX_ANGLE).contains(&angle))
    {
        return Err(DocError::Range(format!("a picture's angle is -{MAX_ANGLE} to {MAX_ANGLE} degrees")));
    }
    if !(-MAX_LAYER..=MAX_LAYER).contains(&m.layer) {
        return Err(DocError::Range(format!("a picture's layer is -{MAX_LAYER} to {MAX_LAYER}")));
    }
    Ok(())
}

/// A colour as `#rrggbb`.
mod hex_colour {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(c: &[u8; 3], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&dalica_core::color::hex_u8(*c))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 3], D::Error> {
        let text = String::deserialize(d)?;
        dalica_core::color::parse_hex_u8(&text).ok_or_else(|| D::Error::custom("a colour is #rrggbb"))
    }
}

/// A `u64` as 16 hex digits, so readers whose numbers are doubles keep every bit.
pub(super) mod hex_u64 {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("{v:016x}"))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        let text = String::deserialize(d)?;
        if text.is_empty() || text.len() > 16 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(D::Error::custom("a seed is 1 to 16 hex digits"));
        }
        u64::from_str_radix(&text, 16).map_err(D::Error::custom)
    }
}
