//! `cargo xtask test-layout`: the shape of the test suites, so it cannot creep back
//! (`docs/development.md` §6; maintainer decision, 2026-10-15). Run by every gate.
//!
//! - **One integration-test binary per crate:** a crate (or app) with integration tests sets
//!   `autotests = false`, declares `[[test]] name = "all"` (`tests/all.rs`) and makes every other
//!   `tests/*.rs` a `mod` of it. Separate `[[test]]` targets only where [`SEPARATE`] allows them.
//! - **No per-milestone test files** (`settings_m10.rs`, `m9_widgets.rs`, `lessons_m9.rs`): tests
//!   go in the file of the feature they test. [`LEGACY`] lists the ones that predate the rule (it
//!   may only shrink); the open task blocks of `docs/roadmap.md` may not plan new ones.
//! - **No hard-coded proptest case counts above [`MAX_LITERAL_CASES`]:** heavier counts go through
//!   the crate's quick/full switch (`DALICA_FULL_TESTS`).
//! - **No fingerprint baselines at the small window size** (`*900x600*`, `*-900.json`): the small
//!   size is checked by asserting that things fit (`architecture.md` §11).
//!
//! The acceptance package (`tests/acceptance`, one file per milestone by design) is not checked.

use std::path::Path;

use crate::tasks;

/// Separate test targets allowed beside `all`: (package directory, test name, why).
pub const SEPARATE: &[(&str, &str, &str)] = &[
    ("crates/engine", "fileops_faults", "re-execs its own binary per case and installs a process-wide logger"),
    ("crates/engine", "undo_faults", "re-execs its own binary per case and installs a process-wide logger"),
    ("crates/image", "heif", "half of it needs the `heif` feature"),
    ("crates/image", "corpus", "needs the `corpus` feature and the fetched corpus"),
    ("crates/meta", "corpus", "needs the `corpus` feature and the fetched corpus"),
    ("crates/faces", "corpus", "needs the `corpus` feature and the fetched corpus"),
    ("crates/mcp", "workflows", "cargo xtask stats runs it alone (`--test workflows`) and reads its results"),
];

/// Per-milestone test files that predate the rule. Never add to this list; remove a file from it
/// when its tests move to their feature files.
pub const LEGACY: &[&str] = &[
    "crates/engine/tests/lessons_m9.rs",
    "crates/library/tests/m6_fields.rs",
    "crates/library/tests/m8_fields.rs",
    "crates/library/tests/m9_fields.rs",
    "crates/store/tests/m6_fields.rs",
    "crates/store/tests/m8_fields.rs",
    "crates/store/tests/m9_fields.rs",
    "crates/ui-kit/tests/m9_widgets.rs",
];

/// The largest proptest case count a test may write as a literal.
pub const MAX_LITERAL_CASES: u64 = 32;

/// Whether a test file's stem names a milestone (`m9_widgets`, `settings_m10`, `lessons_m9`).
pub fn is_milestone_name(stem: &str) -> bool {
    if stem.starts_with("lessons_") {
        return true;
    }
    stem.split('_').any(|part| part.len() > 1 && part.starts_with('m') && part[1..].chars().all(|c| c.is_ascii_digit()))
}

/// The `[[test]]` targets of a manifest: (name, path), the path defaulting to `tests/<name>.rs`.
pub fn declared_tests(manifest: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    let mut in_test = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_test = line == "[[test]]";
            if in_test {
                out.push((String::new(), None));
            }
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let value = value.trim().trim_matches('"').to_string();
        if let (true, Some(last)) = (in_test, out.last_mut()) {
            match key.trim() {
                "name" => last.0 = value,
                "path" => last.1 = Some(value),
                _ => {}
            }
        }
    }
    out.into_iter().map(|(name, path)| (name.clone(), path.unwrap_or_else(|| format!("tests/{name}.rs")))).collect()
}

/// Whether `[package]` sets `autotests = false`.
pub fn autotests_off(manifest: &str) -> bool {
    let mut in_package = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if in_package && line.replace(' ', "") == "autotests=false" {
            return true;
        }
    }
    false
}

/// The modules `tests/all.rs` declares (`mod name;`, with or without `pub`).
pub fn declared_mods(all_rs: &str) -> Vec<String> {
    all_rs
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("pub ").unwrap_or(l).strip_prefix("mod ")?.strip_suffix(';').map(|m| m.trim().to_string()))
        .collect()
}

/// One package's problems: `dir` is its directory relative to the root, `files` the stems of its
/// top-level `tests/*.rs`, `all_rs` the text of `tests/all.rs` if it exists.
pub fn check_package(dir: &str, manifest: &str, files: &[String], all_rs: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    if files.is_empty() {
        return out;
    }
    if !autotests_off(manifest) {
        out.push(format!("{dir}/Cargo.toml: integration tests need `autotests = false` and one `[[test]] name = \"all\"` (one binary per crate)"));
    }
    let declared = declared_tests(manifest);
    if !declared.iter().any(|(n, p)| n == "all" && p == "tests/all.rs") {
        out.push(format!("{dir}/Cargo.toml: no `[[test]] name = \"all\", path = \"tests/all.rs\"`"));
    }
    for (name, _) in declared.iter().filter(|(n, _)| n != "all") {
        if !SEPARATE.iter().any(|(d, n, _)| *d == dir && n == name) {
            out.push(format!(
                "{dir}/Cargo.toml: a separate test binary `{name}`; make it a `mod` of tests/all.rs (or, if it must be its own process, add it to SEPARATE in xtask/src/test_layout.rs with the reason)"
            ));
        }
    }
    let mods = all_rs.map(declared_mods).unwrap_or_default();
    for stem in files.iter().filter(|s| *s != "all" && *s != "common") {
        let separate = declared.iter().any(|(n, p)| n != "all" && *p == format!("tests/{stem}.rs"));
        if !separate && !mods.contains(stem) {
            out.push(format!("{dir}/tests/{stem}.rs: not a module of tests/all.rs (add `mod {stem};` there)"));
        }
        let path = format!("{dir}/tests/{stem}.rs");
        if is_milestone_name(stem) && !LEGACY.contains(&path.as_str()) {
            out.push(format!("{path}: a per-milestone test file; put its tests in the file of the feature they test"));
        }
    }
    out
}

/// Literal proptest case counts above [`MAX_LITERAL_CASES`] in `text` (`with_cases(4096)`,
/// `cases: 512`): (line, count).
pub fn literal_cases(text: &str) -> Vec<(usize, u64)> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        for key in ["with_cases(", "cases:"] {
            let mut rest = line;
            while let Some(at) = rest.find(key) {
                rest = &rest[at + key.len()..];
                let digits: String = rest.trim_start().chars().take_while(|c| c.is_ascii_digit() || *c == '_').filter(|c| *c != '_').collect();
                if let Ok(n) = digits.parse::<u64>()
                    && n > MAX_LITERAL_CASES
                {
                    out.push((i + 1, n));
                }
            }
        }
    }
    out
}

/// Whether a file name is a fingerprint baseline at the small window size.
pub fn is_small_size_baseline(name: &str) -> bool {
    name.ends_with(".json") && (name.contains("900x600") || name.ends_with("-900.json"))
}

fn rs_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if !["target", "fuzz"].contains(&e.file_name().to_string_lossy().as_ref()) {
                rs_files(&p, out);
            }
        } else {
            out.push(p);
        }
    }
}

pub fn run(root: &Path) -> Result<(), String> {
    let mut problems = Vec::new();
    let mut packages = 0;
    for group in ["crates", "apps"] {
        let Ok(entries) = std::fs::read_dir(root.join(group)) else { continue };
        let mut dirs: Vec<_> = entries.flatten().map(|e| e.path()).filter(|p| p.join("Cargo.toml").is_file()).collect();
        dirs.sort();
        for path in dirs {
            let rel = format!("{group}/{}", path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
            let manifest = std::fs::read_to_string(path.join("Cargo.toml")).map_err(|e| format!("{rel}/Cargo.toml: {e}"))?;
            let mut stems: Vec<String> = std::fs::read_dir(path.join("tests"))
                .map(|rd| {
                    rd.flatten()
                        .map(|e| e.path())
                        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "rs"))
                        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
                        .collect()
                })
                .unwrap_or_default();
            stems.sort();
            let all_rs = std::fs::read_to_string(path.join("tests").join("all.rs")).ok();
            problems.extend(check_package(&rel, &manifest, &stems, all_rs.as_deref()));
            packages += 1;
            let mut files = Vec::new();
            rs_files(&path, &mut files);
            for f in files {
                let name = f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let shown = f.strip_prefix(root).unwrap_or(&f).display().to_string().replace('\\', "/");
                if is_small_size_baseline(&name) {
                    problems.push(format!("{shown}: a fingerprint at the small window size; check that it fits instead"));
                }
                if name.ends_with(".rs")
                    && let Ok(text) = std::fs::read_to_string(&f)
                {
                    for (line, n) in literal_cases(&text) {
                        problems.push(format!(
                            "{shown}:{line}: {n} proptest cases as a literal; use the crate's quick/full switch (at most {MAX_LITERAL_CASES} quick)"
                        ));
                    }
                }
            }
        }
    }
    // Open task blocks may not plan per-milestone test files.
    if let Ok(roadmap) = tasks::load_roadmap(root) {
        for t in roadmap.tasks.iter().filter(|t| !t.done) {
            for f in &t.files {
                let in_tests = (f.starts_with("crates/") || f.starts_with("apps/")) && f.contains("/tests/");
                let stem = f.rsplit('/').next().and_then(|n| n.strip_suffix(".rs")).unwrap_or("");
                if in_tests && is_milestone_name(stem) && !LEGACY.contains(&f.as_str()) {
                    problems.push(format!("docs/roadmap.md: {} plans the per-milestone test file {f}; name the feature file instead", t.id));
                }
            }
        }
    }
    for p in &problems {
        eprintln!("test-layout: {p}");
    }
    if problems.is_empty() {
        println!(
            "test-layout: ok ({packages} packages: one test binary each, no per-milestone files, no literal case counts over {MAX_LITERAL_CASES}, no small-size baselines)"
        );
        Ok(())
    } else {
        Err(format!("test-layout: {} problem(s)", problems.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn milestone_names() {
        for yes in ["settings_m10", "m9_widgets", "view_state_m6", "lessons_m9", "lessons_learned", "m6_fields"] {
            assert!(is_milestone_name(yes), "{yes}");
        }
        for no in ["settings", "m", "mcp", "move_copy", "map", "m_x", "xmp_merge", "jpeg_m"] {
            assert!(!is_milestone_name(no), "{no}");
        }
    }

    #[test]
    fn one_binary_per_crate() {
        let good = "[package]\nname = \"x\"\nautotests = false\n\n[[test]]\nname = \"all\"\npath = \"tests/all.rs\"\n";
        let files = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            check_package("crates/x", good, &files(&["all", "common", "a", "b"]), Some("mod common;\nmod a;\npub mod b;\n")),
            Vec::<String>::new()
        );
        // A file left out of all.rs, autotests on, a separate target not allowed.
        let p = check_package("crates/x", good, &files(&["all", "a", "b"]), Some("mod a;\n"));
        assert!(p.len() == 1 && p[0].contains("tests/b.rs: not a module"), "{p:?}");
        let p = check_package("crates/x", "[package]\nname = \"x\"\n", &files(&["a"]), None);
        assert!(p.iter().any(|m| m.contains("autotests")) && p.iter().any(|m| m.contains("no `[[test]] name = \"all\"")), "{p:?}");
        let extra = format!("{good}\n[[test]]\nname = \"slow\"\n");
        let p = check_package("crates/x", &extra, &files(&["all", "slow"]), Some(""));
        assert!(p.len() == 1 && p[0].contains("separate test binary `slow`"), "{p:?}");
        // An allowed separate target needs no mod line.
        let heif = format!("{good}\n[[test]]\nname = \"heif\"\nrequired-features = [\"heif\"]\n");
        assert_eq!(check_package("crates/image", &heif, &files(&["all", "heif"]), Some("")), Vec::<String>::new());
        // A new per-milestone file fails; a legacy one does not.
        let p = check_package("crates/x", good, &files(&["all", "settings_m10"]), Some("mod settings_m10;"));
        assert!(p.len() == 1 && p[0].contains("per-milestone"), "{p:?}");
        assert_eq!(check_package("crates/ui-kit", good, &files(&["all", "m9_widgets"]), Some("mod m9_widgets;")), Vec::<String>::new());
        // No tests, nothing to check.
        assert_eq!(check_package("crates/x", "[package]\n", &[], None), Vec::<String>::new());
    }

    #[test]
    fn literal_case_counts() {
        assert_eq!(literal_cases("ProptestConfig::with_cases(4096)\nwith_cases(32)\nwith_cases(cases(4096))"), [(1, 4096)]);
        assert_eq!(literal_cases("ProptestConfig { cases: 2_048, ..Default::default() }"), [(1, 2048)]);
        assert_eq!(literal_cases("let cases: u32 = 9; cases: n"), Vec::<(usize, u64)>::new());
    }

    #[test]
    fn small_size_baselines() {
        assert!(is_small_size_baseline("dialog-export-900x600.json") && is_small_size_baseline("region-tree-900.json"));
        assert!(!is_small_size_baseline("dialog-export-1400x900.json") && !is_small_size_baseline("pack-900.rs"));
    }

    #[test]
    fn the_repository_passes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        assert_eq!(run(&root), Ok(()));
    }
}
