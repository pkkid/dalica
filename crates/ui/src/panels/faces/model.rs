//! What the Unnamed grid shows (`organizing.md` §6.2, `faces.md` §5): the unnamed faces in groups
//! of similar faces, largest first, and with Show Ignored Faces the ignored faces after them.
//!
//! - **The source** is the engine's `faces.list {unnamed: true, ignored}` (read a page of
//!   [`PAGE`] at a time): each face with its `group`, the group of similar unnamed faces the face
//!   index gives it (M13.18). A face without one is a group of its own until the index has looked
//!   at it, so the grid never waits for the scan.
//! - **Groups** ([`grid_of`]) are numbered from 0, largest first; groups of the same size keep the
//!   order of their first face (the photo order `faces.list` gives), so the grid comes back the same
//!   after a restart. Within a group the faces keep `faces.list`'s order.
//! - **Planted groups** ([`plant_groups`]): until the face index groups faces, a test plants the
//!   groups it would give, for the window's thread only. The window itself never plants.
//! - **The names typed** under the groups (`unnamed.names`, by group number) follow their group
//!   when the groups change ([`remap_names`]): a group named or ignored leaves, the others move up,
//!   and a name typed for a group that is gone is dropped.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use dalica_engine::Session;
use serde_json::{Value, json};

use crate::state::FaceRef;

/// The faces one `faces.list` call asks for (the engine's largest page).
pub const PAGE: usize = 10_000;
/// The most faces the grid reads (pages stop there).
pub const MAX_FACES: usize = 1_000_000;

/// One face as `faces.list` gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Listed {
    pub face: FaceRef,
    /// Its group of similar faces, when the face index has one.
    pub group: Option<u64>,
    pub ignored: bool,
}

/// One group of similar unnamed faces.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Group {
    pub faces: Vec<FaceRef>,
}

/// The grid: the groups, largest first, and the ignored faces shown.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Grid {
    pub groups: Vec<Group>,
    /// The ignored faces (only with Show Ignored Faces), in photo order.
    pub ignored: Vec<FaceRef>,
}

impl Grid {
    /// The faces of group `n`.
    pub fn group(&self, n: u32) -> Option<&Group> {
        self.groups.get(usize::try_from(n).ok()?)
    }

    /// Every face shown, in the grid's order: group by group, then the ignored ones.
    pub fn faces(&self) -> impl Iterator<Item = FaceRef> + '_ {
        self.groups.iter().flat_map(|g| g.faces.iter().copied()).chain(self.ignored.iter().copied())
    }

    /// The number of the group holding `face`, if one does.
    pub fn group_of(&self, face: FaceRef) -> Option<u32> {
        let at = self.groups.iter().position(|g| g.faces.contains(&face))?;
        u32::try_from(at).ok()
    }

    /// Whether the grid shows nothing.
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty() && self.ignored.is_empty()
    }
}

/// The grid of `faces` (in `faces.list`'s order): the unnamed ones grouped by their `group` (each
/// without one alone), the groups largest first, ties in the order of their first face; the
/// ignored ones after, as listed.
pub fn grid_of(faces: &[Listed]) -> Grid {
    let mut groups: Vec<Group> = Vec::new();
    let mut by_group: HashMap<u64, usize> = HashMap::new();
    let mut ignored = Vec::new();
    for f in faces {
        if f.ignored {
            ignored.push(f.face);
            continue;
        }
        let at = match f.group {
            Some(g) => *by_group.entry(g).or_insert_with(|| {
                groups.push(Group::default());
                groups.len() - 1
            }),
            None => {
                groups.push(Group::default());
                groups.len() - 1
            }
        };
        if let Some(group) = groups.get_mut(at) {
            group.faces.push(f.face);
        }
    }
    // A stable sort: groups of one size keep the order of their first face.
    groups.sort_by_key(|g| std::cmp::Reverse(g.faces.len()));
    Grid { groups, ignored }
}

/// One face of a `faces.list` reply, if it is one.
fn listed(v: &Value) -> Option<Listed> {
    let face: FaceRef = v.get("id")?.as_str()?.parse().ok()?;
    Some(Listed { face, group: v.get("group").and_then(Value::as_u64), ignored: v.get("ignored").and_then(Value::as_bool).unwrap_or(false) })
}

/// The faces of a `faces.list` reply (anything that is no face left out).
pub fn listed_of(reply: &Value) -> Vec<Listed> {
    reply.get("faces").and_then(Value::as_array).map(|a| a.iter().filter_map(listed).collect()).unwrap_or_default()
}

/// The unnamed faces of `session`'s library (and the ignored ones with `ignored`), as `faces.list`
/// gives them, with the planted groups for faces the index has not grouped.
pub fn list(session: &mut Session, ignored: bool) -> Vec<Listed> {
    let mut out: Vec<Listed> = Vec::new();
    let mut offset = 0usize;
    while offset < MAX_FACES {
        let reply = session.execute("faces.list", &json!({ "unnamed": true, "ignored": ignored, "offset": offset, "limit": PAGE }));
        let Ok(reply) = reply else { break };
        let page = listed_of(&reply);
        let total = reply.get("total").and_then(Value::as_u64).and_then(|t| usize::try_from(t).ok()).unwrap_or(0);
        let got = page.len();
        out.extend(page);
        offset = offset.saturating_add(PAGE);
        if got == 0 || offset >= total {
            break;
        }
    }
    PLANTED.with(|p| {
        let planted = p.borrow();
        if !planted.is_empty() {
            for f in out.iter_mut().filter(|f| f.group.is_none()) {
                f.group = planted.get(&f.face).copied();
            }
        }
    });
    out
}

/// The grid of `session`'s library now ([`list`], [`grid_of`]).
pub fn grid(session: &mut Session, ignored: bool) -> Grid {
    grid_of(&list(session, ignored))
}

/// What a grid was made from: when it is the same, the grid is too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub revision: u64,
    pub index: u64,
    pub ignored: bool,
    pub planted: u64,
}

/// The key of the grid `session` gives now with `ignored`.
pub fn key(session: &Session, ignored: bool) -> Key {
    Key { revision: session.library().revision(), index: session.faces_index().generation(), ignored, planted: PLANTED_AT.with(|p| *p.borrow()) }
}

thread_local! {
    /// Groups planted by a test, by face.
    static PLANTED: RefCell<HashMap<FaceRef, u64>> = RefCell::new(HashMap::new());
    /// Rises with each planting, so a cached grid is made again.
    static PLANTED_AT: RefCell<u64> = const { RefCell::new(0) };
}

/// Plants groups for this thread (tests only, until the face index groups faces, M13.18): each
/// face listed takes its group number when `faces.list` gives it none. An empty list takes the
/// planted groups away.
#[doc(hidden)]
pub fn plant_groups(groups: &[(FaceRef, u64)]) {
    PLANTED.with(|p| *p.borrow_mut() = groups.iter().copied().collect());
    PLANTED_AT.with(|p| {
        let mut at = p.borrow_mut();
        *at = at.wrapping_add(1);
    });
}

/// The names typed under the groups of `old`, moved to the groups of `new` that hold their
/// group's first face (a group named or ignored is gone, and its name with it).
pub fn remap_names(names: &BTreeMap<u32, String>, old: &Grid, new: &Grid) -> BTreeMap<u32, String> {
    let mut out = BTreeMap::new();
    for (n, name) in names {
        let Some(first) = old.group(*n).and_then(|g| g.faces.first()) else { continue };
        if let Some(to) = new.group_of(*first) {
            out.insert(to, name.clone());
        }
    }
    out
}

/// The faces of `faces` the grid still shows, in their order.
pub fn still_shown(faces: &[FaceRef], grid: &Grid) -> Vec<FaceRef> {
    let shown: std::collections::HashSet<FaceRef> = grid.faces().collect();
    faces.iter().copied().filter(|f| shown.contains(f)).collect()
}

#[cfg(test)]
mod tests {
    use dalica_core::PhotoId;

    use super::*;

    fn f(photo: u64, index: u16) -> FaceRef {
        FaceRef { photo: PhotoId::new(photo), index }
    }

    fn l(photo: u64, index: u16, group: Option<u64>) -> Listed {
        Listed { face: f(photo, index), group, ignored: false }
    }

    #[test]
    fn groups_come_largest_first_ties_in_photo_order() {
        let faces = [
            l(1, 0, Some(7)),
            l(1, 1, None),
            l(2, 0, Some(3)),
            l(3, 0, Some(7)),
            l(4, 0, Some(3)),
            l(5, 0, Some(3)),
            l(6, 0, None),
            Listed { face: f(6, 1), group: Some(3), ignored: true },
        ];
        let g = grid_of(&faces);
        let groups: Vec<Vec<FaceRef>> = g.groups.iter().map(|g| g.faces.clone()).collect();
        assert_eq!(groups, vec![vec![f(2, 0), f(4, 0), f(5, 0)], vec![f(1, 0), f(3, 0)], vec![f(1, 1)], vec![f(6, 0)]]);
        assert_eq!(g.ignored, vec![f(6, 1)]);
        assert_eq!(g.group_of(f(3, 0)), Some(1));
        assert_eq!(g.group_of(f(6, 1)), None);
        assert_eq!(g.faces().count(), 8);
        assert!(grid_of(&[]).is_empty());
    }

    #[test]
    fn names_follow_their_group_and_leave_with_it() {
        let old = grid_of(&[l(1, 0, Some(1)), l(2, 0, Some(1)), l(3, 0, Some(2)), l(4, 0, Some(2)), l(5, 0, None)]);
        let names = BTreeMap::from([(0, "Ana".to_string()), (1, "Rui".to_string()), (2, "Eva".to_string()), (9, "x".to_string())]);
        // Group 0 was named: the others move up.
        let new = grid_of(&[l(3, 0, Some(2)), l(4, 0, Some(2)), l(5, 0, None)]);
        assert_eq!(remap_names(&names, &old, &new), BTreeMap::from([(0, "Rui".to_string()), (1, "Eva".to_string())]));
        assert_eq!(still_shown(&[f(1, 0), f(5, 0)], &new), vec![f(5, 0)]);
    }

    #[test]
    fn replies_are_read_leniently() {
        let reply = json!({ "faces": [
            { "id": "12.0", "group": 4, "ignored": false },
            { "id": "12.1", "group": null },
            { "id": "12.256" },
            { "id": 3 },
            { "id": "13.2", "ignored": true },
        ], "total": 5 });
        assert_eq!(listed_of(&reply), vec![l(12, 0, Some(4)), l(12, 1, None), Listed { face: f(13, 2), group: None, ignored: true }]);
        assert_eq!(listed_of(&json!({ "faces": "x" })), Vec::<Listed>::new());
    }
}
