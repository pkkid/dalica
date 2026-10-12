//! Watched-folder rules (`data-model.md` §7, `organizing.md` §2): a mode per folder (Scan Always,
//! Scan Once, Remove) and the "Find faces in this folder" flag, each inherited by subfolders that do
//! not set their own. Pure: nothing here touches the disk, so the owner thread may ask it anything.
//!
//! - A folder's **effective mode** is the mode of its nearest ancestor (itself included) that sets
//!   one. A folder below no such ancestor is not indexed, the same as Remove.
//! - Every folder that sets Scan Always or Scan Once is a **unit**: it owns its subtree minus the
//!   subtrees of the folders inside it that set a mode of their own. A library folder belongs to
//!   the unit of its effective mode, or to none (it is not indexed).
//! - A **region** is what one scan of a path walks: the path's subtree minus every folder inside
//!   it that sets a mode ([`Rules::region`]). The folders inside get scans of their own.
//! - A Scan Once unit is indexed once and then frozen: [`Rule::indexed`] says its first scan
//!   finished, and [`Rules::verification`] leaves it out from then on.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// The longest folder path a rule takes, in bytes.
pub const MAX_PATH: usize = 4096;
/// The most folders that may set a rule.
pub const MAX_RULES: usize = 10_000;

/// What Dalica does with a folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Mode {
    /// Indexed and watched: external changes are picked up.
    Always,
    /// Indexed once, then frozen (removable or slow media).
    Once,
    /// Not indexed; nothing on disk changes.
    Remove,
}

impl Mode {
    /// The stable id `folders.json` and the control API use.
    pub const fn id(self) -> &'static str {
        match self {
            Mode::Always => "always",
            Mode::Once => "once",
            Mode::Remove => "remove",
        }
    }

    /// The mode with this [`Mode::id`].
    pub fn from_id(id: &str) -> Option<Mode> {
        [Mode::Always, Mode::Once, Mode::Remove].into_iter().find(|m| m.id() == id)
    }

    /// Scan Always or Scan Once.
    pub const fn indexes(self) -> bool {
        !matches!(self, Mode::Remove)
    }
}

/// What one folder sets. A rule that sets nothing is no rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rule {
    /// The folder's own mode; `None` inherits.
    pub mode: Option<Mode>,
    /// "Find faces in this folder"; `None` inherits (on at the top).
    pub faces: Option<bool>,
    /// A Scan Once unit whose first scan finished: it is frozen.
    pub indexed: bool,
}

impl Rule {
    /// Sets neither a mode nor the faces flag.
    pub fn is_empty(&self) -> bool {
        self.mode.is_none() && self.faces.is_none()
    }
}

/// A folder's effective mode and where it comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Effective {
    pub mode: Mode,
    /// The folder that sets it (the folder itself when it sets one); `None` when no ancestor sets
    /// one (not indexed).
    pub from: Option<PathBuf>,
}

impl Effective {
    /// The folder sets its own mode.
    pub fn explicit(&self, folder: &Path) -> bool {
        self.from.as_deref() == Some(folder)
    }
}

/// What one scan walks: its roots, and the subtrees it must not enter.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub roots: Vec<PathBuf>,
    pub excluded: Vec<PathBuf>,
}

/// The rules of a library, by folder path.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rules {
    rules: BTreeMap<PathBuf, Rule>,
}

impl Rules {
    /// No rules: nothing is indexed.
    pub fn new() -> Rules {
        Rules::default()
    }

    /// The rule `folder` sets, if any.
    pub fn get(&self, folder: &Path) -> Option<&Rule> {
        self.rules.get(folder)
    }

    /// Sets `folder`'s rule (an empty one removes it) and returns the one it replaced.
    pub fn set(&mut self, folder: PathBuf, rule: Rule) -> Option<Rule> {
        if rule.is_empty() { self.rules.remove(&folder) } else { self.rules.insert(folder, rule) }
    }

    /// Every rule, by path.
    pub fn iter(&self) -> impl Iterator<Item = (&Path, &Rule)> {
        self.rules.iter().map(|(p, r)| (p.as_path(), r))
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The nearest rule at or above `folder` that `pick` accepts, with its folder.
    fn nearest<'a>(&'a self, folder: &'a Path, pick: impl Fn(&Rule) -> bool) -> Option<(&'a Path, &'a Rule)> {
        folder.ancestors().find_map(|a| self.rules.get(a).filter(|r| pick(r)).map(|r| (a, r)))
    }

    /// `folder`'s effective mode: the mode of its nearest ancestor (itself included) that sets
    /// one; Remove when none does.
    pub fn effective(&self, folder: &Path) -> Effective {
        match self.nearest(folder, |r| r.mode.is_some()) {
            Some((at, r)) => Effective { mode: r.mode.unwrap_or(Mode::Remove), from: Some(at.to_path_buf()) },
            None => Effective { mode: Mode::Remove, from: None },
        }
    }

    /// Whether `folder` is indexed (its effective mode is Scan Always or Scan Once).
    pub fn indexes(&self, folder: &Path) -> bool {
        self.effective(folder).mode.indexes()
    }

    /// Whether faces are found in `folder`: the nearest flag at or above it; on when none is set.
    pub fn faces(&self, folder: &Path) -> bool {
        self.nearest(folder, |r| r.faces.is_some()).and_then(|(_, r)| r.faces).unwrap_or(true)
    }

    /// The folders that set a mode of `mode`, by path.
    pub fn with_mode(&self, mode: Mode) -> Vec<PathBuf> {
        self.rules.iter().filter(|(_, r)| r.mode == Some(mode)).map(|(p, _)| p.clone()).collect()
    }

    /// The units: the folders that set Scan Always or Scan Once, by path.
    pub fn units(&self) -> Vec<PathBuf> {
        self.rules.iter().filter(|(_, r)| r.mode.is_some_and(Mode::indexes)).map(|(p, _)| p.clone()).collect()
    }

    /// The folders strictly inside `folder` that set a mode.
    pub fn inside(&self, folder: &Path) -> Vec<PathBuf> {
        self.rules.iter().filter(|(p, r)| r.mode.is_some() && p.as_path() != folder && p.starts_with(folder)).map(|(p, _)| p.clone()).collect()
    }

    /// The scan of `folder`'s region: the folder, without the folders inside it that set a mode
    /// (they are units of their own, or removed).
    pub fn region(&self, folder: &Path) -> Plan {
        Plan { roots: vec![folder.to_path_buf()], excluded: self.inside(folder) }
    }

    /// The scan that indexes everything (a rebuild): the units below no other unit as roots, and
    /// the removed folders that hold no unit as excluded. A removed folder that holds a unit is
    /// walked for the unit's sake: the caller drops what it finds there ([`Rules::indexes`]).
    pub fn everything(&self) -> Plan {
        let units = self.units();
        let roots = units.iter().filter(|u| !units.iter().any(|o| o != *u && u.starts_with(o))).cloned().collect();
        let excluded = self.with_mode(Mode::Remove).into_iter().filter(|r| !units.iter().any(|u| u.starts_with(r))).collect();
        Plan { roots, excluded }
    }

    /// The units a verification pass rescans: every Scan Always unit, and the Scan Once units whose
    /// first scan has not finished. A frozen Scan Once unit is never among them.
    pub fn verification(&self) -> Vec<PathBuf> {
        self.rules
            .iter()
            .filter(|(_, r)| match r.mode {
                Some(Mode::Always) => true,
                Some(Mode::Once) => !r.indexed,
                _ => false,
            })
            .map(|(p, _)| p.clone())
            .collect()
    }

    /// Whether the region of `folder` needs a scan after its effective mode went from `before` to
    /// `after`: it became watched (it may have changed while it was not), or it is indexed for the
    /// first time.
    pub fn needs_scan(before: Mode, after: Mode) -> bool {
        match after {
            Mode::Always => before != Mode::Always,
            Mode::Once => before == Mode::Remove,
            Mode::Remove => false,
        }
    }

    /// Marks the Scan Once units inside the roots of a finished scan (and not under its excluded
    /// subtrees) as indexed. Returns whether any changed.
    pub fn mark_indexed(&mut self, plan: &Plan) -> bool {
        let mut changed = false;
        for (path, rule) in &mut self.rules {
            let covered = plan.roots.iter().any(|r| path.starts_with(r)) && !plan.excluded.iter().any(|x| path.starts_with(x));
            if rule.mode == Some(Mode::Once) && !rule.indexed && covered {
                rule.indexed = true;
                changed = true;
            }
        }
        changed
    }
}

/// `path` as a rule's folder: absolute, without `.` or `..` components, at most [`MAX_PATH`]
/// bytes; trailing separators dropped. `Err` says why not.
pub fn folder_path(path: &Path) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err("the folder path is empty".into());
    }
    if path.as_os_str().len() > MAX_PATH {
        return Err(format!("the folder path is longer than {MAX_PATH} bytes"));
    }
    if !path.is_absolute() {
        return Err(format!("{} is not an absolute folder path", path.display()));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
        return Err(format!("{} has `.` or `..` in it", path.display()));
    }
    Ok(path.components().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn modes_round_trip_their_ids() {
        for m in [Mode::Always, Mode::Once, Mode::Remove] {
            assert_eq!(Mode::from_id(m.id()), Some(m));
        }
        assert_eq!(Mode::from_id("never"), None);
    }

    #[cfg(unix)]
    #[test]
    fn folder_paths_are_normalised_or_refused() {
        assert_eq!(folder_path(Path::new("/a/b/")), Ok(p("/a/b")));
        assert!(folder_path(Path::new("a/b")).is_err());
        assert!(folder_path(Path::new("/a/../b")).is_err());
        // A `.` inside a path is no component of its own.
        assert_eq!(folder_path(Path::new("/a/./b")), Ok(p("/a/b")));
        assert!(folder_path(Path::new("")).is_err());
        assert!(folder_path(&PathBuf::from(format!("/{}", "x".repeat(MAX_PATH)))).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn the_rebuild_plan_walks_each_unit_once() {
        let mut r = Rules::new();
        r.set(p("/h"), Rule { mode: Some(Mode::Always), ..Rule::default() });
        r.set(p("/h/big"), Rule { mode: Some(Mode::Remove), ..Rule::default() });
        r.set(p("/h/big/keep"), Rule { mode: Some(Mode::Once), ..Rule::default() });
        r.set(p("/h/junk"), Rule { mode: Some(Mode::Remove), ..Rule::default() });
        r.set(p("/h/a"), Rule { mode: Some(Mode::Once), ..Rule::default() });
        let plan = r.everything();
        assert_eq!(plan.roots, [p("/h")], "units inside it are walked by its walk");
        assert_eq!(plan.excluded, [p("/h/junk")]);
        assert_eq!(r.region(Path::new("/h")).excluded, [p("/h/a"), p("/h/big"), p("/h/big/keep"), p("/h/junk")]);
        // A unit inside a removed folder at the top is a root of its own.
        let mut r = Rules::new();
        r.set(p("/big"), Rule { mode: Some(Mode::Remove), ..Rule::default() });
        r.set(p("/big/keep"), Rule { mode: Some(Mode::Once), ..Rule::default() });
        assert_eq!(r.everything(), Plan { roots: vec![p("/big/keep")], excluded: vec![] });
    }
}
