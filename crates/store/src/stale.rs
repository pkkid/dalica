//! Stale sidecar entries (`data-model.md` §3, §7): a photo deleted outside Dalica leaves the
//! library at once, but its entry in the folder's `.dalica.json` (its star, and whatever else the
//! user gave it) is kept for [`PRUNE_AFTER`], in case the file comes back (restored from the
//! trash, a volume or sync folder that catches up).
//!
//! The sidecar itself remembers since when: `dalica.missing` maps each such entry's file name to
//! the UTC time Dalica first found its file gone (`"2026-10-12T05:15:00Z"`). [`sweep`] runs on
//! every sidecar write (the writer calls it after merging the model's values), so pruning happens
//! on the folder's next write once the time has passed. An entry is stale only when all of these
//! hold, checked again at every write:
//!
//! - it is an object holding Dalica's own data (a key the model writes, [`crate::mapping`]: the
//!   star, M6's fields, `savedAs`, M9's `albums` and M13's `faces` and `geo`, a `"geo": null`
//!   included), so an entry only another program wrote is never touched;
//! - its name is a plain file name ([`dalica_sidecar::plain_name`]): a hostile key never reaches
//!   a path;
//! - the model holds no photo of that name in the folder, and
//! - no file of that name is in the folder (a file the scans skip, too small or of a type turned
//!   off, keeps its entry; so does any entry whose file cannot be looked up).
//!
//! A name that stops being stale (the photo is back, the entry or its data went) loses its mark. A
//! mark that is not a time this module wrote is replaced by the time of the write (the wait starts
//! again: never shorter). At most [`MAX_EXAMINED`] candidates are looked at per write; the rest are
//! left as they are.

use std::collections::BTreeSet;
use std::time::Duration;

use dalica_core::{DateTime, Timestamp};
use serde_json::{Map, Value};

use crate::mapping::{FACE_FIELDS, PHOTO_DATA_M9, PHOTO_DATA_M13, PHOTO_FIELDS, PHOTO_INFO_FIELDS, SAVED_AS_FIELDS, Source};

/// How long an entry whose file is gone is kept.
pub const PRUNE_AFTER: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// The key under `dalica` that remembers when each stale entry's file was first found gone.
pub const MISSING_KEY: &str = "missing";
/// The most candidate entries one write looks at (each costs a look-up in the folder).
pub const MAX_EXAMINED: usize = 4096;

/// What one [`sweep`] did, by file name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sweep {
    /// Entries newly found stale: their time starts now.
    pub marked: Vec<String>,
    /// Entries removed: stale for at least [`PRUNE_AFTER`].
    pub pruned: Vec<String>,
    /// Marks dropped: the entry is no longer stale (or no longer there).
    pub cleared: Vec<String>,
}

impl Sweep {
    /// Whether the sidecar changed.
    pub fn changed(&self) -> bool {
        !(self.marked.is_empty() && self.pruned.is_empty() && self.cleared.is_empty())
    }
}

/// `2026-10-12T05:15:00Z` for `t` (whole seconds, UTC); `None` outside years 1..=9999.
pub fn stamp(t: Timestamp) -> Option<String> {
    DateTime::from_unix_secs(t.as_secs()).map(|d| format!("{d}Z"))
}

/// A mark as [`stamp`] writes it, in that same form; `None` for anything else.
fn read_stamp(v: &Value) -> Option<String> {
    let s = v.as_str()?;
    let d: DateTime = s.strip_suffix('Z')?.parse().ok()?;
    Some(format!("{d}Z"))
}

/// The keys under `photos.<name>` that hold Dalica's own data: the star and every field of what
/// the user said about the photo (caption, tags, hidden, recipe, redo list, date), and the copy a
/// Save wrote beside it (`savedAs`, M8), its album memberships (`albums`, M9), its faces and its
/// geotag (`faces`, `geo`, M13).
fn dalica_keys() -> impl Iterator<Item = &'static str> {
    let mut keys: Vec<&'static str> = PHOTO_FIELDS
        .iter()
        .chain(&PHOTO_INFO_FIELDS)
        .chain(&SAVED_AS_FIELDS)
        .chain(&PHOTO_DATA_M9)
        .chain(&PHOTO_DATA_M13)
        .chain(&FACE_FIELDS)
        .filter_map(|(_, s)| match s {
            Source::Sidecar(key) => Some(*key),
            _ => None,
        })
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter()
}

/// Whether the sidecar (as JSON) may need a sweep: it has an entry the model does not hold, or
/// marks to look at. Cheap; lets the writer skip [`sweep`] for most folders.
pub fn wanted(root: &Map<String, Value>, model: &BTreeSet<&str>) -> bool {
    let entries = root.get("photos").and_then(Value::as_object).is_some_and(|p| p.keys().any(|n| !model.contains(n.as_str())));
    entries || root.get("dalica").and_then(|d| d.get(MISSING_KEY)).is_some()
}

/// Marks, keeps or prunes the stale entries of one folder's sidecar `root` (its whole JSON
/// document) at `now`. `model` holds the names of the photos the model has in the folder;
/// `present(name)` says whether the folder holds an entry of that name (it is asked only for
/// plain names, at most [`MAX_EXAMINED`] times; answer `true` when unsure). Leaves `root`
/// untouched when `dalica` or `photos` is not an object.
pub fn sweep(root: &mut Map<String, Value>, model: &BTreeSet<&str>, mut present: impl FnMut(&str) -> bool, now: Timestamp) -> Sweep {
    let mut out = Sweep::default();
    let Some(now_stamp) = stamp(now) else { return out };
    // Entries stale for this long or longer go: those marked at or before the cut-off.
    let cutoff = now.as_secs().checked_sub(i64::try_from(PRUNE_AFTER.as_secs()).unwrap_or(i64::MAX)).and_then(DateTime::from_unix_secs);
    let cutoff = cutoff.map(|d| format!("{d}Z"));
    if !matches!(root.get("dalica"), None | Some(Value::Object(_))) {
        return out;
    }
    let keys: Vec<&str> = dalica_keys().collect();
    // The candidates: Dalica's entries the model does not hold, with plain names.
    let candidates: Vec<String> = match root.get("photos") {
        None => Vec::new(),
        Some(Value::Object(photos)) => photos
            .iter()
            .filter(|(name, entry)| {
                !model.contains(name.as_str())
                    && dalica_sidecar::plain_name(name)
                    && entry.as_object().is_some_and(|e| keys.iter().any(|k| e.contains_key(*k)))
            })
            .map(|(name, _)| name.clone())
            .collect(),
        Some(_) => return out,
    };
    let old: Map<String, Value> = match root.get("dalica").and_then(|d| d.get(MISSING_KEY)) {
        Some(Value::Object(m)) => m.clone(),
        _ => Map::new(),
    };
    let mut marks = Map::new();
    let mut prune = Vec::new();
    for (i, name) in candidates.iter().enumerate() {
        if i >= MAX_EXAMINED {
            // Not looked at this time: its mark, if any, stays as it is.
            if let Some(v) = old.get(name) {
                marks.insert(name.clone(), v.clone());
            }
            continue;
        }
        if present(name) {
            continue;
        }
        match old.get(name).and_then(read_stamp) {
            Some(since) if cutoff.as_ref().is_some_and(|c| since <= *c) => prune.push(name.clone()),
            Some(since) => {
                marks.insert(name.clone(), Value::String(since));
            }
            None => {
                marks.insert(name.clone(), Value::String(now_stamp.clone()));
                out.marked.push(name.clone());
            }
        }
    }
    out.cleared = old.keys().filter(|n| !marks.contains_key(*n) && !prune.contains(n)).cloned().collect();
    if marks == old && prune.is_empty() {
        return Sweep::default();
    }
    if let Some(Value::Object(photos)) = root.get_mut("photos") {
        for name in &prune {
            photos.remove(name);
        }
    }
    out.pruned = prune;
    match root.get_mut("dalica") {
        Some(Value::Object(dalica)) => {
            if marks.is_empty() {
                dalica.remove(MISSING_KEY);
            } else {
                dalica.insert(MISSING_KEY.into(), Value::Object(marks));
            }
        }
        _ if marks.is_empty() => {}
        _ => {
            let mut dalica = Map::new();
            dalica.insert(MISSING_KEY.into(), Value::Object(marks));
            root.insert("dalica".into(), Value::Object(dalica));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const DAY: i64 = 24 * 60 * 60 * 1000;
    const T0: Timestamp = Timestamp::from_millis(1_790_000_000_000);

    fn at(days: i64) -> Timestamp {
        Timestamp::from_millis(T0.as_millis() + days * DAY)
    }

    fn doc(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap()
    }

    #[test]
    fn stamps_round_trip_and_sort_by_time() {
        let s = stamp(T0).unwrap();
        assert_eq!(s, "2026-09-21T14:13:20Z");
        assert_eq!(read_stamp(&Value::String(s.clone())), Some(s.clone()));
        assert!(stamp(at(-1)).unwrap() < s && s < stamp(at(1)).unwrap());
        for bad in [json!("2026-09-21"), json!("2026-09-21T14:13:20"), json!(17), json!("x")] {
            assert_eq!(read_stamp(&bad), None, "{bad}");
        }
    }

    #[test]
    fn a_gone_entry_is_marked_kept_for_thirty_days_then_pruned() {
        let mut root = doc(json!({ "dalica": { "version": 1 }, "photos": { "a.jpg": { "star": true, "x": 1 }, "b.jpg": { "star": true } } }));
        let model = BTreeSet::from(["b.jpg"]);
        let s = sweep(&mut root, &model, |_| false, T0);
        assert_eq!(s.marked, ["a.jpg"]);
        assert_eq!(root["dalica"]["missing"], json!({ "a.jpg": "2026-09-21T14:13:20Z" }));
        // Nothing changes until thirty days have passed.
        let before = root.clone();
        assert!(!sweep(&mut root, &model, |_| false, at(29)).changed());
        assert_eq!(root, before);
        let s = sweep(&mut root, &model, |_| false, at(30));
        assert_eq!(s.pruned, ["a.jpg"]);
        assert_eq!(Value::Object(root), json!({ "dalica": { "version": 1 }, "photos": { "b.jpg": { "star": true } } }));
    }

    /// An entry holding only album memberships (M9) is Dalica's data like a star: marked when its
    /// file is gone, pruned thirty days later.
    #[test]
    fn an_entry_holding_only_albums_is_marked_and_pruned() {
        let mut root =
            doc(json!({ "dalica": { "version": 1 }, "photos": { "a.jpg": { "albums": { "a91c0e": "a1" } }, "theirs.jpg": { "album": "x" } } }));
        let s = sweep(&mut root, &BTreeSet::new(), |_| false, T0);
        assert_eq!(s.marked, ["a.jpg"], "another program's key is not Dalica's");
        let s = sweep(&mut root, &BTreeSet::new(), |_| false, at(30));
        assert_eq!(s.pruned, ["a.jpg"]);
        assert_eq!(Value::Object(root), json!({ "dalica": { "version": 1 }, "photos": { "theirs.jpg": { "album": "x" } } }));
    }

    #[test]
    fn a_returning_file_or_photo_clears_the_mark_and_nothing_else_is_touched() {
        let start = json!({
            "dalica": { "version": 1, "missing": { "a.jpg": "2026-01-01T00:00:00Z", "gone.jpg": "2026-01-01T00:00:00Z" } },
            "photos": {
                "a.jpg": { "star": true },
                "small.png": { "star": true },
                "theirs.jpg": { "rating": 5 },
                "../escape.jpg": { "star": true },
                "odd.jpg": 7
            }
        });
        let mut root = doc(start.clone());
        // `a.jpg` is back in the model; `small.png` is on disk though the model skips it.
        let model = BTreeSet::from(["a.jpg"]);
        let s = sweep(&mut root, &model, |n| n == "small.png", at(100));
        assert_eq!((s.marked.len(), s.pruned.len()), (0, 0));
        assert_eq!(s.cleared, ["a.jpg", "gone.jpg"]);
        let mut want = start;
        want["dalica"] = json!({ "version": 1 });
        assert_eq!(Value::Object(root), want, "only the marks went");
    }

    #[test]
    fn a_bad_mark_restarts_the_wait_and_hostile_shapes_are_left_alone() {
        let mut root = doc(json!({ "dalica": { "missing": { "a.jpg": 12 } }, "photos": { "a.jpg": { "star": true } } }));
        let s = sweep(&mut root, &BTreeSet::new(), |_| false, at(400));
        assert_eq!((s.marked.clone(), s.pruned.clone()), (vec!["a.jpg".to_string()], vec![]));
        assert_eq!(root["dalica"]["missing"]["a.jpg"], json!(stamp(at(400)).unwrap()));
        for shape in [json!({ "dalica": 3, "photos": { "a.jpg": { "star": true } } }), json!({ "photos": [1] })] {
            let mut root = doc(shape.clone());
            assert!(!sweep(&mut root, &BTreeSet::new(), |_| false, at(400)).changed());
            assert_eq!(Value::Object(root), shape);
        }
        // No `dalica` object yet: one is made for the mark.
        let mut root = doc(json!({ "photos": { "a.jpg": { "star": true } } }));
        assert_eq!(sweep(&mut root, &BTreeSet::new(), |_| false, T0).marked, ["a.jpg"]);
        assert!(root["dalica"]["missing"]["a.jpg"].is_string());
    }

    #[test]
    fn at_most_max_examined_names_are_looked_up() {
        let photos: Map<String, Value> = (0..MAX_EXAMINED + 10).map(|i| (format!("{i:06}.jpg"), json!({ "star": true }))).collect();
        let mut root = Map::new();
        root.insert("photos".into(), Value::Object(photos));
        let mut asked = 0;
        let s = sweep(
            &mut root,
            &BTreeSet::new(),
            |_| {
                asked += 1;
                false
            },
            T0,
        );
        assert_eq!((asked, s.marked.len()), (MAX_EXAMINED, MAX_EXAMINED));
    }
}
