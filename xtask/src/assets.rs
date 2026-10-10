//! `cargo xtask assets`: every asset file has an `ATTRIBUTION.md` row, every licence file a row
//! references exists, and nothing in the repository is named after the reference product or its
//! vendor or uses the reference application's own file formats (`docs/assets.md` §3).

use std::path::Path;

use crate::names::{Matcher, Token};

/// Extensions that make a file an asset (images, icons, fonts, sounds, video, raw/TIFF, ICC, XMP).
pub const ASSET_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "webp", "svg", "ico", "icns", "tif", "tiff", "heic", "heif", "avif", "jxl", "psd", "dng", "cr2", "cr3",
    "nef", "arw", "orf", "rw2", "raf", "pef", "srw", "raw", "ttf", "otf", "ttc", "woff", "woff2", "wav", "mp3", "ogg", "flac", "aac", "m4a", "mp4",
    "mov", "avi", "mkv", "webm", "icc", "icm", "xmp",
];

/// Names that may not appear anywhere in a file's path: the reference product and its vendor.
pub const FORBIDDEN_IN_NAMES: &[Token] = &[
    Token { len: 6, fnv: 0xdd0d_db4a, sha256: "7fcf284f3cd606b2f6a6c6bcb5adbe4d60af1a7c7654f2669cb8074c0cdfb682" },
    Token { len: 6, fnv: 0x9da4_e586, sha256: "bbdefa2950f49882f295b1285d4fa9dec45fc4144bfb07ee6acc68762d12c2e3" },
];

/// The reference application's own file formats: extensions (album and database column files).
pub const FORBIDDEN_EXTENSIONS: &[Token] = &[
    Token { len: 3, fnv: 0x5d37_efe2, sha256: "8c962f45f55170088bbb684e8b299b8304e7221510ee31aca47d0e8520230213" },
    Token { len: 3, fnv: 0x5155_f212, sha256: "a1a38b5bae5866d7d998a9834229ec2f9db7a4fc8fb6f58b1115a96a446875ff" },
];

/// …and whole file names (its index, cache and repository files).
pub const FORBIDDEN_FILE_NAMES: &[Token] = &[
    Token { len: 13, fnv: 0xef8c_6813, sha256: "d94cb4b83279d0cdf5850ae219fde7c5b8f69bed2f584da1d2e6e7376aebe1b4" },
    Token { len: 15, fnv: 0xc1bf_4df1, sha256: "b755705a49b2a5af030e82a74932fe66f7c52eaf01ef7d58b8b59a3b5ddf9fda" },
    Token { len: 15, fnv: 0x9d71_8e1e, sha256: "d8c79370eeb59a19a4a327b59b10a580ba713a240bd74283df115b1503692ea8" },
    Token { len: 14, fnv: 0x50d5_1a2a, sha256: "739991f808c40eccaf3f8287d783b20564f10c7a207b58d6c32cbefa627c8515" },
    Token { len: 12, fnv: 0xa1cc_2b26, sha256: "22b1abcf4c1a96dc0c946a16a65ec14d8e75946feaed12ddb3c1ac508fbe831c" },
    Token { len: 13, fnv: 0xff67_b329, sha256: "6745e6daefd1b9fcc22a757fe4afbb00e3cc170748c8063b9e20f123fe836bed" },
];

/// One `ATTRIBUTION.md` table row: its path patterns and the licence files it references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub line: usize,
    pub patterns: Vec<String>,
    pub licence_files: Vec<String>,
}

fn backticked(cell: &str) -> Vec<String> {
    cell.split('`').skip(1).step_by(2).map(str::to_string).filter(|s| !s.is_empty()).collect()
}

/// Asset rows: table lines starting with `|`, with at least the seven columns
/// Path | Asset | Creator | Source | Licence | Added | Modifications and a backticked path.
pub fn parse_attribution(text: &str) -> Vec<Row> {
    let mut rows = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line.trim().trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() < 7 {
            continue;
        }
        let patterns = backticked(cells.first().copied().unwrap_or(""));
        if patterns.is_empty() {
            continue;
        }
        rows.push(Row { line: i + 1, patterns, licence_files: backticked(cells.get(4).copied().unwrap_or("")) });
    }
    rows
}

/// `*` matches any characters except `/`; a pattern ending in `/` covers everything under it.
pub fn pattern_matches(pattern: &str, path: &str) -> bool {
    if let Some(dir) = pattern.strip_suffix('/') {
        return path.starts_with(&format!("{dir}/"));
    }
    glob(pattern.as_bytes(), path.as_bytes())
}

fn glob(p: &[u8], s: &[u8]) -> bool {
    // Iterative wildcard match with single-star backtracking (no recursion, linear-ish).
    let (mut pi, mut si) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while si < s.len() {
        match p.get(pi) {
            Some(b'*') => {
                star = Some((pi, si));
                pi += 1;
            }
            Some(c) if Some(c) == s.get(si) => {
                pi += 1;
                si += 1;
            }
            _ => match star {
                Some((sp, ss)) if s.get(ss) != Some(&b'/') => {
                    pi = sp + 1;
                    si = ss + 1;
                    star = Some((sp, ss + 1));
                }
                _ => return false,
            },
        }
    }
    p.get(pi..).is_some_and(|rest| rest.iter().all(|c| *c == b'*'))
}

fn extension(path: &str) -> Option<String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').filter(|(stem, _)| !stem.is_empty()).map(|(_, ext)| ext.to_ascii_lowercase())
}

pub struct Rules {
    pub names: Matcher,
    pub extensions: Matcher,
    pub file_names: Matcher,
}

impl Rules {
    pub fn real() -> Rules {
        Rules {
            names: Matcher::real(FORBIDDEN_IN_NAMES),
            extensions: Matcher::real(FORBIDDEN_EXTENSIONS),
            file_names: Matcher::real(FORBIDDEN_FILE_NAMES),
        }
    }
}

pub fn check(root: &Path, files: &[String], attribution: &str, rules: &Rules) -> Vec<String> {
    let mut out = Vec::new();
    let rows = parse_attribution(attribution);
    for row in &rows {
        for licence in &row.licence_files {
            if !root.join(licence).is_file() {
                out.push(format!("ATTRIBUTION.md:{}: licence file `{licence}` does not exist", row.line));
            }
        }
    }
    for file in files {
        let name = file.rsplit('/').next().unwrap_or(file);
        if rules.names.find(file).is_some() {
            out.push(format!("{file}: the path contains the reference product's or its vendor's name"));
        }
        if rules.file_names.is(name) || extension(file).is_some_and(|e| rules.extensions.is(&e)) {
            out.push(format!("{file}: one of the reference application's own file formats; tests generate synthetic data instead"));
        }
        let is_asset = extension(file).is_some_and(|e| ASSET_EXTENSIONS.contains(&e.as_str()));
        if is_asset && !rows.iter().any(|r| r.patterns.iter().any(|p| pattern_matches(p, file))) {
            out.push(format!("{file}: asset without an ATTRIBUTION.md row (docs/assets.md §2)"));
        }
    }
    out
}

pub fn run(root: &Path) -> Result<(), String> {
    let files = crate::names::repo_files(root)?;
    let attribution = std::fs::read_to_string(root.join("ATTRIBUTION.md")).map_err(|e| format!("cannot read ATTRIBUTION.md: {e}"))?;
    let problems = check(root, &files, &attribution, &Rules::real());
    for p in &problems {
        eprintln!("assets: {p}");
    }
    if problems.is_empty() {
        let assets = files.iter().filter(|f| extension(f).is_some_and(|e| ASSET_EXTENSIONS.contains(&e.as_str()))).count();
        println!("assets: ok ({assets} assets in {} files, {} attribution rows)", files.len(), parse_attribution(&attribution).len());
        Ok(())
    } else {
        Err(format!("assets: {} problem(s)", problems.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "| Path | Asset | Creator | Source | Licence | Added | Modifications |\n|---|---|---|---|---|---|---|\n";

    fn rules() -> Rules {
        Rules { names: Matcher::words(&["zebra", "acme"]), extensions: Matcher::words(&["zdb"]), file_names: Matcher::words(&["zindex.db"]) }
    }

    fn tree(files: &[&str]) -> (tempfile::TempDir, Vec<String>) {
        let dir = tempfile::tempdir().unwrap();
        for f in files {
            let p = dir.path().join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        }
        (dir, files.iter().map(|f| f.to_string()).collect())
    }

    #[test]
    fn patterns() {
        assert!(pattern_matches("assets/fonts/Inter-*.ttf", "assets/fonts/Inter-Regular.ttf"));
        assert!(!pattern_matches("assets/fonts/Inter-*.ttf", "assets/fonts/sub/Inter-Regular.ttf"));
        assert!(pattern_matches("docs/mockups/*.png", "docs/mockups/library-900x600.png"));
        assert!(pattern_matches("assets/app-icon/", "assets/app-icon/x/y.png"));
        assert!(!pattern_matches("assets/app-icon/", "assets/app-icons.png"));
        assert!(pattern_matches("a/*b*c.png", "a/xbyc.png"));
        assert!(!pattern_matches("a/*.png", "a/b/c.png"));
    }

    #[test]
    fn rows_parse_only_real_tables() {
        let text = format!(
            "{HEADER}| `assets/fonts/Inter-*.ttf` (Regular) | Inter | R | url | OFL (`assets/fonts/OFL-Inter.txt`) | 2026 | none |\n\
             e.g. | `x.png` | not a row |\n| Font | Use | Licence |\n"
        );
        let rows = parse_attribution(&text);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].patterns, ["assets/fonts/Inter-*.ttf"]);
        assert_eq!(rows[0].licence_files, ["assets/fonts/OFL-Inter.txt"]);
    }

    #[test]
    fn unlisted_png_fails_listed_passes() {
        let (dir, files) = tree(&["docs/img/shot.png", "docs/img/other.png", "src/main.rs"]);
        let text = format!("{HEADER}| `docs/img/shot.png` | Shot | us | original work | MIT | 2026-10-09 | none |\n");
        assert_eq!(check(dir.path(), &files, &text, &rules()), ["docs/img/other.png: asset without an ATTRIBUTION.md row (docs/assets.md §2)"]);
    }

    #[test]
    fn row_pointing_at_a_missing_licence_file_fails() {
        let (dir, files) = tree(&["assets/fonts/Inter-Regular.ttf", "LICENSE-MIT"]);
        let text = format!(
            "{HEADER}| `assets/fonts/Inter-*.ttf` | Inter | R | url | OFL (`assets/fonts/OFL-Inter.txt`) | 2026 | none |\n\
             | `docs/x/` | X | us | original work | MIT (`LICENSE-MIT`) | 2026 | none |\n"
        );
        assert_eq!(check(dir.path(), &files, &text, &rules()), ["ATTRIBUTION.md:3: licence file `assets/fonts/OFL-Inter.txt` does not exist"]);
    }

    #[test]
    fn forbidden_names_and_formats_fail() {
        let (dir, files) =
            tree(&["fixtures/Zebra2Albums/a.txt", "data/acme-export.json", "fixtures/col.zdb", "fixtures/ZINDEX.db", "fine/zindex.dbx"]);
        let problems = check(dir.path(), &files, HEADER, &rules());
        assert_eq!(problems.len(), 4, "{problems:#?}");
        assert!(problems.iter().all(|p| !p.starts_with("fine/")));
    }

    #[test]
    fn the_real_lists_recognise_what_agents_md_names() {
        // AGENTS.md (exempt from `names`) spells the product name; the hashed list must recognise it.
        let text = std::fs::read_to_string(crate::workspace_root().join("AGENTS.md")).unwrap();
        assert!(Rules::real().names.find(&text).is_some());
    }
}
