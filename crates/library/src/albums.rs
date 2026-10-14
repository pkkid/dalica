//! Album order as ops (`data-model.md` §3.2, `organizing.md` §4.3): the position keys an add, a
//! drop between two members, a reorder and Sort Album By give, built from the current model so the
//! caller applies one op.
//!
//! - **Adding** puts the photos after the album's last key (or where they are dropped), in the
//!   order given; photos already in the album are left where they are.
//! - **A drop** between two members takes keys between theirs (`PosKey::between`), one after the
//!   other, so dropping at one spot over and over keeps keys short; only the moved photos get new
//!   keys, so only their sidecars are rewritten.
//! - **Re-keying:** when no key fits (two neighbours with equal keys, possible after merges) or a
//!   key would pass [`MAX_KEY_LEN`] bytes, the whole album gets fresh keys in its new order, in
//!   the same op (one undo step), as Sort Album By does.
//! - **Album ids** are `a` and 12 hex digits drawn from the caller's random source (the engine's
//!   OS random), drawn again on a clash ([`Library::new_album_id`]).

use std::collections::HashSet;
use std::path::Path;

use dalica_core::poskey::MAX_KEY_LEN;
use dalica_core::{AlbumId, PhotoId, PosKey};

use crate::model::{Library, PhotoSort};
use crate::op::Op;
use crate::{Error, Result};

/// The most draws [`Library::new_album_id`] makes before giving up (a clash is one in 2⁴⁸ per
/// album; this many in a row means the random source is broken).
pub const MAX_ID_DRAWS: usize = 64;

/// Where dropped photos go in an album: right before or right after a member.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Anchor {
    Before(PhotoId),
    After(PhotoId),
}

/// An album name as stored: control characters made spaces, trimmed, cut to
/// [`MAX_ALBUM_NAME`](crate::MAX_ALBUM_NAME) bytes at a character boundary; `None` when nothing
/// is left.
pub fn clean_album_name(text: &str) -> Option<String> {
    let spaced: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let trimmed = spaced.trim();
    let mut end = trimmed.len().min(crate::MAX_ALBUM_NAME);
    while end > 0 && !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    let name = trimmed.get(..end).unwrap_or_default().trim_end();
    if name.is_empty() { None } else { Some(name.to_string()) }
}

/// The album id the 48 low bits of `bits` spell: `a` and 12 lowercase hex digits (always the
/// right shape, so always `Some`).
pub fn album_id_from(bits: u64) -> Option<AlbumId> {
    AlbumId::parse(&format!("a{:012x}", bits & 0xffff_ffff_ffff)).ok()
}

impl Library {
    /// A new album id: `a` and 12 hex digits from `draw` (the caller's random source), drawn
    /// again while it names an existing album, at most [`MAX_ID_DRAWS`] times.
    pub fn new_album_id(&self, mut draw: impl FnMut() -> u64) -> Result<AlbumId> {
        for _ in 0..MAX_ID_DRAWS {
            if let Some(id) = album_id_from(draw())
                && !self.model.albums.contains_key(&id)
            {
                return Ok(id);
            }
        }
        Err(Error::TooMany { what: "album id clashes", max: MAX_ID_DRAWS })
    }

    /// The op that adds `ids` to album `album` at `at` (the end when `None`), in the order given;
    /// photos already in the album and repeats are left out. When every photo is a member already
    /// it is an [`Op::AddMembers`] with nothing in it. Refuses an unknown album or photo, and an
    /// anchor that is not a member.
    pub fn album_add_op(&self, album: &AlbumId, ids: &[PhotoId], at: Option<Anchor>) -> Result<Op> {
        self.check_album(album)?;
        let mut seen = HashSet::with_capacity(ids.len());
        let mut new = Vec::with_capacity(ids.len());
        for &id in ids {
            if self.photo(id).is_none() {
                return Err(Error::UnknownPhoto(id));
            }
            if self.member_key(album, id).is_none() && seen.insert(id) {
                new.push(id);
            }
        }
        if new.is_empty() {
            return Ok(Op::AddMembers { members: Vec::new() });
        }
        Ok(match self.place(album, &new, at)? {
            Placed::Keys(keys) => Op::AddMembers { members: new.iter().zip(keys).map(|(&id, key)| (album.clone(), id, key)).collect() },
            Placed::Rekeyed(order) => {
                let keys = fresh_keys(order.len())?;
                let mut set = Vec::new();
                let mut add = Vec::new();
                for (id, key) in order.into_iter().zip(keys) {
                    if seen.contains(&id) {
                        add.push((album.clone(), id, key));
                    } else {
                        set.push((album.clone(), id, key));
                    }
                }
                Op::Batch { ops: vec![Op::SetMemberKeys { keys: set }, Op::AddMembers { members: add }] }
            }
        })
    }

    /// The op that moves members `ids` of album `album` to `at` (the end when `None`), in the
    /// order given (repeats count once). Refuses an unknown album, a photo that is not a member,
    /// and an anchor that is not a member or is one of the moved photos.
    pub fn album_reorder_op(&self, album: &AlbumId, ids: &[PhotoId], at: Option<Anchor>) -> Result<Op> {
        self.check_album(album)?;
        let mut seen = HashSet::with_capacity(ids.len());
        let mut moving = Vec::with_capacity(ids.len());
        for &id in ids {
            if self.member_key(album, id).is_none() {
                return Err(Error::NotMember { album: album.clone(), photo: id });
            }
            if seen.insert(id) {
                moving.push(id);
            }
        }
        let keys = match self.place(album, &moving, at)? {
            Placed::Keys(keys) => moving.into_iter().zip(keys).map(|(id, key)| (album.clone(), id, key)).collect(),
            Placed::Rekeyed(order) => {
                let keys = fresh_keys(order.len())?;
                order.into_iter().zip(keys).map(|(id, key)| (album.clone(), id, key)).collect()
            }
        };
        Ok(Op::SetMemberKeys { keys })
    }

    /// Sort Album By ▸: the op that rewrites every key of album `album` in the order `sort` gives
    /// (`data-model.md` §3.2: one undo step).
    pub fn album_sort_op(&self, album: &AlbumId, sort: PhotoSort) -> Result<Op> {
        self.check_album(album)?;
        let order = self.sorted(self.album_members(album).iter().filter_map(|id| self.photo(*id)), sort);
        let keys = fresh_keys(order.len())?;
        Ok(Op::SetMemberKeys { keys: order.into_iter().zip(keys).map(|(id, key)| (album.clone(), id, key)).collect() })
    }

    /// The op that gives album `album` fresh, short keys in its current order.
    pub fn album_rekey_op(&self, album: &AlbumId) -> Result<Op> {
        self.check_album(album)?;
        let order = self.album_members(album).to_vec();
        let keys = fresh_keys(order.len())?;
        Ok(Op::SetMemberKeys { keys: order.into_iter().zip(keys).map(|(id, key)| (album.clone(), id, key)).collect() })
    }

    fn check_album(&self, album: &AlbumId) -> Result<()> {
        if self.album(album).is_none() { Err(Error::UnknownAlbum(album.clone())) } else { Ok(()) }
    }

    /// Keys for `moving` (not empty, no repeats) placed at `at` among the other members of
    /// `album`, or the album's whole new order when it must be re-keyed.
    fn place(&self, album: &AlbumId, moving: &[PhotoId], at: Option<Anchor>) -> Result<Placed> {
        let set: HashSet<PhotoId> = moving.iter().copied().collect();
        if let Some(Anchor::Before(a) | Anchor::After(a)) = at
            && (set.contains(&a) || self.member_key(album, a).is_none())
        {
            return Err(Error::NotMember { album: album.clone(), photo: a });
        }
        // The two neighbours of the gap, found through the key index without sorting the album:
        // the last member before it and the first after it, in album order.
        let anchor = |x: PhotoId| self.member_key(album, x).map(|k| (k, x));
        let (lo, hi) = match at {
            None => (self.next_to(album, None, false, &set), None),
            Some(Anchor::Before(x)) => (self.next_to(album, anchor(x), false, &set), anchor(x).map(|(k, x)| (x, k.clone()))),
            Some(Anchor::After(x)) => (anchor(x).map(|(k, x)| (x, k.clone())), self.next_to(album, anchor(x), true, &set)),
        };
        if let Some(keys) = keys_between(lo.map(|(_, k)| k), hi.as_ref().map(|(_, k)| k), moving.len()) {
            return Ok(Placed::Keys(keys));
        }
        // No key fits: the whole album in its new order, to re-key.
        let mut order: Vec<PhotoId> = self.album_members(album).iter().copied().filter(|id| !set.contains(id)).collect();
        let index = match hi {
            Some((h, _)) => order.iter().position(|id| *id == h).unwrap_or(order.len()),
            None => order.len(),
        };
        let tail = order.split_off(index);
        order.extend_from_slice(moving);
        order.extend(tail);
        Ok(Placed::Rekeyed(order))
    }

    /// The member of `album` right before (`after` false) or right after the point `at` (a key
    /// and its photo; `None` is past the end) in album order, leaving out `skip`. Album order is
    /// by key and, between equal keys, by folder path, file name and id ([`Library::place_of`]).
    fn next_to(&self, album: &AlbumId, at: Option<(&PosKey, PhotoId)>, after: bool, skip: &HashSet<PhotoId>) -> Option<(PhotoId, PosKey)> {
        let index = self.by_key.get(album)?;
        let group =
            |key: &PosKey| index.range((key.clone(), PhotoId::new(0))..=(key.clone(), PhotoId::new(u64::MAX))).filter(|(_, id)| !skip.contains(id));
        let extreme = |list: Vec<&(PosKey, PhotoId)>| -> Option<(PhotoId, PosKey)> {
            let pick = if after {
                list.into_iter().min_by_key(|(_, id)| self.place_of(*id))
            } else {
                list.into_iter().max_by_key(|(_, id)| self.place_of(*id))
            };
            pick.map(|(k, id)| (*id, k.clone()))
        };
        // First the members with the same key on the right side of `at`.
        if let Some((key, x)) = at {
            let px = self.place_of(x);
            let same: Vec<&(PosKey, PhotoId)> =
                group(key).filter(|(_, id)| *id != x && if after { self.place_of(*id) > px } else { self.place_of(*id) < px }).collect();
            if !same.is_empty() {
                return extreme(same);
            }
        }
        // Then the nearest key beyond it, and the extreme of that key's members.
        let beyond = match (at, after) {
            (None, false) => index.iter().rev().find(|(_, id)| !skip.contains(id)),
            (None, true) => index.iter().find(|(_, id)| !skip.contains(id)),
            (Some((key, _)), false) => index.range(..(key.clone(), PhotoId::new(0))).rev().find(|(_, id)| !skip.contains(id)),
            (Some((key, _)), true) => index
                .range((std::ops::Bound::Excluded((key.clone(), PhotoId::new(u64::MAX))), std::ops::Bound::Unbounded))
                .find(|(_, id)| !skip.contains(id)),
        }?;
        extreme(group(&beyond.0).collect())
    }

    /// Where a photo sorts among members with equal keys: its folder path, file name and id.
    fn place_of(&self, id: PhotoId) -> (Option<&Path>, &str, PhotoId) {
        let p = self.photo(id);
        (p.and_then(|p| self.folder(p.folder)).map(|f| f.path.as_path()), p.map_or("", |p| p.name.as_str()), id)
    }
}

/// Where [`Library::place`] put the photos.
enum Placed {
    /// One key per moved photo, in order.
    Keys(Vec<PosKey>),
    /// No key fits: the album's whole order, to re-key.
    Rekeyed(Vec<PhotoId>),
}

/// `n` keys in order strictly after `lo` (if any) and before `hi` (if any), each placed right
/// after the one before; `None` when no key fits within [`MAX_KEY_LEN`] bytes.
fn keys_between(lo: Option<PosKey>, hi: Option<&PosKey>, n: usize) -> Option<Vec<PosKey>> {
    let mut out = Vec::with_capacity(n);
    let mut prev = lo;
    for _ in 0..n {
        let key = match (&prev, hi) {
            (None, None) => Ok(PosKey::first()),
            (Some(a), None) => a.after(),
            (None, Some(b)) => b.before(),
            (Some(a), Some(b)) => PosKey::between(a, b),
        }
        .ok()
        .filter(|k| k.as_str().len() <= MAX_KEY_LEN)?;
        out.push(key.clone());
        prev = Some(key);
    }
    Some(out)
}

/// `n` short keys in order: the first key, then each one after the last.
fn fresh_keys(n: usize) -> Result<Vec<PosKey>> {
    keys_between(None, None, n).ok_or(Error::TooMany { what: "photos in one album", max: n })
}
