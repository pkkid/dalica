//! What a search asks for ([`Query`]: the text, the filter buttons and the age slider), what it
//! shows ([`Scope`]: hidden and small pictures, the light table's sections), and the matcher: the
//! rule for one photo, which the index answers for all photos at once. [`Extract::matching`] runs
//! the rule photo by photo, as the reference the index is tested against.

use std::collections::{BTreeSet, HashMap};

use dalica_core::{DateTime, FolderId, PhotoId, Timestamp};
use serde::{Deserialize, Serialize};

use super::extract::{AlbumDoc, Extract, FolderDoc, PersonDoc, PhotoDoc};
use super::grammar::{Field, Parsed};
use crate::{RECENT_DAYS, SMALL_PICTURE_PX, Section, TreeOptions};

/// The filter buttons (`organizing.md` §8.2), combinable with each other and with text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Filters {
    pub starred: bool,
    /// Photos with a named or unnamed face (ignored faces do not count).
    pub faces: bool,
    pub videos: bool,
    pub geotagged: bool,
}

impl Filters {
    /// Whether any button is down.
    pub fn any(&self) -> bool {
        self.starred || self.faces || self.videos || self.geotagged
    }
}

/// The age slider: only photos whose date ([`PhotoDoc::sort_date`]) is at most `days` days before
/// `now` (or after it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Age {
    pub days: u32,
    pub now: Timestamp,
}

impl Age {
    /// The oldest date that passes; `None` when every date does (a span reaching before year 1).
    pub(crate) fn cutoff(&self) -> Option<DateTime> {
        let span = i64::from(self.days).saturating_mul(86_400);
        DateTime::from_unix_secs(self.now.as_secs().checked_sub(span)?)
    }
}

/// One search: the text of the search box, the filter buttons and the age slider.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Query {
    pub text: String,
    pub filters: Filters,
    pub age: Option<Age>,
}

impl Query {
    /// A query of `text` alone.
    pub fn text(text: impl Into<String>) -> Query {
        Query { text: text.into(), ..Query::default() }
    }

    /// Whether the query asks for anything: text that is not blank, a filter button, the age.
    /// Search mode (and its banner) is on while it does.
    pub fn is_active(&self) -> bool {
        !self.text.trim().is_empty() || self.filters.any() || self.age.is_some()
    }
}

/// What the light table shows around a search: hidden and small pictures (as View ▸ Hidden
/// Pictures and Small Pictures say), its sections in order, and the clock for Recently Updated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    /// Hidden photos, and photos of hidden folders, only when this is on.
    pub hidden_pictures: bool,
    /// Pictures smaller than `small_threshold` on both sides only when this is on.
    pub small_pictures: bool,
    pub small_threshold: u32,
    /// The light table's sections in order (the tree's): the result follows their folders, and
    /// lists those of them with matches. Folders not among them follow, by path.
    pub sections: Vec<Section>,
    /// Now, for Recently Updated (that section has no matches without it).
    pub now: Option<Timestamp>,
    pub recent_days: u32,
}

impl Default for Scope {
    fn default() -> Scope {
        Scope {
            hidden_pictures: false,
            small_pictures: false,
            small_threshold: SMALL_PICTURE_PX,
            sections: Vec::new(),
            now: None,
            recent_days: RECENT_DAYS,
        }
    }
}

impl Scope {
    /// The scope of the tree built with `options`, whose sections are `sections` (the tree's
    /// sections without a search filter).
    pub fn from_options(options: &TreeOptions, sections: Vec<Section>) -> Scope {
        Scope {
            hidden_pictures: options.hidden_pictures,
            small_pictures: options.small_pictures,
            small_threshold: options.small_threshold,
            sections,
            now: options.now,
            recent_days: options.recent_days,
        }
    }

    /// Whether a photo shows at all, whatever the search: hidden ones (its own flag or its
    /// folder's) only with Hidden Pictures, small ones only with Small Pictures.
    pub(crate) fn shows(&self, doc: &PhotoDoc, folder_hidden: bool) -> bool {
        let small = doc.width > 0 && doc.height > 0 && doc.width < self.small_threshold && doc.height < self.small_threshold;
        (self.hidden_pictures || !(doc.hidden || folder_hidden)) && (self.small_pictures || !small)
    }

    /// The oldest updated time Recently Updated keeps, if it has a clock.
    pub(crate) fn recent_since(&self) -> Option<Timestamp> {
        let span = std::time::Duration::from_secs(u64::from(self.recent_days).saturating_mul(86_400));
        self.now.map(|now| now.saturating_sub(span))
    }
}

/// Whether a photo passes the filter buttons and the age (`cutoff` from [`Age::cutoff`]).
pub(crate) fn passes(doc: &PhotoDoc, filters: &Filters, age: Option<&Age>, cutoff: Option<DateTime>) -> bool {
    if (filters.faces && !doc.faces) || (filters.starred && !doc.star) || (filters.videos && !doc.video) || (filters.geotagged && !doc.geotagged) {
        return false;
    }
    match (age, cutoff) {
        (Some(_), Some(cutoff)) => doc.sort_date.is_some_and(|d| d >= cutoff),
        _ => true,
    }
}

/// Whether the text of `parsed` matches `photo`, in `folder`, in `albums`, with the faces of
/// `people` (the matcher's rule): every word that is not negated matches it and no negated word
/// does. A word matches when each of its tokens starts one of the photo's terms in the word's
/// field (ignoring case): for plain words its file name, caption, tags, date words, camera, pixel
/// size, its folder's name, description and place, its albums' names and its people's names and
/// nicknames; for `person:` its people's names and nicknames.
pub fn text_matches(parsed: &Parsed, photo: &PhotoDoc, folder: Option<&FolderDoc>, albums: &[&AlbumDoc], people: &[&PersonDoc]) -> bool {
    let mut any: Option<BTreeSet<String>> = None;
    let mut tags: Option<BTreeSet<String>> = None;
    let mut camera: Option<BTreeSet<String>> = None;
    let mut names: Option<BTreeSet<String>> = None;
    for atom in &parsed.atoms {
        let terms: &BTreeSet<String> = match atom.field {
            Field::Any => any.get_or_insert_with(|| {
                let mut all = photo.own_terms();
                all.extend(photo.tag_terms());
                all.extend(photo.camera_terms());
                all.extend(folder.map(FolderDoc::terms).unwrap_or_default());
                for a in albums {
                    all.extend(a.terms());
                }
                for p in people {
                    all.extend(p.terms());
                }
                all
            }),
            Field::Tag => tags.get_or_insert_with(|| photo.tag_terms()),
            Field::Camera => camera.get_or_insert_with(|| photo.camera_terms()),
            Field::Person => names.get_or_insert_with(|| people.iter().flat_map(|p| p.terms()).collect()),
            Field::Color => {
                if atom.negated {
                    continue;
                }
                return false;
            }
        };
        let hit = atom.tokens.iter().all(|t| terms.iter().any(|term| term.starts_with(t.as_str())));
        if hit == atom.negated {
            return false;
        }
    }
    true
}

impl Extract {
    /// The photos `query` matches under `scope`, by id, worked out photo by photo with
    /// [`text_matches`]: the reference for the index (and enough for a handful of photos).
    pub fn matching(&self, query: &Query, scope: &Scope) -> Vec<PhotoId> {
        let parsed = Parsed::new(&query.text);
        let cutoff = query.age.as_ref().and_then(Age::cutoff);
        let folders: HashMap<FolderId, &FolderDoc> = self.folders.iter().map(|f| (f.id, f)).collect();
        let albums: HashMap<&dalica_core::AlbumId, &AlbumDoc> = self.albums.iter().map(|a| (&a.id, a)).collect();
        let people: HashMap<&dalica_core::PersonId, &PersonDoc> = self.people.iter().map(|p| (&p.id, p)).collect();
        let mut out: Vec<PhotoId> = self
            .photos
            .iter()
            .filter(|p| {
                let folder = folders.get(&p.folder).copied();
                let in_albums: Vec<&AlbumDoc> = p.albums.iter().filter_map(|a| albums.get(a).copied()).collect();
                let with: Vec<&PersonDoc> = p.people.iter().filter_map(|id| people.get(id).copied()).collect();
                scope.shows(p, folder.is_some_and(|f| f.hidden))
                    && passes(p, &query.filters, query.age.as_ref(), cutoff)
                    && text_matches(&parsed, p, folder, &in_albums, &with)
            })
            .map(|p| p.id)
            .collect();
        out.sort();
        out
    }
}
