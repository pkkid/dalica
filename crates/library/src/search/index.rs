//! The search index (`organizing.md` §8.1): sorted terms → photo sets, a prefix found by a range
//! search in the sorted terms, built from an [`Extract`] and kept in step by [`Update`]s photo by
//! photo, so the engine can keep it on a worker.
//!
//! Photos live in slots (reused after a removal) and every term maps to the set of slots holding
//! it. A folder's name, description and place, an album's name and a person's name and nickname
//! are terms of the folder, the album or the person, found through its photos, so renaming one
//! touches one record. A query
//! word becomes a bit set per token (the union of every term the token starts), the tokens of a
//! word are intersected, the words intersected (or, for `-word`, subtracted) from the photos the
//! filters and the scope let through. Each folder's photos are sorted once in the folder's own
//! sort and kept until one of them changes, so the result comes out in light-table order without
//! sorting it.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::Bound;
use std::sync::OnceLock;

use dalica_core::{AlbumId, FolderId, PersonId, PhotoId};
use serde::Serialize;

use super::extract::{AlbumDoc, Extract, FolderDoc, PersonDoc, PhotoDoc, Update};
use super::grammar::{Atom, Field, MAX_QUERY, Parsed, cut, is_word_separator, terms};
use super::matcher::{Age, Query, Scope, passes};
use crate::tags::fold;
use crate::{AlbumRef, PhotoSort, Section, SortBy};

/// The most suggestions [`SearchIndex::suggest`] gives.
pub const MAX_SUGGESTIONS: usize = 100;

/// Terms in sorted order, each with the set of things holding it.
type TermMap<K> = BTreeMap<String, BTreeSet<K>>;

fn link<K: Ord + Copy>(map: &mut TermMap<K>, term: &str, key: K) {
    match map.get_mut(term) {
        Some(set) => {
            set.insert(key);
        }
        None => {
            map.insert(term.to_string(), BTreeSet::from([key]));
        }
    }
}

fn unlink<K: Ord>(map: &mut TermMap<K>, term: &str, key: &K) {
    if let Some(set) = map.get_mut(term) {
        set.remove(key);
        if set.is_empty() {
            map.remove(term);
        }
    }
}

/// Moves `key` from the terms of `old` to those of `new`, touching only the terms that differ.
fn relink<K: Ord + Copy>(map: &mut TermMap<K>, key: K, old: &BTreeSet<String>, new: &BTreeSet<String>) {
    for t in old.difference(new) {
        unlink(map, t, &key);
    }
    for t in new.difference(old) {
        link(map, t, key);
    }
}

/// The entries of `map` whose term starts with `prefix` (a range search in the sorted terms).
fn with_prefix<'a, K>(map: &'a TermMap<K>, prefix: &'a str) -> impl Iterator<Item = &'a BTreeSet<K>> + 'a {
    map.range::<str, _>((Bound::Included(prefix), Bound::Unbounded)).take_while(move |(t, _)| t.starts_with(prefix)).map(|(_, set)| set)
}

/// A set of slots.
#[derive(Clone, Debug)]
struct Bits {
    words: Vec<u64>,
}

impl Bits {
    fn new(slots: usize) -> Bits {
        Bits { words: vec![0; slots.div_ceil(64)] }
    }

    fn set(&mut self, slot: u32) {
        let i = slot as usize;
        if let Some(w) = self.words.get_mut(i / 64) {
            *w |= 1u64 << (i % 64);
        }
    }

    fn get(&self, slot: u32) -> bool {
        let i = slot as usize;
        self.words.get(i / 64).is_some_and(|w| w & (1u64 << (i % 64)) != 0)
    }

    fn and(&mut self, other: &Bits) {
        for (a, b) in self.words.iter_mut().zip(other.words.iter().chain(std::iter::repeat(&0))) {
            *a &= b;
        }
    }

    fn and_not(&mut self, other: &Bits) {
        for (a, b) in self.words.iter_mut().zip(other.words.iter()) {
            *a &= !b;
        }
    }

    fn is_empty(&self) -> bool {
        self.words.iter().all(|w| *w == 0)
    }

    fn ones(&self) -> impl Iterator<Item = u32> + '_ {
        self.words.iter().enumerate().flat_map(|(i, w)| {
            let mut w = *w;
            std::iter::from_fn(move || {
                if w == 0 {
                    return None;
                }
                let bit = w.trailing_zeros();
                w &= w - 1;
                u32::try_from(i.saturating_mul(64).saturating_add(bit as usize)).ok()
            })
        })
    }
}

#[derive(Clone, Debug, Default)]
struct FolderEntry {
    doc: Option<FolderDoc>,
    photos: BTreeSet<u32>,
    /// `photos` in the folder's own sort, worked out on first use after a change.
    order: OnceLock<Vec<u32>>,
}

#[derive(Clone, Debug, Default)]
struct AlbumEntry {
    doc: Option<AlbumDoc>,
    photos: BTreeSet<u32>,
}

#[derive(Clone, Debug, Default)]
struct PersonEntry {
    doc: Option<PersonDoc>,
    photos: BTreeSet<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct TagEntry {
    /// Each spelling in use with how many photos use it.
    spellings: BTreeMap<String, usize>,
    photos: BTreeSet<u32>,
}

impl TagEntry {
    /// The spelling most photos use (ties: the first in byte order), as the tag index shows it.
    fn spelling(&self) -> &str {
        let max = self.spellings.values().copied().max().unwrap_or(0);
        self.spellings.iter().find(|(_, n)| **n == max).map_or("", |(s, _)| s.as_str())
    }
}

/// How many photos of a section match, for the light table's sections in search mode.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionCount {
    pub section: Section,
    pub count: usize,
}

/// What a search finds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    /// The matching photos in light-table order: by folder in the scope's section order (then
    /// the other folders by path), each folder's photos in its own sort. The search album lists
    /// them in this order.
    pub ids: Vec<PhotoId>,
    /// The sections with matches and their counts: the search album first (when anything
    /// matches), then the scope's sections that keep a match, then folders the scope left out.
    pub sections: Vec<SectionCount>,
    /// How many folders hold a match (the banner's "in 4 folders").
    pub folders: usize,
    /// The matches' bytes on disk.
    pub bytes: u64,
    /// Operators and filters used that match nothing for want of data: `person` while the library
    /// has no people, `color` (not built yet), `faces` while no photo has a face.
    pub unavailable: Vec<&'static str>,
    /// Whether the text was cut at the grammar's caps.
    pub truncated: bool,
}

impl SearchResult {
    /// How many photos match.
    pub fn count(&self) -> usize {
        self.ids.len()
    }
}

/// What a suggestion names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SuggestionKind {
    Tag,
    Folder,
    Album,
}

/// One line of the search box's autocomplete: a tag, folder or album with how many photos show
/// under it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub kind: SuggestionKind,
    /// The tag (in the spelling most photos use), the folder's name or the album's name.
    pub text: String,
    pub count: usize,
    /// The folder's or album's section; `None` for a tag.
    pub section: Option<Section>,
}

/// The search index of one library (see the module docs).
#[derive(Clone, Debug, Default)]
pub struct SearchIndex {
    docs: Vec<Option<PhotoDoc>>,
    slot_of: HashMap<PhotoId, u32>,
    free: Vec<u32>,
    folders: BTreeMap<FolderId, FolderEntry>,
    albums: BTreeMap<AlbumId, AlbumEntry>,
    /// File names, captions, date words and pixel sizes.
    own: TermMap<u32>,
    /// Tag words (`tag:`).
    tags: TermMap<u32>,
    /// Camera make and model words (`camera:`).
    camera: TermMap<u32>,
    folder_terms: TermMap<FolderId>,
    album_terms: TermMap<AlbumId>,
    people: BTreeMap<PersonId, PersonEntry>,
    /// People's name and nickname words (`person:`, and plain words).
    person_terms: TermMap<PersonId>,
    /// How many photos have a face the Faces filter counts.
    with_faces: usize,
    /// Whole tags (folded), for suggestions.
    tag_names: BTreeMap<String, TagEntry>,
}

impl SearchIndex {
    /// The index of `extract`.
    pub fn build(extract: Extract) -> SearchIndex {
        let mut index = SearchIndex::default();
        for f in extract.folders {
            index.set_folder(f.id, Some(f));
        }
        for a in extract.albums {
            index.set_album(a.id.clone(), Some(a));
        }
        for p in extract.people {
            index.set_person(p.id.clone(), Some(p));
        }
        index.docs.reserve(extract.photos.len());
        for p in extract.photos {
            index.set_photo(p.id, Some(p));
        }
        index
    }

    /// The index of a whole library.
    pub fn from_library(library: &crate::Library) -> SearchIndex {
        SearchIndex::build(Extract::from_library(library))
    }

    /// How many photos it holds.
    pub fn len(&self) -> usize {
        self.slot_of.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slot_of.is_empty()
    }

    /// Photo `id`'s record, if the index holds it.
    pub fn photo(&self, id: PhotoId) -> Option<&PhotoDoc> {
        self.slot_of.get(&id).and_then(|s| self.doc(*s))
    }

    fn doc(&self, slot: u32) -> Option<&PhotoDoc> {
        self.docs.get(slot as usize).and_then(Option::as_ref)
    }

    /// Brings the index in step with one op ([`Update::for_op`]).
    pub fn apply(&mut self, update: Update) {
        for id in update.purged_folders {
            self.set_folder(id, None);
        }
        for id in update.purged_albums {
            self.set_album(id, None);
        }
        for id in update.purged_people {
            self.set_person(id, None);
        }
        for (id, doc) in update.folders {
            self.set_folder(id, doc);
        }
        for (id, doc) in update.albums {
            self.set_album(id, doc);
        }
        for (id, doc) in update.people {
            self.set_person(id, doc);
        }
        for (id, doc) in update.photos {
            self.set_photo(id, doc);
        }
    }

    /// Sets folder `id`'s record; `None` removes the folder and every photo in it.
    fn set_folder(&mut self, id: FolderId, doc: Option<FolderDoc>) {
        let Some(doc) = doc else {
            let slots: Vec<u32> = self.folders.get(&id).map(|e| e.photos.iter().copied().collect()).unwrap_or_default();
            for slot in slots {
                if let Some(pid) = self.doc(slot).map(|d| d.id) {
                    self.set_photo(pid, None);
                }
            }
            if let Some(old) = self.folders.remove(&id).and_then(|e| e.doc) {
                relink(&mut self.folder_terms, id, &old.terms(), &BTreeSet::new());
            }
            return;
        };
        let entry = self.folders.entry(id).or_default();
        let old_terms = entry.doc.as_ref().map(FolderDoc::terms).unwrap_or_default();
        if entry.doc.as_ref().is_none_or(|old| old.sort != doc.sort) {
            entry.order = OnceLock::new();
        }
        let new_terms = doc.terms();
        entry.doc = Some(doc);
        relink(&mut self.folder_terms, id, &old_terms, &new_terms);
    }

    /// Sets album `id`'s record; `None` removes the album and every membership.
    fn set_album(&mut self, id: AlbumId, doc: Option<AlbumDoc>) {
        let Some(doc) = doc else {
            if let Some(entry) = self.albums.remove(&id) {
                for slot in &entry.photos {
                    if let Some(Some(d)) = self.docs.get_mut(*slot as usize) {
                        d.albums.retain(|a| *a != id);
                    }
                }
                if let Some(old) = entry.doc {
                    for t in old.terms() {
                        unlink(&mut self.album_terms, &t, &id);
                    }
                }
            }
            return;
        };
        let entry = self.albums.entry(id.clone()).or_default();
        let old_terms = entry.doc.as_ref().map(AlbumDoc::terms).unwrap_or_default();
        let new_terms = doc.terms();
        entry.doc = Some(doc);
        for t in old_terms.difference(&new_terms) {
            unlink(&mut self.album_terms, t, &id);
        }
        for t in new_terms.difference(&old_terms) {
            match self.album_terms.get_mut(t) {
                Some(set) => {
                    set.insert(id.clone());
                }
                None => {
                    self.album_terms.insert(t.clone(), BTreeSet::from([id.clone()]));
                }
            }
        }
    }

    /// Sets person `id`'s record; `None` removes the person and their name from every photo.
    fn set_person(&mut self, id: PersonId, doc: Option<PersonDoc>) {
        let Some(doc) = doc else {
            if let Some(entry) = self.people.remove(&id) {
                for slot in &entry.photos {
                    if let Some(Some(d)) = self.docs.get_mut(*slot as usize) {
                        d.people.retain(|p| *p != id);
                    }
                }
                if let Some(old) = entry.doc {
                    for t in old.terms() {
                        unlink(&mut self.person_terms, &t, &id);
                    }
                }
            }
            return;
        };
        let entry = self.people.entry(id.clone()).or_default();
        let old_terms = entry.doc.as_ref().map(PersonDoc::terms).unwrap_or_default();
        let new_terms = doc.terms();
        entry.doc = Some(doc);
        for t in old_terms.difference(&new_terms) {
            unlink(&mut self.person_terms, t, &id);
        }
        for t in new_terms.difference(&old_terms) {
            match self.person_terms.get_mut(t) {
                Some(set) => {
                    set.insert(id.clone());
                }
                None => {
                    self.person_terms.insert(t.clone(), BTreeSet::from([id.clone()]));
                }
            }
        }
    }

    /// Sets photo `id`'s record; `None` removes it. Only the terms and sets that change are
    /// touched.
    fn set_photo(&mut self, id: PhotoId, new: Option<PhotoDoc>) {
        let slot = match (self.slot_of.get(&id).copied(), &new) {
            (Some(slot), _) => slot,
            (None, Some(_)) => match self.free.pop() {
                Some(slot) => slot,
                None => match u32::try_from(self.docs.len()) {
                    Ok(slot) => {
                        self.docs.push(None);
                        slot
                    }
                    Err(_) => return,
                },
            },
            (None, None) => return,
        };
        let old = self.docs.get_mut(slot as usize).and_then(Option::take);
        self.relink_photo(slot, old.as_ref(), new.as_ref());
        match new {
            Some(doc) => {
                if let Some(cell) = self.docs.get_mut(slot as usize) {
                    *cell = Some(doc);
                }
                self.slot_of.insert(id, slot);
            }
            None => {
                self.slot_of.remove(&id);
                self.free.push(slot);
            }
        }
    }

    fn relink_photo(&mut self, slot: u32, old: Option<&PhotoDoc>, new: Option<&PhotoDoc>) {
        let terms_of = |d: Option<&PhotoDoc>, f: fn(&PhotoDoc) -> BTreeSet<String>| d.map(f).unwrap_or_default();
        relink(&mut self.own, slot, &terms_of(old, PhotoDoc::own_terms), &terms_of(new, PhotoDoc::own_terms));
        relink(&mut self.tags, slot, &terms_of(old, PhotoDoc::tag_terms), &terms_of(new, PhotoDoc::tag_terms));
        relink(&mut self.camera, slot, &terms_of(old, PhotoDoc::camera_terms), &terms_of(new, PhotoDoc::camera_terms));
        // Whole tags with their spellings.
        if old.map(|d| &d.tags) != new.map(|d| &d.tags) {
            for tag in old.map(|d| d.tags.as_slice()).unwrap_or_default() {
                let key = fold(tag);
                if let Some(entry) = self.tag_names.get_mut(&key) {
                    entry.photos.remove(&slot);
                    if let Some(n) = entry.spellings.get_mut(tag) {
                        *n = n.saturating_sub(1);
                        if *n == 0 {
                            entry.spellings.remove(tag);
                        }
                    }
                    if entry.photos.is_empty() {
                        self.tag_names.remove(&key);
                    }
                }
            }
            for tag in new.map(|d| d.tags.as_slice()).unwrap_or_default() {
                let entry = self.tag_names.entry(fold(tag)).or_default();
                entry.photos.insert(slot);
                let n = entry.spellings.entry(tag.clone()).or_default();
                *n = n.saturating_add(1);
            }
        }
        // Its folder, and the folder's sorted order when a sort key changed.
        let sort_key = |d: &PhotoDoc| (d.folder, d.sort_date, d.name.clone(), d.size);
        if old.map(sort_key) != new.map(sort_key) {
            if let Some(d) = old {
                let gone = match self.folders.get_mut(&d.folder) {
                    Some(entry) => {
                        entry.photos.remove(&slot);
                        entry.order = OnceLock::new();
                        entry.doc.is_none() && entry.photos.is_empty()
                    }
                    None => false,
                };
                if gone {
                    self.folders.remove(&d.folder);
                }
            }
            if let Some(d) = new {
                let entry = self.folders.entry(d.folder).or_default();
                entry.photos.insert(slot);
                entry.order = OnceLock::new();
            }
        }
        // Its albums.
        let none: &[AlbumId] = &[];
        let old_albums: BTreeSet<&AlbumId> = old.map_or(none, |d| d.albums.as_slice()).iter().collect();
        let new_albums: BTreeSet<&AlbumId> = new.map_or(none, |d| d.albums.as_slice()).iter().collect();
        for a in old_albums.difference(&new_albums) {
            let gone = match self.albums.get_mut(*a) {
                Some(entry) => {
                    entry.photos.remove(&slot);
                    entry.doc.is_none() && entry.photos.is_empty()
                }
                None => false,
            };
            if gone {
                self.albums.remove(*a);
            }
        }
        for a in new_albums.difference(&old_albums) {
            self.albums.entry((*a).clone()).or_default().photos.insert(slot);
        }
        // Its people.
        let none: &[PersonId] = &[];
        let old_people: BTreeSet<&PersonId> = old.map_or(none, |d| d.people.as_slice()).iter().collect();
        let new_people: BTreeSet<&PersonId> = new.map_or(none, |d| d.people.as_slice()).iter().collect();
        for p in old_people.difference(&new_people) {
            let gone = match self.people.get_mut(*p) {
                Some(entry) => {
                    entry.photos.remove(&slot);
                    entry.doc.is_none() && entry.photos.is_empty()
                }
                None => false,
            };
            if gone {
                self.people.remove(*p);
            }
        }
        for p in new_people.difference(&old_people) {
            self.people.entry((*p).clone()).or_default().photos.insert(slot);
        }
        // Whether it counts for the Faces filter.
        match (old.is_some_and(|d| d.faces), new.is_some_and(|d| d.faces)) {
            (false, true) => self.with_faces = self.with_faces.saturating_add(1),
            (true, false) => self.with_faces = self.with_faces.saturating_sub(1),
            _ => {}
        }
    }

    /// Whether the photo in `slot` shows under `scope` (hidden and small pictures).
    fn shows(&self, slot: u32, scope: &Scope) -> bool {
        self.doc(slot).is_some_and(|d| scope.shows(d, self.folder_hidden(d.folder)))
    }

    fn folder_hidden(&self, id: FolderId) -> bool {
        self.folders.get(&id).and_then(|e| e.doc.as_ref()).is_some_and(|f| f.hidden)
    }

    /// The slots `parsed`, the filters and the age let through under `scope`.
    fn matched(&self, parsed: &Parsed, query: &Query, scope: &Scope) -> Bits {
        // The words that must match narrow the photos first; the filters and the scope then look
        // only at what is left, and the negated words take theirs out.
        let mut candidates: Option<Bits> = None;
        for atom in parsed.atoms.iter().filter(|a| !a.negated) {
            let hits = self.atom_bits(atom);
            let next = match candidates {
                None => hits,
                Some(mut c) => {
                    c.and(&hits);
                    c
                }
            };
            let empty = next.is_empty();
            candidates = Some(next);
            if empty {
                break;
            }
        }
        let cutoff = query.age.as_ref().and_then(Age::cutoff);
        let admits = |doc: &PhotoDoc| scope.shows(doc, self.folder_hidden(doc.folder)) && passes(doc, &query.filters, query.age.as_ref(), cutoff);
        let mut bits = Bits::new(self.docs.len());
        match candidates {
            Some(c) => {
                for slot in c.ones() {
                    if self.doc(slot).is_some_and(admits) {
                        bits.set(slot);
                    }
                }
            }
            None => {
                for (slot, doc) in self.docs.iter().enumerate() {
                    let Ok(slot) = u32::try_from(slot) else { break };
                    if doc.as_ref().is_some_and(admits) {
                        bits.set(slot);
                    }
                }
            }
        }
        for atom in parsed.atoms.iter().filter(|a| a.negated) {
            if bits.is_empty() {
                break;
            }
            bits.and_not(&self.atom_bits(atom));
        }
        bits
    }

    /// The slots one word matches (every token must).
    fn atom_bits(&self, atom: &Atom) -> Bits {
        let mut acc: Option<Bits> = None;
        for token in &atom.tokens {
            let hits = self.token_bits(atom.field, token);
            let next = match acc {
                None => hits,
                Some(mut a) => {
                    a.and(&hits);
                    a
                }
            };
            let empty = next.is_empty();
            acc = Some(next);
            if empty {
                break;
            }
        }
        acc.unwrap_or_else(|| Bits::new(0))
    }

    /// The slots with a term in `field` that `token` starts.
    fn token_bits(&self, field: Field, token: &str) -> Bits {
        let mut bits = Bits::new(self.docs.len());
        let maps: &[&TermMap<u32>] = match field {
            Field::Any => &[&self.own, &self.tags, &self.camera],
            Field::Tag => &[&self.tags],
            Field::Camera => &[&self.camera],
            Field::Person => &[],
            Field::Color => return bits,
        };
        for map in maps {
            for set in with_prefix(map, token) {
                for slot in set {
                    bits.set(*slot);
                }
            }
        }
        if matches!(field, Field::Any | Field::Person) {
            let people: HashSet<&PersonId> = with_prefix(&self.person_terms, token).flatten().collect();
            for p in people {
                for slot in self.people.get(p).map(|e| &e.photos).into_iter().flatten() {
                    bits.set(*slot);
                }
            }
        }
        if field == Field::Any {
            let folders: HashSet<FolderId> = with_prefix(&self.folder_terms, token).flatten().copied().collect();
            for f in folders {
                for slot in self.folders.get(&f).map(|e| &e.photos).into_iter().flatten() {
                    bits.set(*slot);
                }
            }
            let albums: HashSet<&AlbumId> = with_prefix(&self.album_terms, token).flatten().collect();
            for a in albums {
                for slot in self.albums.get(a).map(|e| &e.photos).into_iter().flatten() {
                    bits.set(*slot);
                }
            }
        }
        bits
    }

    /// The photos `query` matches under `scope`, by id (the same set as [`Extract::matching`]).
    pub fn matching(&self, query: &Query, scope: &Scope) -> Vec<PhotoId> {
        let bits = self.matched(&Parsed::new(&query.text), query, scope);
        let mut out: Vec<PhotoId> = bits.ones().filter_map(|s| self.doc(s).map(|d| d.id)).collect();
        out.sort();
        out
    }

    /// Folder `id`'s slots in its own photo sort (as [`crate::Library::sorted`] orders them).
    fn folder_order<'a>(&'a self, entry: &'a FolderEntry) -> &'a [u32] {
        entry.order.get_or_init(|| {
            let sort = entry.doc.as_ref().map(|d| d.sort).unwrap_or_default();
            let mut list: Vec<(&PhotoDoc, u32)> = entry.photos.iter().filter_map(|s| self.doc(*s).map(|d| (d, *s))).collect();
            sort_docs(&mut list, sort);
            list.into_iter().map(|(_, s)| s).collect()
        })
    }

    /// Runs `query` under `scope` (see [`SearchResult`]).
    pub fn search(&self, query: &Query, scope: &Scope) -> SearchResult {
        let parsed = Parsed::new(&query.text);
        let bits = self.matched(&parsed, query, scope);
        let mut unavailable = parsed.unavailable.clone();
        if self.people.values().any(|e| e.doc.is_some()) {
            unavailable.retain(|op| *op != Field::Person.name());
        }
        if query.filters.faces && self.with_faces == 0 {
            unavailable.push("faces");
        }
        // Matches per folder, and bytes.
        let mut per_folder: HashMap<FolderId, usize> = HashMap::new();
        let mut bytes: u64 = 0;
        for slot in bits.ones() {
            if let Some(d) = self.doc(slot) {
                *per_folder.entry(d.folder).or_default() += 1;
                bytes = bytes.saturating_add(d.size);
            }
        }
        // The folders in light-table order: the scope's, then the rest by path.
        let mut order: Vec<FolderId> = Vec::with_capacity(per_folder.len());
        let mut placed: HashSet<FolderId> = HashSet::with_capacity(per_folder.len());
        for s in &scope.sections {
            if let Section::Folder(f) = s
                && per_folder.contains_key(f)
                && placed.insert(*f)
            {
                order.push(*f);
            }
        }
        let mut rest: Vec<(Option<&std::path::Path>, FolderId)> = per_folder
            .keys()
            .filter(|f| !placed.contains(f))
            .map(|f| (self.folders.get(f).and_then(|e| e.doc.as_ref()).map(|d| d.path.as_path()), *f))
            .collect();
        rest.sort();
        let rest: Vec<FolderId> = rest.into_iter().map(|(_, f)| f).collect();
        let mut ids: Vec<PhotoId> = Vec::new();
        for f in order.iter().chain(rest.iter()) {
            if let Some(entry) = self.folders.get(f) {
                ids.extend(self.folder_order(entry).iter().filter(|s| bits.get(**s)).filter_map(|s| self.doc(*s).map(|d| d.id)));
            }
        }
        // The sections with matches.
        let mut sections: Vec<SectionCount> = Vec::new();
        if !ids.is_empty() {
            sections.push(SectionCount { section: Section::Album(AlbumRef::Search), count: ids.len() });
        }
        let mut seen: HashSet<&Section> = HashSet::new();
        for s in &scope.sections {
            if !seen.insert(s) {
                continue;
            }
            let count = match s {
                Section::Folder(f) => per_folder.get(f).copied().unwrap_or(0),
                Section::Album(AlbumRef::User(a)) => self.albums.get(a).map_or(0, |e| e.photos.iter().filter(|s| bits.get(**s)).count()),
                Section::Album(AlbumRef::Starred) => bits.ones().filter(|s| self.doc(*s).is_some_and(|d| d.star)).count(),
                Section::Album(AlbumRef::Recent) => match scope.recent_since() {
                    Some(since) => bits.ones().filter(|s| self.doc(*s).is_some_and(|d| d.updated >= since)).count(),
                    None => 0,
                },
                Section::Album(AlbumRef::Search) => 0,
            };
            if count > 0 {
                sections.push(SectionCount { section: s.clone(), count });
            }
        }
        for f in &rest {
            sections.push(SectionCount { section: Section::Folder(*f), count: per_folder.get(f).copied().unwrap_or(0) });
        }
        SearchResult { folders: per_folder.len(), bytes, ids, sections, unavailable, truncated: parsed.truncated }
    }

    /// Suggestions for the word being typed (the last word of `text`): tags, folders and albums a
    /// word of whose name (or whose whole name) starts with it, ignoring case, each with how many
    /// photos show under `scope`, those with none left out; tags, then folders, then albums, each
    /// kind by count (most first), then by name; at most `limit` (and [`MAX_SUGGESTIONS`]). After
    /// `tag:` only tags; after another operator nothing. A leading `-` is ignored.
    pub fn suggest(&self, text: &str, scope: &Scope, limit: usize) -> Vec<Suggestion> {
        let limit = limit.min(MAX_SUGGESTIONS);
        let word = cut(text, MAX_QUERY).rsplit(is_word_separator).next().unwrap_or_default();
        let word = word.strip_prefix('-').unwrap_or(word);
        let (tags_only, value) = match word.split_once(':') {
            Some((name, value)) if name.eq_ignore_ascii_case("tag") => (true, value),
            Some((name, _)) if ["camera", "person", "color"].iter().any(|f| f.eq_ignore_ascii_case(name)) => return Vec::new(),
            _ => (false, word),
        };
        let prefix = value.to_lowercase();
        if prefix.is_empty() || limit == 0 {
            return Vec::new();
        }
        let named = |name: &str| fold(name).starts_with(&prefix) || terms(name).any(|t| t.starts_with(&prefix));
        let shown = |slots: &BTreeSet<u32>| slots.iter().filter(|s| self.shows(**s, scope)).count();
        let ranked = |mut list: Vec<Suggestion>| {
            list.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| fold(&a.text).cmp(&fold(&b.text))).then_with(|| a.text.cmp(&b.text)));
            list
        };
        let mut out: Vec<Suggestion> = Vec::new();
        let tags = self
            .tag_names
            .values()
            .filter(|e| named(e.spelling()))
            .map(|e| Suggestion { kind: SuggestionKind::Tag, text: e.spelling().to_string(), count: shown(&e.photos), section: None })
            .filter(|s| s.count > 0)
            .collect();
        out.extend(ranked(tags));
        if !tags_only {
            let folders = self
                .folders
                .iter()
                .filter_map(|(id, e)| Some((id, e, e.doc.as_ref()?.name())))
                .filter(|(_, _, name)| named(name))
                .map(|(id, e, name)| Suggestion {
                    kind: SuggestionKind::Folder,
                    text: name,
                    count: shown(&e.photos),
                    section: Some(Section::Folder(*id)),
                })
                .filter(|s| s.count > 0)
                .collect();
            out.extend(ranked(folders));
            let albums = self
                .albums
                .iter()
                .filter_map(|(id, e)| Some((id, e, e.doc.as_ref()?.name.as_str())))
                .filter(|(_, _, name)| named(name))
                .map(|(id, e, name)| Suggestion {
                    kind: SuggestionKind::Album,
                    text: name.to_string(),
                    count: shown(&e.photos),
                    section: Some(Section::Album(AlbumRef::User(id.clone()))),
                })
                .filter(|s| s.count > 0)
                .collect();
            out.extend(ranked(albums));
        }
        out.truncate(limit);
        out
    }

    /// The index's content keyed by photo id rather than slot, to compare two indexes built in
    /// different ways.
    fn canonical(&self) -> Canonical<'_> {
        // A slot whose photo is gone shows as `None`, so a stale entry never compares equal.
        let ids = |slots: &BTreeSet<u32>| -> BTreeSet<Option<PhotoId>> { slots.iter().map(|s| self.doc(*s).map(|d| d.id)).collect() };
        let map = |m: &TermMap<u32>| -> BTreeMap<String, BTreeSet<Option<PhotoId>>> { m.iter().map(|(t, s)| (t.clone(), ids(s))).collect() };
        Canonical {
            photos: self.docs.iter().flatten().map(|d| (d.id, d)).collect(),
            slots: self.slot_of.iter().filter(|(id, s)| self.doc(**s).is_some_and(|d| d.id == **id)).count(),
            folders: self.folders.iter().map(|(id, e)| (*id, (e.doc.as_ref(), ids(&e.photos)))).collect(),
            albums: self.albums.iter().map(|(id, e)| (id.clone(), (e.doc.as_ref(), ids(&e.photos)))).collect(),
            own: map(&self.own),
            tags: map(&self.tags),
            camera: map(&self.camera),
            folder_terms: &self.folder_terms,
            album_terms: &self.album_terms,
            people: self.people.iter().map(|(id, e)| (id.clone(), (e.doc.as_ref(), ids(&e.photos)))).collect(),
            person_terms: &self.person_terms,
            with_faces: self.with_faces,
            tag_names: self.tag_names.iter().map(|(t, e)| (t.clone(), (&e.spellings, ids(&e.photos)))).collect(),
        }
    }
}

/// A term map keyed by photo id (`None` for a slot whose photo is gone).
type IdMap = BTreeMap<String, BTreeSet<Option<PhotoId>>>;
/// Whole tags with their spellings and photos, keyed by photo id.
type TagIds<'a> = BTreeMap<String, (&'a BTreeMap<String, usize>, BTreeSet<Option<PhotoId>>)>;

/// [`SearchIndex::canonical`].
#[derive(PartialEq, Eq)]
struct Canonical<'a> {
    photos: BTreeMap<PhotoId, &'a PhotoDoc>,
    slots: usize,
    folders: BTreeMap<FolderId, (Option<&'a FolderDoc>, BTreeSet<Option<PhotoId>>)>,
    albums: BTreeMap<AlbumId, (Option<&'a AlbumDoc>, BTreeSet<Option<PhotoId>>)>,
    own: IdMap,
    tags: IdMap,
    camera: IdMap,
    folder_terms: &'a TermMap<FolderId>,
    album_terms: &'a TermMap<AlbumId>,
    people: BTreeMap<PersonId, (Option<&'a PersonDoc>, BTreeSet<Option<PhotoId>>)>,
    person_terms: &'a TermMap<PersonId>,
    with_faces: usize,
    tag_names: TagIds<'a>,
}

impl PartialEq for SearchIndex {
    /// Two indexes are equal when they hold the same records and every term and set agrees,
    /// whichever slots they use.
    fn eq(&self, other: &SearchIndex) -> bool {
        self.slot_of.len() == other.slot_of.len() && self.canonical() == other.canonical()
    }
}

impl Eq for SearchIndex {}

/// `list` in `sort`'s order, as [`crate::Library::sorted`] orders photos: by date (then file
/// name, then id), by file name ignoring case (then as written, then id), or by bytes (then file
/// name, then id); `reverse` turns all of it around.
fn sort_docs(list: &mut [(&PhotoDoc, u32)], sort: PhotoSort) {
    match sort.by {
        SortBy::Date => list.sort_by(|(a, _), (b, _)| a.sort_date.cmp(&b.sort_date).then_with(|| a.name.cmp(&b.name)).then_with(|| a.id.cmp(&b.id))),
        SortBy::Name => list.sort_by_cached_key(|(d, _)| (d.name.to_lowercase(), d.name.clone(), d.id)),
        SortBy::Size => list.sort_by(|(a, _), (b, _)| a.size.cmp(&b.size).then_with(|| a.name.cmp(&b.name)).then_with(|| a.id.cmp(&b.id))),
    }
    if sort.reverse {
        list.reverse();
    }
}
