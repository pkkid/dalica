//! Tags and the text the user types about photos and folders: the tag index (counts per tag,
//! partial counts over a set of photos, prefix lookup for autocomplete), the case rule, and the
//! length caps of `security.md` §4.4 with the functions that bring tainted text inside them.
//!
//! **The case rule:** two tags are the same tag when they are equal after [`fold`] (Unicode
//! lowercase). A photo never holds two spellings of one tag. Adding a tag that is already in use
//! takes the spelling in use ([`TagIndex::spelling`]), so the first spelling is kept; renaming a
//! tag onto another tag merges the two (`Op::RenameTag`).
//!
//! **The index** is derived from the model and kept in step by every op, so it costs nothing to
//! read: one entry per folded tag with how many photos hold it under each spelling. Spellings only
//! differ for tags imported from files or sidecars; the index shows the one most photos use (ties:
//! the first in byte order), so it is a pure function of the model.

use std::collections::{BTreeMap, HashSet};

use dalica_core::PhotoId;

use crate::model::{FolderInfo, Library, PhotoInfo};
use crate::{Error, Result};

/// The most bytes in a caption (UTF-8).
pub const MAX_CAPTION: usize = 16 * 1024;
/// The most bytes in one tag (UTF-8).
pub const MAX_TAG: usize = 256;
/// The most tags on one photo.
pub const MAX_TAGS: usize = 1_000;
/// The most bytes in a folder description (UTF-8).
pub const MAX_DESCRIPTION: usize = 16 * 1024;
/// The most bytes in a folder place (UTF-8).
pub const MAX_PLACE: usize = 1024;
/// The most operations in a recipe or a redo list (`security.md` §4.2: the sidecar's own limit).
pub const MAX_OPERATIONS: usize = 10_000;
/// The most bytes in one operation string.
pub const MAX_OPERATION: usize = 64 * 1024;

/// The form tags are compared in: Unicode lowercase.
pub fn fold(tag: &str) -> String {
    tag.to_lowercase()
}

/// `text` cut to at most `max` bytes at a character boundary.
fn cut(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default()
}

/// A caption as stored: trimmed and cut to [`MAX_CAPTION`] bytes. Empty means no caption.
pub fn clean_caption(text: &str) -> String {
    cut(text.trim(), MAX_CAPTION).trim_end().to_string()
}

/// A folder description as stored: trimmed and cut to [`MAX_DESCRIPTION`] bytes.
pub fn clean_description(text: &str) -> String {
    cut(text.trim(), MAX_DESCRIPTION).trim_end().to_string()
}

/// A folder place as stored: control characters made spaces, trimmed, cut to [`MAX_PLACE`] bytes.
pub fn clean_place(text: &str) -> String {
    let spaced: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    cut(spaced.trim(), MAX_PLACE).trim_end().to_string()
}

/// A tag as stored: control characters (newlines, tabs) made spaces, trimmed, cut to [`MAX_TAG`]
/// bytes; `None` when nothing is left. Commas are kept: splitting typed text at commas is the
/// caller's (the Tags pane's add field), and tags from files may hold them.
pub fn clean_tag(text: &str) -> Option<String> {
    let spaced: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let tag = cut(spaced.trim(), MAX_TAG).trim_end();
    if tag.is_empty() { None } else { Some(tag.to_string()) }
}

/// Clean tags from `tags`: each through [`clean_tag`], the empty ones dropped, a tag equal to an
/// earlier one ignoring case dropped (the first spelling stays), at most [`MAX_TAGS`].
pub fn clean_tags<'a>(tags: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut seen = HashSet::new();
    tags.into_iter().filter_map(clean_tag).filter(|t| seen.insert(fold(t))).take(MAX_TAGS).collect()
}

fn is_clean_tag(tag: &str) -> bool {
    clean_tag(tag).as_deref() == Some(tag)
}

/// Checks a photo's tag list: clean tags, no two the same ignoring case, at most [`MAX_TAGS`].
pub(crate) fn check_tags(tags: &[String]) -> Result<()> {
    if tags.len() > MAX_TAGS {
        return Err(Error::TooMany { what: "tags on a photo", max: MAX_TAGS });
    }
    let mut seen = HashSet::with_capacity(tags.len());
    for t in tags {
        check_tag(t)?;
        if !seen.insert(fold(t)) {
            return Err(Error::BadText { what: "tag", why: "the same tag twice on one photo" });
        }
    }
    Ok(())
}

/// Checks one tag: what [`clean_tag`] would give back unchanged.
pub(crate) fn check_tag(tag: &str) -> Result<()> {
    if tag.len() > MAX_TAG {
        return Err(Error::TooLong { what: "tag", max: MAX_TAG });
    }
    if !is_clean_tag(tag) {
        return Err(Error::BadText { what: "tag", why: "empty, untrimmed or with control characters" });
    }
    Ok(())
}

pub(crate) fn check_caption(caption: &str) -> Result<()> {
    if caption.len() > MAX_CAPTION { Err(Error::TooLong { what: "caption", max: MAX_CAPTION }) } else { Ok(()) }
}

pub(crate) fn check_list(list: &[String]) -> Result<()> {
    if list.len() > MAX_OPERATIONS {
        return Err(Error::TooMany { what: "operations in a recipe", max: MAX_OPERATIONS });
    }
    if list.iter().any(|op| op.len() > MAX_OPERATION) {
        return Err(Error::TooLong { what: "operation", max: MAX_OPERATION });
    }
    Ok(())
}

/// Checks a whole photo info against the caps.
pub(crate) fn check_info(info: &PhotoInfo) -> Result<()> {
    check_caption(&info.caption)?;
    check_tags(&info.tags)?;
    check_list(&info.edits)?;
    check_list(&info.redo)
}

/// Checks a folder info against the caps.
pub(crate) fn check_folder_info(info: &FolderInfo) -> Result<()> {
    if info.description.len() > MAX_DESCRIPTION {
        return Err(Error::TooLong { what: "folder description", max: MAX_DESCRIPTION });
    }
    if info.place.len() > MAX_PLACE {
        return Err(Error::TooLong { what: "folder place", max: MAX_PLACE });
    }
    Ok(())
}

/// One tag in the index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Entry {
    /// Photos holding the tag under each spelling.
    spellings: BTreeMap<String, usize>,
    /// Photos holding the tag (the sum of `spellings`).
    photos: usize,
}

impl Entry {
    /// The spelling most photos use; ties go to the first in byte order.
    fn spelling(&self) -> &str {
        let mut best: Option<(&String, usize)> = None;
        for (s, &n) in &self.spellings {
            if best.is_none_or(|(_, m)| n > m) {
                best = Some((s, n));
            }
        }
        best.map(|(s, _)| s.as_str()).unwrap_or_default()
    }
}

/// A tag with the number of photos holding it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagCount {
    pub tag: String,
    pub count: usize,
}

/// A tag over a set of photos: `count` of the `of` photos hold it ("beach (3 of 8)").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartialCount {
    pub tag: String,
    pub count: usize,
    pub of: usize,
}

impl PartialCount {
    /// Whether only some of the photos hold the tag.
    pub fn is_partial(&self) -> bool {
        self.count < self.of
    }
}

impl std::fmt::Display for PartialCount {
    /// `beach` when every photo holds it, `beach (3 of 8)` when only some do.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_partial() { write!(f, "{} ({} of {})", self.tag, self.count, self.of) } else { write!(f, "{}", self.tag) }
    }
}

/// Every tag in use, by folded tag. Built and kept by the library ([`Library::tag_index`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TagIndex {
    by_fold: BTreeMap<String, Entry>,
}

impl TagIndex {
    /// How many different tags are in use.
    pub fn len(&self) -> usize {
        self.by_fold.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_fold.is_empty()
    }

    /// How many photos hold `tag` (any spelling).
    pub fn count(&self, tag: &str) -> usize {
        self.by_fold.get(&fold(tag)).map_or(0, |e| e.photos)
    }

    /// The spelling in use for `tag`, if any photo holds it.
    pub fn spelling(&self, tag: &str) -> Option<&str> {
        self.by_fold.get(&fold(tag)).map(Entry::spelling)
    }

    /// Every tag with its count, in folded (case-insensitive) order.
    pub fn counts(&self) -> Vec<TagCount> {
        self.by_fold.values().map(|e| TagCount { tag: e.spelling().to_string(), count: e.photos }).collect()
    }

    /// Up to `limit` tags starting with `prefix` (ignoring case), in folded order, for autocomplete.
    pub fn with_prefix(&self, prefix: &str, limit: usize) -> Vec<TagCount> {
        let key = fold(prefix);
        self.by_fold
            .range(key.clone()..)
            .take_while(|(k, _)| k.starts_with(&key))
            .take(limit)
            .map(|(_, e)| TagCount { tag: e.spelling().to_string(), count: e.photos })
            .collect()
    }

    pub(crate) fn add(&mut self, tags: &[String]) {
        for t in tags {
            let e = self.by_fold.entry(fold(t)).or_default();
            e.photos = e.photos.saturating_add(1);
            let n = e.spellings.entry(t.clone()).or_default();
            *n = n.saturating_add(1);
        }
    }

    pub(crate) fn remove(&mut self, tags: &[String]) {
        for t in tags {
            let key = fold(t);
            let Some(e) = self.by_fold.get_mut(&key) else { continue };
            e.photos = e.photos.saturating_sub(1);
            if let Some(n) = e.spellings.get_mut(t) {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    e.spellings.remove(t);
                }
            }
            if e.photos == 0 || e.spellings.is_empty() {
                self.by_fold.remove(&key);
            }
        }
    }
}

impl Library {
    /// The tags of `ids` with how many of them hold each, in folded order: the Tags pane's "Tags
    /// in the selection". `of` counts the distinct known photos of `ids`; unknown ids are skipped.
    pub fn tag_counts_in(&self, ids: &[PhotoId]) -> Vec<PartialCount> {
        let mut seen = HashSet::with_capacity(ids.len());
        let mut counts: BTreeMap<String, (usize, &str)> = BTreeMap::new();
        for &id in ids {
            if self.photo(id).is_none() || !seen.insert(id) {
                continue;
            }
            for t in self.tags_of(id) {
                counts.entry(fold(t)).or_insert((0, t.as_str())).0 += 1;
            }
        }
        let of = seen.len();
        counts
            .into_iter()
            .map(|(key, (count, first))| {
                let tag = self.tags.by_fold.get(&key).map_or(first, Entry::spelling).to_string();
                PartialCount { tag, count, of }
            })
            .collect()
    }
}
