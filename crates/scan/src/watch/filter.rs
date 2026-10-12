//! Which paths a watcher cares about: inside a watched root, outside the excluded subtrees, and
//! nothing between the root and the path left out by the skip rules.

use std::path::{Path, PathBuf};

use crate::skip::SkipRules;

#[derive(Clone, Debug)]
pub(crate) struct Filter {
    pub(crate) roots: Vec<PathBuf>,
    pub(crate) skip: SkipRules,
    pub(crate) excluded: Vec<PathBuf>,
}

impl Filter {
    /// The watched root holding `path` (the deepest, when roots nest).
    pub(crate) fn root_of(&self, path: &Path) -> Option<&Path> {
        self.roots.iter().filter(|r| path.starts_with(r)).max_by_key(|r| r.components().count()).map(PathBuf::as_path)
    }

    /// The root of `path` when changes at `path` concern the library. `is_dir` says what `path`
    /// is (false when it is gone): folder-only skip rules apply to it only then.
    pub(crate) fn accepts(&self, path: &Path, is_dir: bool) -> Option<&Path> {
        let root = self.root_of(path)?;
        if self.excluded.iter().any(|x| path.starts_with(x)) {
            return None;
        }
        if path == root {
            return Some(root);
        }
        if self.skip.check_path(path, is_dir).is_some() {
            return None;
        }
        // Every folder between the root and `path`.
        let mut at = path.parent();
        while let Some(dir) = at {
            if dir == root || !dir.starts_with(root) {
                break;
            }
            if self.skip.check_path(dir, true).is_some() {
                return None;
            }
            at = dir.parent();
        }
        Some(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skip::Platform;

    #[test]
    fn skips_what_scans_skip_below_the_root_only() {
        let base = PathBuf::from("/top");
        let root = base.join("tmp").join("Pictures");
        let f = Filter {
            roots: vec![root.clone()],
            skip: SkipRules::for_platform(Platform::Unix, &base, &base.join("home")),
            excluded: vec![root.join("no")],
        };
        // `tmp` at the top of the disk is skipped by scans, but the user chose a root inside it.
        assert_eq!(f.accepts(&root.join("a/1.jpg"), false), Some(root.as_path()));
        assert_eq!(f.accepts(&root, true), Some(root.as_path()));
        assert_eq!(f.accepts(&root.join(".git/x"), false), None);
        assert_eq!(f.accepts(&root.join("a/.dalica.json"), false), None);
        assert_eq!(f.accepts(&root.join("a/@eaDir/t.jpg"), false), None);
        assert_eq!(f.accepts(&root.join("a/node_modules"), true), None);
        assert_eq!(f.accepts(&root.join("no/1.jpg"), false), None);
        assert_eq!(f.accepts(&base.join("elsewhere.jpg"), false), None);
    }
}
