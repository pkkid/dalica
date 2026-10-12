//! The Folder Manager's rules on a temporary tree (`data-model.md` §7, `organizing.md` §2):
//! inheritance and overrides, the scan plans they give (a region, everything), Scan Once frozen
//! after its first scan, and the faces flag.

use std::path::{Path, PathBuf};

use dalica_scan::{Mode, Options, Plan, Rule, Rules, scan_with};
use dalica_scenes::{Scene, jpeg};

fn put(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, jpeg(&Scene::new(1, 16, 16), 80, None).unwrap()).unwrap();
}

fn rule(mode: Mode) -> Rule {
    Rule { mode: Some(mode), ..Rule::default() }
}

/// The tree: `p/{a/1, a/x/2, b/3, b/y/4, b/y/z/5, c/6}`.
fn tree() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("p");
    for f in ["a/1.jpg", "a/x/2.jpg", "b/3.jpg", "b/y/4.jpg", "b/y/z/5.jpg", "c/6.jpg"] {
        put(&p.join(f));
    }
    (dir, p)
}

/// The photos a scan of `plan` lists that the rules index, relative to `p`, with `/`.
fn indexed(p: &Path, rules: &Rules, plan: &Plan) -> Vec<String> {
    let s = scan_with(&plan.roots, &Options { excluded: plan.excluded.clone(), ..Options::default() });
    let mut out: Vec<String> = s
        .folders
        .iter()
        .filter(|f| rules.indexes(&f.path))
        .flat_map(|f| f.photos.iter().map(move |ph| f.path.join(&ph.name)))
        .map(|f| f.strip_prefix(p).unwrap().components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"))
        .collect();
    out.sort();
    out
}

#[test]
fn subfolders_inherit_the_nearest_setting_unless_they_set_their_own() {
    let (_dir, p) = tree();
    let mut r = Rules::new();
    assert_eq!(r.effective(&p.join("a")).mode, Mode::Remove, "below no setting nothing is indexed");
    r.set(p.clone(), rule(Mode::Always));
    r.set(p.join("b"), rule(Mode::Remove));
    r.set(p.join("b/y"), rule(Mode::Once));
    let e = r.effective(&p.join("a/x"));
    assert_eq!((e.mode, e.from.as_deref()), (Mode::Always, Some(p.as_path())));
    assert!(!e.explicit(&p.join("a/x")) && r.effective(&p).explicit(&p));
    assert_eq!(r.effective(&p.join("b")).mode, Mode::Remove);
    assert_eq!(r.effective(&p.join("b/y/z")).mode, Mode::Once, "an override inside a removed folder");
    assert_eq!(r.units(), [p.clone(), p.join("b/y")]);
    // The region of `p` leaves out the folders that set their own mode.
    assert_eq!(indexed(&p, &r, &r.region(&p)), ["a/1.jpg", "a/x/2.jpg", "c/6.jpg"]);
    assert_eq!(indexed(&p, &r, &r.region(&p.join("b/y"))), ["b/y/4.jpg", "b/y/z/5.jpg"]);
    // Everything: both units, walked once, nothing of the removed folder itself.
    let all = r.everything();
    assert_eq!(all.roots, std::slice::from_ref(&p));
    assert_eq!(indexed(&p, &r, &all), ["a/1.jpg", "a/x/2.jpg", "b/y/4.jpg", "b/y/z/5.jpg", "c/6.jpg"]);
    // A removed folder holding no unit is never entered.
    r.set(p.join("c"), rule(Mode::Remove));
    let all = r.everything();
    assert!(all.excluded.contains(&p.join("c")) && !all.excluded.contains(&p.join("b")));
    assert_eq!(indexed(&p, &r, &all), ["a/1.jpg", "a/x/2.jpg", "b/y/4.jpg", "b/y/z/5.jpg"]);
    // Clearing an override gives the folder back to its parent's setting.
    r.set(p.join("b"), Rule::default());
    assert_eq!(r.get(&p.join("b")), None);
    assert_eq!(r.effective(&p.join("b")).mode, Mode::Always);
    assert_eq!(indexed(&p, &r, &r.region(&p)), ["a/1.jpg", "a/x/2.jpg", "b/3.jpg"]);
}

#[test]
fn a_scan_once_unit_is_frozen_after_its_first_scan() {
    let (_dir, p) = tree();
    let mut r = Rules::new();
    r.set(p.clone(), rule(Mode::Always));
    r.set(p.join("b"), rule(Mode::Once));
    assert_eq!(r.verification(), [p.clone(), p.join("b")], "not indexed yet: verified");
    // A cancelled or partial scan of another root marks nothing.
    assert!(!r.mark_indexed(&Plan { roots: vec![p.join("a")], excluded: vec![] }));
    // A finished scan that left `b` out marks nothing either.
    assert!(!r.mark_indexed(&r.region(&p)));
    assert!(r.mark_indexed(&r.region(&p.join("b"))));
    assert!(r.get(&p.join("b")).unwrap().indexed);
    assert_eq!(r.verification(), std::slice::from_ref(&p), "frozen: never rescanned by verification");
    // Which changes need a scan of the region.
    assert!(Rules::needs_scan(Mode::Remove, Mode::Once) && Rules::needs_scan(Mode::Once, Mode::Always));
    assert!(!Rules::needs_scan(Mode::Always, Mode::Once) && !Rules::needs_scan(Mode::Once, Mode::Once));
    assert!(!Rules::needs_scan(Mode::Always, Mode::Remove) && !Rules::needs_scan(Mode::Always, Mode::Always));
}

#[test]
fn the_faces_flag_is_inherited_and_on_by_default() {
    let (_dir, p) = tree();
    let mut r = Rules::new();
    assert!(r.faces(&p.join("a")));
    r.set(p.clone(), Rule { mode: Some(Mode::Always), faces: Some(false), indexed: false });
    r.set(p.join("b/y"), Rule { faces: Some(true), ..Rule::default() });
    assert!(!r.faces(&p.join("a/x")));
    assert!(r.faces(&p.join("b/y/z")));
    // A rule that only sets faces is no unit and does not split a region.
    assert_eq!(r.units(), std::slice::from_ref(&p));
    assert!(r.region(&p).excluded.is_empty());
}
