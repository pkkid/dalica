//! M0 acceptance: every gate bites. The tree is copied to `<target>/accept-m0/tree` (file times kept,
//! so warm runs rebuild almost nothing; one shared target dir for all runs), then one violation at a
//! time is planted, the matching gate must fail, the violation is removed, and the gate must pass
//! again.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use dalica_acceptance::workspace_root;

fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| workspace_root().join("target"))
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").current_dir(dir).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Copies every tracked (and untracked, not ignored) file of the workspace into a fresh git repo.
fn copy_tree(dest: &Path) {
    let root = workspace_root();
    if dest.exists() {
        std::fs::remove_dir_all(dest).unwrap();
    }
    std::fs::create_dir_all(dest).unwrap();
    for file in git(&root, &["ls-files", "-z", "--cached", "--others", "--exclude-standard"]).split('\0').filter(|f| !f.is_empty()) {
        let src = root.join(file);
        if !src.is_file() {
            continue;
        }
        let to = dest.join(file);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(&src, &to).unwrap();
        let modified = std::fs::metadata(&src).unwrap().modified().unwrap();
        std::fs::File::options().write(true).open(&to).unwrap().set_modified(modified).unwrap();
    }
    git(dest, &["-c", "init.defaultBranch=main", "init", "-q"]);
}

/// Runs a gate in the copy; true if it passed. Its output is returned for the failure message.
fn gate(tree: &Path, target: &Path, args: &[&str]) -> (bool, String) {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = Command::new(cargo).current_dir(tree).env("CARGO_TARGET_DIR", target).args(args).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

struct Plant {
    name: &'static str,
    gate: &'static [&'static str],
    /// What the failing gate's output must mention.
    expect: &'static str,
    apply: fn(&Path),
}

fn append(path: &Path, text: &str) {
    let mut s = std::fs::read_to_string(path).unwrap();
    s.push_str(text);
    std::fs::write(path, s).unwrap();
}

/// Adds a line right under `[dependencies]` of a manifest.
fn add_dependency(manifest: &Path, line: &str) {
    let s = std::fs::read_to_string(manifest).unwrap();
    assert!(s.contains("\n[dependencies]\n"), "{} has no [dependencies] table", manifest.display());
    std::fs::write(manifest, s.replacen("\n[dependencies]\n", &format!("\n[dependencies]\n{line}\n"), 1)).unwrap();
}

const PLANTS: &[Plant] = &[
    Plant {
        // ui-kit depends on core only, so this can never be a cargo cycle (which cargo would report
        // before the layers check runs).
        name: "a layer violation (library -> ui-kit)",
        gate: &["xtask", "layers"],
        expect: "dalica-library (L2) -> dalica-ui-kit (L5)",
        apply: |t| add_dependency(&t.join("crates/library/Cargo.toml"), "dalica-ui-kit.workspace = true"),
    },
    Plant {
        name: "unwrap() in non-test code",
        gate: &["clippy", "-p", "dalica-core", "--all-targets", "--", "-D", "warnings"],
        expect: "unwrap_used",
        apply: |t| append(&t.join("crates/core/src/lib.rs"), "\npub fn planted() -> u8 {\n    \"1\".parse().unwrap()\n}\n"),
    },
    Plant {
        name: "an unlisted PNG",
        gate: &["xtask", "assets"],
        expect: "docs/planted.png: asset without an ATTRIBUTION.md row",
        apply: |t| std::fs::write(t.join("docs/planted.png"), b"\x89PNG\r\n\x1a\n").unwrap(),
    },
    Plant {
        name: "the reference product's name in a file",
        gate: &["xtask", "names"],
        expect: "docs/planted.md:",
        // AGENTS.md is the one file allowed to name it; the same text anywhere else must fail.
        apply: |t| {
            std::fs::copy(t.join("AGENTS.md"), t.join("docs/planted.md")).unwrap();
        },
    },
    Plant {
        name: "a path dependency named evil-sys",
        gate: &["xtask", "deps"],
        expect: "evil-sys 0.1.0: `*-sys` crate not in the allow-list",
        apply: |t| {
            let dir = t.join("vendor/evil-sys/src");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("lib.rs"), "").unwrap();
            std::fs::write(
                t.join("vendor/evil-sys/Cargo.toml"),
                "[package]\nname = \"evil-sys\"\nversion = \"0.1.0\"\nedition = \"2024\"\nlicense = \"MIT\"\n",
            )
            .unwrap();
            add_dependency(&t.join("apps/dalica-cli/Cargo.toml"), "evil-sys = { path = \"../../vendor/evil-sys\" }");
        },
    },
    Plant {
        name: "std::fs::write outside fs",
        gate: &["xtask", "layers"],
        expect: "rule 4: dalica-store",
        apply: |t| append(&t.join("crates/store/src/lib.rs"), "\npub fn planted() {\n    let _ = std::fs::write(\"x\", b\"y\");\n}\n"),
    },
];

/// Undo a plant by restoring every file of the original tree and deleting files it did not have.
fn restore(tree: &Path) {
    let root = workspace_root();
    for f in ["crates/library/Cargo.toml", "crates/core/src/lib.rs", "apps/dalica-cli/Cargo.toml", "crates/store/src/lib.rs"] {
        std::fs::copy(root.join(f), tree.join(f)).unwrap();
    }
    for f in ["docs/planted.png", "docs/planted.md"] {
        let _ = std::fs::remove_file(tree.join(f));
    }
    let _ = std::fs::remove_dir_all(tree.join("vendor"));
}

#[test]
fn every_gate_bites() {
    let started = Instant::now();
    let base = target_dir().join("accept-m0");
    let tree = base.join("tree");
    let target = base.join("target");
    copy_tree(&tree);

    let mut bitten = 0;
    for plant in PLANTS {
        let (ok, out) = gate(&tree, &target, plant.gate);
        assert!(ok, "{}: the gate `cargo {}` fails on the clean copy:\n{out}", plant.name, plant.gate.join(" "));
        (plant.apply)(&tree);
        let (ok, out) = gate(&tree, &target, plant.gate);
        assert!(!ok, "{}: `cargo {}` passed with the violation planted:\n{out}", plant.name, plant.gate.join(" "));
        assert!(out.contains(plant.expect), "{}: the failure does not mention `{}`:\n{out}", plant.name, plant.expect);
        restore(&tree);
        let (ok, out) = gate(&tree, &target, plant.gate);
        assert!(ok, "{}: `cargo {}` still fails after removing the violation:\n{out}", plant.name, plant.gate.join(" "));
        bitten += 1;
        println!("gate bites: {} (cargo {})", plant.name, plant.gate.join(" "));
    }
    println!("{bitten}/{} gates bite ({:.1} s)", PLANTS.len(), started.elapsed().as_secs_f64());
    assert_eq!(bitten, 6);
}
