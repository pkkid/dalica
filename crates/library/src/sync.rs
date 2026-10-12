//! Which way each owned metadata field goes between a photo file and Dalica (`xmp-interop.md` §5):
//! a pure function of the file's value, Dalica's value (the folder sidecar's, as the model holds
//! it) and the field's last-synced hash ([`Synced`]).
//!
//! - **First scan** (the photo has no entry in its folder sidecar): every value the file has is
//!   imported.
//! - **The file and Dalica agree:** nothing to do.
//! - **Only Dalica changed** since the last sync (the file still hashes as last synced), or there is
//!   no last-synced value (after an index rebuild): Dalica's value wins and is written.
//! - **Only the file changed** (Dalica's value still hashes as last synced): another application
//!   edited it; the file's value is imported and nothing is written back.
//! - **Both changed:** Dalica's value wins and is written, and the conflict is reported
//!   ("Changed in another application; Dalica's value kept").
//!
//! A date is never removed from a file: with no date in Dalica, a write becomes [`Decision::Keep`].
//! Whether a [`Decision::Write`] is made is the caller's: only for a field an op changed, and only
//! when the field's write option is on (rule 4).
//!
//! The hashes are FNV-1a (64 bit) over a canonical form of each value; they are stored in the index
//! ([`Synced`]), so the form never changes (the tests pin it). Tags hash as a set (their order in a
//! file's `rdf:Bag` means nothing), a place to the micro-degree, a date as its ISO text.

use dalica_core::DateTime;

use crate::Synced;

/// An owned metadata field (`xmp-interop.md` §2), as [`Synced`] keeps one hash of each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Field {
    Caption,
    Tags,
    /// Dalica's star against the file's rating: a rating of 5 is a star.
    Rating,
    Place,
    Date,
}

impl Field {
    pub const ALL: [Field; 5] = [Field::Caption, Field::Tags, Field::Rating, Field::Place, Field::Date];

    /// The field's name in messages.
    pub fn name(self) -> &'static str {
        match self {
            Field::Caption => "caption",
            Field::Tags => "tags",
            Field::Rating => "star",
            Field::Place => "place",
            Field::Date => "date taken",
        }
    }
}

/// The owned values of one photo, on one side (the file's, or Dalica's).
///
/// The caller passes them as the library stores them (`clean_caption`, `clean_tags`): the file's
/// values cleaned the same way, so equal text compares equal. Dalica's date is the photo's
/// effective date (the user's, else the file's as last scanned).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Values {
    /// Empty for none.
    pub caption: String,
    pub tags: Vec<String>,
    /// A star; on the file's side, a rating of 5.
    pub star: bool,
    /// Latitude and longitude in degrees.
    pub place: Option<(f64, f64)>,
    pub date: Option<DateTime>,
}

impl Values {
    /// The hash of `field`'s value.
    pub fn hash(&self, field: Field) -> u64 {
        let mut h = Fnv::new(field);
        match field {
            Field::Caption => h.text(&self.caption),
            Field::Tags => {
                let mut tags: Vec<&str> = self.tags.iter().map(String::as_str).collect();
                tags.sort_unstable();
                tags.dedup();
                h.number(u64::try_from(tags.len()).unwrap_or(u64::MAX));
                for t in tags {
                    h.text(t);
                }
            }
            Field::Rating => h.bytes(&[u8::from(self.star)]),
            Field::Place => match self.place {
                None => h.bytes(&[0]),
                Some((lat, lon)) => {
                    h.bytes(&[1]);
                    h.number(micro(lat));
                    h.number(micro(lon));
                }
            },
            Field::Date => match self.date {
                None => h.bytes(&[0]),
                Some(d) => {
                    h.bytes(&[1]);
                    h.text(&d.to_string());
                }
            },
        }
        h.finish()
    }

    /// The hashes of every field: what [`Synced`] becomes once these values are read from (or
    /// written to) the file.
    pub fn hashes(&self) -> Synced {
        let mut s = Synced::default();
        for f in Field::ALL {
            s.set(f, Some(self.hash(f)));
        }
        s
    }
}

/// A coordinate in whole micro-degrees, as the bits of an `i64` (finite values only; anything
/// else hashes as 0).
fn micro(degrees: f64) -> u64 {
    let m = (degrees * 1e6).round();
    let m = if m.is_finite() && m.abs() <= 1e12 { m as i64 } else { 0 };
    u64::from_le_bytes(m.to_le_bytes())
}

impl Synced {
    /// The last-synced hash of `field`, if it was ever synced.
    pub fn get(&self, field: Field) -> Option<u64> {
        match field {
            Field::Caption => self.caption,
            Field::Tags => self.tags,
            Field::Rating => self.rating,
            Field::Place => self.place,
            Field::Date => self.date,
        }
    }

    /// Sets (or with `None` forgets) the last-synced hash of `field`.
    pub fn set(&mut self, field: Field, hash: Option<u64>) {
        let slot = match field {
            Field::Caption => &mut self.caption,
            Field::Tags => &mut self.tags,
            Field::Rating => &mut self.rating,
            Field::Place => &mut self.place,
            Field::Date => &mut self.date,
        };
        *slot = hash;
    }
}

/// What to do with one field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Nothing: the file and Dalica agree, or there is nothing Dalica could write (a date it does
    /// not have).
    Keep,
    /// Dalica's value goes into the file (it changed, or there is no last-synced value).
    Write,
    /// The file's value comes into Dalica (another application changed it; Dalica's did not).
    Import,
    /// Both changed since the last sync: Dalica's value is kept (and written), the conflict is
    /// reported.
    Conflict,
}

/// The decision for one field from its three hashes: the file's value, Dalica's value and the
/// last-synced one (`None` after an index rebuild or before any sync). `first_scan`: the photo has
/// no entry in its folder sidecar, so the file's value is imported.
pub fn decide(file: u64, ours: u64, synced: Option<u64>, first_scan: bool) -> Decision {
    if file == ours {
        Decision::Keep
    } else if first_scan {
        Decision::Import
    } else {
        match synced {
            None => Decision::Write,
            Some(s) if s == file => Decision::Write,
            Some(s) if s == ours => Decision::Import,
            Some(_) => Decision::Conflict,
        }
    }
}

/// The decision for `field` between the file's values and Dalica's (`ours`).
pub fn decide_field(field: Field, file: &Values, ours: &Values, synced: Option<&Synced>, first_scan: bool) -> Decision {
    let d = decide(file.hash(field), ours.hash(field), synced.and_then(|s| s.get(field)), first_scan);
    match d {
        // A date is never removed from a file.
        Decision::Write if field == Field::Date && ours.date.is_none() => Decision::Keep,
        d => d,
    }
}

/// The decision for every field, in [`Field::ALL`] order.
pub fn reconcile(file: &Values, ours: &Values, synced: Option<&Synced>, first_scan: bool) -> [(Field, Decision); 5] {
    Field::ALL.map(|f| (f, decide_field(f, file, ours, synced, first_scan)))
}

/// FNV-1a, 64 bit, with the field's own start so equal bytes of two fields hash apart.
struct Fnv(u64);

impl Fnv {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new(field: Field) -> Fnv {
        let mut h = Fnv(Fnv::OFFSET);
        h.bytes(&[field as u8]);
        h
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(Fnv::PRIME);
        }
    }

    fn number(&mut self, n: u64) {
        self.bytes(&n.to_le_bytes());
    }

    /// Length first, so `["ab", "c"]` and `["a", "bc"]` differ.
    fn text(&mut self, s: &str) {
        self.number(u64::try_from(s.len()).unwrap_or(u64::MAX));
        self.bytes(s.as_bytes());
    }

    fn finish(&self) -> u64 {
        self.0
    }
}
