//! `trim(start,end)`: a video's trim (`data-model.md` §3.1, `editing.md` §11; reserved decision 2,
//! `roadmap.md` M15.25): the part of the clip that plays, Export Clip writes and a slideshow shows,
//! as two whole numbers of milliseconds from the clip's start, the start before the end. A
//! video-only operation: it renders nothing (a photo never holds one; the engine refuses it on a
//! still), so its module stays a stub ([`Operation::VERSION`] 0) and [`crate::Op::typed`] leaves it
//! untyped, as the catalogue says (`catalog.rs`, `renders: false`). A recipe holds at most one that
//! counts: the last readable one ([`of`]); the engine's Set Start and Set End replace it in place.
//!
//! Only the text Dalica prints is read: `trim(500,1500)`, never `trim(0.5,1.5)`, `trim(-1,5)`,
//! `trim(+5,9)` or `trim( 5,9)`; a start at or after the end is refused, and so is a time past
//! [`MAX_MS`]. An end past the clip's length plays to its end.

use std::fmt;

use super::{Operation, ParamError};

/// The most milliseconds a trim names (a little over 11 days: more than any container lasts).
pub const MAX_MS: u64 = 1_000_000_000;

/// The longest time written, in digits ([`MAX_MS`] has ten).
const MAX_DIGITS: usize = 10;

/// `trim(start,end)`: the part of a video between `start_ms` (included) and `end_ms` (excluded).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Trim {
    pub start_ms: u64,
    pub end_ms: u64,
}

impl Trim {
    /// The trim from `start_ms` to `end_ms`, or why not (`start_ms` at or after `end_ms`, or
    /// either past [`MAX_MS`]).
    pub fn new(start_ms: u64, end_ms: u64) -> Result<Trim, ParamError> {
        let text = format!("{start_ms},{end_ms}");
        if start_ms > MAX_MS || end_ms > MAX_MS {
            return Err(ParamError::new(Self::NAME, &text, format!("times are at most {MAX_MS} ms")));
        }
        if start_ms >= end_ms {
            return Err(ParamError::new(Self::NAME, &text, "the start must come before the end"));
        }
        Ok(Trim { start_ms, end_ms })
    }

    /// How long the trimmed part lasts, in milliseconds (before a clip shorter than its end cuts it).
    pub fn length_ms(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }

    /// The trim as a recipe entry: `trim(500,1500)`.
    pub fn entry(&self) -> String {
        format!("{}({self})", Self::NAME)
    }
}

/// The whole clip: from its start to [`MAX_MS`] (Set Start or Set End on a clip without a trim
/// starts from it).
impl Default for Trim {
    fn default() -> Trim {
        Trim { start_ms: 0, end_ms: MAX_MS }
    }
}

impl fmt::Display for Trim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{},{}", self.start_ms, self.end_ms)
    }
}

/// One time as Dalica prints it: ASCII digits, no sign, no leading zero (but `0` itself).
fn millis(text: &str) -> Option<u64> {
    let plain = !text.is_empty() && text.len() <= MAX_DIGITS && text.bytes().all(|b| b.is_ascii_digit());
    if !plain || (text.len() > 1 && text.starts_with('0')) {
        return None;
    }
    text.parse().ok()
}

impl Operation for Trim {
    const NAME: &'static str = "trim";
    /// Renders nothing: a trim is a video's, and only the player and Export Clip read it.
    const VERSION: u32 = 0;
    const LABEL: &'static str = "Trim";

    fn parse(params: &str) -> Result<Trim, ParamError> {
        let Some((start, end)) = params.split_once(',') else {
            return Err(ParamError::new(Self::NAME, params, "takes a start and an end in milliseconds: trim(500,1500)"));
        };
        match (millis(start), millis(end)) {
            (Some(start), Some(end)) => Trim::new(start, end).map_err(|e| ParamError::new(Self::NAME, params, e.reason)),
            _ => Err(ParamError::new(Self::NAME, params, "times are whole milliseconds from the clip's start, 0 or more")),
        }
    }
}

/// Where the trim that counts is in a recipe's entries: the index of the last `trim(…)` entry and
/// what it reads as (`None` when it cannot be read: the clip then plays whole).
pub fn find(edits: &[String]) -> Option<(usize, Option<Trim>)> {
    let at = edits.iter().rposition(|e| e.starts_with("trim(") && e.ends_with(')'))?;
    let entry = edits.get(at)?;
    let params = entry.get(Trim::NAME.len() + 1..entry.len().saturating_sub(1))?;
    Some((at, Trim::parse(params).ok()))
}

/// The trim of a recipe: its last `trim(…)` entry, when that reads; `None` otherwise.
pub fn of(edits: &[String]) -> Option<Trim> {
    find(edits).and_then(|(_, t)| t)
}
