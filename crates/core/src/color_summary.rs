//! The search colours and the colour classifier behind `color:` search (`organizing.md` §8.1).
//!
//! A photo's [`ColourSummary`] is the set of [`SearchColour`]s that cover enough of it, its main
//! colour first. [`classify`] makes one from a small grid of sRGB cells (the decoded colour
//! sketch every photo already has), so a summary costs no file read and follows edits.
//!
//! How a grid is read, every threshold a constant below:
//!
//! 1. Each cell goes through [`crate::color::rgb_to_hsv`]. A cell whose saturation is under
//!    [`MIN_SATURATION`] or whose value is under [`MIN_VALUE`] has no colour (grey, white, black,
//!    a faint tint, deep shadow).
//! 2. Every other cell adds its saturation to the weight of the colour its hue falls in
//!    ([`hue_colour`]; the boundaries are [`HUE_BOUNDS`]: pink counts as red, brown as orange,
//!    cyan as blue, magenta as purple).
//! 3. When the coloured weight is under [`BW_SHARE`] of the cell count, the picture is black and
//!    white: the summary is `bw` alone (a speck of colour in a grey picture never makes it red).
//! 4. Otherwise a colour joins when its weight reaches [`JOIN_SHARE`] of the coloured weight; the
//!    largest is the main colour (a tie goes to the first in [`SearchColour::ALL`]).
//!
//! An empty grid, or one of more than [`MAX_CELLS`] cells, answers the empty summary. The answer
//! depends only on the cells, in order, so the same grid gives the same summary every time.

use crate::color::{rgb_from_u8, rgb_to_hsv};

/// A colour `color:` search and Tools ▸ Experimental ▸ Search for ▸ name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SearchColour {
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Purple,
    /// Black and white: a picture with almost no colour.
    Bw,
}

impl SearchColour {
    /// The seven, in the menu's order.
    pub const ALL: [SearchColour; 7] = [Self::Red, Self::Orange, Self::Yellow, Self::Green, Self::Blue, Self::Purple, Self::Bw];

    /// The six that are hues (every one but `bw`), in the menu's order.
    pub const HUES: [SearchColour; 6] = [Self::Red, Self::Orange, Self::Yellow, Self::Green, Self::Blue, Self::Purple];

    /// The word `color:<word>` and `search.color {color}` use: `red` … `purple`, `bw`.
    pub fn word(self) -> &'static str {
        match self {
            Self::Red => "red",
            Self::Orange => "orange",
            Self::Yellow => "yellow",
            Self::Green => "green",
            Self::Blue => "blue",
            Self::Purple => "purple",
            Self::Bw => "bw",
        }
    }

    /// The menu's name: `Red` … `Purple`, `Black and White`.
    pub fn label(self) -> &'static str {
        match self {
            Self::Red => "Red",
            Self::Orange => "Orange",
            Self::Yellow => "Yellow",
            Self::Green => "Green",
            Self::Blue => "Blue",
            Self::Purple => "Purple",
            Self::Bw => "Black and White",
        }
    }

    /// The colour `word` names, in any case (`GREEN`, `Bw`); anything else is `None`.
    pub fn from_word(word: &str) -> Option<SearchColour> {
        Self::ALL.into_iter().find(|c| c.word().eq_ignore_ascii_case(word))
    }

    /// The place of `self` in [`Self::ALL`].
    fn index(self) -> usize {
        self as usize
    }

    /// The place of a hue in [`Self::HUES`]; `None` for `bw`.
    fn hue_index(self) -> Option<usize> {
        if self == Self::Bw { None } else { Some(self.index()) }
    }
}

/// The hue boundaries, in degrees: a hue `h` belongs to the colour of the last bound at or below
/// it. Red wraps round 0 (it starts at 320 and runs past 360 to 15), so pink is red; brown (a dark
/// orange, hue about 20 … 40) is orange; cyan is blue; magenta (300) is purple.
pub const HUE_BOUNDS: [(f32, SearchColour); 7] = [
    (0.0, SearchColour::Red),
    (15.0, SearchColour::Orange),
    (45.0, SearchColour::Yellow),
    (70.0, SearchColour::Green),
    (165.0, SearchColour::Blue),
    (255.0, SearchColour::Purple),
    (320.0, SearchColour::Red),
];

/// A cell less saturated than this has no colour (greys, whites, faint tints).
pub const MIN_SATURATION: f32 = 0.2;

/// A cell darker than this (HSV value) has no colour: in deep shadow a hue is noise.
pub const MIN_VALUE: f32 = 0.2;

/// A picture whose coloured weight (the sum of its coloured cells' saturations) is under this share
/// of its cell count is black and white.
pub const BW_SHARE: f32 = 0.04;

/// A colour joins the summary when its weight reaches this share of the coloured weight.
pub const JOIN_SHARE: f32 = 0.15;

/// The most cells [`classify`] reads: a 64 × 64 grid, the largest a sketch decodes to.
pub const MAX_CELLS: usize = 64 * 64;

/// The colour a hue (degrees; any value, it wraps; not finite reads as 0) falls in ([`HUE_BOUNDS`]).
/// Never `bw`.
pub fn hue_colour(hue: f32) -> SearchColour {
    let h = if hue.is_finite() { hue.rem_euclid(360.0) } else { 0.0 };
    let mut colour = SearchColour::Red;
    for (bound, c) in HUE_BOUNDS {
        if h >= bound {
            colour = c;
        }
    }
    colour
}

/// The colours that cover a photo: a set of [`SearchColour`]s with its main colour, or empty (no
/// summary made yet, or nothing to make it from). `bw` never shares a summary with a hue.
///
/// Its byte form ([`Self::to_byte`]), bits least significant first: bits 0..3 the main colour
/// (0 none, 1 … 6 red … purple, 7 `bw`); bits 3..8 the other hues of the summary, one bit each for
/// the five hues that are not the main one, in [`SearchColour::HUES`] order. The empty summary is
/// 0; with no main colour or with `bw`, bits 3..8 are 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ColourSummary {
    main: Option<SearchColour>,
    /// Every colour of the summary, the main one included: bit i is `SearchColour::ALL[i]`.
    set: u8,
}

/// Bits 0..3 of the byte form.
const MAIN_BITS: u8 = 0b111;
/// The main-colour code of `bw` in the byte form.
const BW_CODE: u8 = 7;

impl ColourSummary {
    /// The empty summary.
    pub const EMPTY: ColourSummary = ColourSummary { main: None, set: 0 };

    /// The summary whose main colour is `main`, with `others` too (`main` among them is ignored).
    /// `None` when `bw` would share the summary with a hue.
    pub fn new(main: SearchColour, others: &[SearchColour]) -> Option<ColourSummary> {
        let mut set = 1u8 << main.index();
        for &c in others {
            set |= 1 << c.index();
        }
        let bw = 1u8 << SearchColour::Bw.index();
        if set & bw != 0 && set != bw {
            return None;
        }
        Some(ColourSummary { main: Some(main), set })
    }

    /// Whether the summary holds no colour.
    pub fn is_empty(self) -> bool {
        self.main.is_none()
    }

    /// The main colour: the one covering most of the picture, or `bw`. `None` when empty.
    pub fn main(self) -> Option<SearchColour> {
        self.main
    }

    /// Whether `colour` is in the summary (as the main colour or another).
    pub fn contains(self, colour: SearchColour) -> bool {
        self.set & (1 << colour.index()) != 0
    }

    /// The colours of the summary: the main one first, then the others in [`SearchColour::ALL`] order.
    pub fn colours(self) -> impl Iterator<Item = SearchColour> {
        let main = self.main;
        main.into_iter().chain(SearchColour::ALL.into_iter().filter(move |&c| Some(c) != main && self.contains(c)))
    }

    /// The byte form (see the type's docs).
    pub fn to_byte(self) -> u8 {
        let Some(main) = self.main else { return 0 };
        let Some(m) = main.hue_index() else { return BW_CODE };
        let mut byte = (m as u8) + 1;
        let others = SearchColour::HUES.iter().filter(|c| c.hue_index() != Some(m));
        for (k, &c) in others.enumerate() {
            if self.contains(c) {
                byte |= 1 << (3 + k);
            }
        }
        byte
    }

    /// The summary `byte` holds; `None` for a byte with a bit this form does not know (other hues
    /// set beside no main colour or beside `bw`).
    pub fn from_byte(byte: u8) -> Option<ColourSummary> {
        let code = byte & MAIN_BITS;
        let rest = byte >> 3;
        match code {
            0 if rest == 0 => Some(Self::EMPTY),
            BW_CODE if rest == 0 => ColourSummary::new(SearchColour::Bw, &[]),
            0 | BW_CODE => None,
            _ => {
                let m = usize::from(code - 1);
                let main = *SearchColour::HUES.get(m)?;
                let others: Vec<SearchColour> = SearchColour::HUES
                    .iter()
                    .filter(|c| c.hue_index() != Some(m))
                    .enumerate()
                    .filter(|(k, _)| rest & (1 << k) != 0)
                    .map(|(_, &c)| c)
                    .collect();
                ColourSummary::new(main, &others)
            }
        }
    }
}

/// The summary of a grid of opaque sRGB cells, in any order and of any shape up to [`MAX_CELLS`]
/// (the module docs say how). Empty or oversized: the empty summary.
pub fn classify(cells: &[[u8; 3]]) -> ColourSummary {
    if cells.is_empty() || cells.len() > MAX_CELLS {
        return ColourSummary::EMPTY;
    }
    let mut weights = [0f32; 6];
    for &cell in cells {
        let [h, s, v] = rgb_to_hsv(rgb_from_u8(cell));
        if s < MIN_SATURATION || v < MIN_VALUE {
            continue;
        }
        if let Some(w) = hue_colour(h).hue_index().and_then(|i| weights.get_mut(i)) {
            *w += s;
        }
    }
    let coloured: f32 = weights.iter().sum();
    // At most 4096 cells, so the count is exact as an f32.
    let picture = cells.len() as f32;
    if coloured < BW_SHARE * picture || coloured <= 0.0 {
        return ColourSummary::new(SearchColour::Bw, &[]).unwrap_or_default();
    }
    let mut main: Option<(SearchColour, f32)> = None;
    let mut others = Vec::new();
    for (c, &w) in SearchColour::HUES.iter().zip(&weights) {
        if w < JOIN_SHARE * coloured {
            continue;
        }
        others.push(*c);
        if main.is_none_or(|(_, best)| w > best) {
            main = Some((*c, w));
        }
    }
    match main {
        Some((m, _)) => ColourSummary::new(m, &others).unwrap_or_default(),
        // Unreachable in practice: the largest weight is at least a sixth of the total, over
        // JOIN_SHARE. Answer empty rather than guess.
        None => ColourSummary::EMPTY,
    }
}
