//! `dalica_core::build`: the version string with and without the build variables.

use dalica_core::build::{SHORT_SHA, VERSION, build_date, commit, describe, version_string};

#[test]
fn without_the_variables_it_is_a_dev_build() {
    assert_eq!(describe("0.1.0", None, None), "0.1.0 (dev build)");
    assert_eq!(describe("0.2.0-rc.1", None, None), "0.2.0-rc.1 (dev build)");
    // Blank values count as missing.
    assert_eq!(describe("0.1.0", Some(""), Some("  ")), "0.1.0 (dev build)");
}

#[test]
fn with_the_variables_it_names_the_commit_and_date() {
    let sha = "3f2a9c1d0e8b7a6f5e4d3c2b1a09f8e7d6c5b4a3";
    assert_eq!(describe("0.1.0", Some(sha), Some("2026-10-12")), "0.1.0 (3f2a9c1d0, 2026-10-12)");
    assert_eq!(describe("0.1.0", Some(sha), None), "0.1.0 (3f2a9c1d0)");
    assert_eq!(describe("0.1.0", None, Some("2026-10-12")), "0.1.0 (2026-10-12)");
    // A short commit is shown whole; surrounding blanks are trimmed.
    assert_eq!(describe("1.0.0", Some(" abc\n"), Some(" 2026-10-12 ")), "1.0.0 (abc, 2026-10-12)");
}

#[test]
fn odd_values_never_panic() {
    // Multi-byte characters at the cut, and a very long date, are cut on a character boundary.
    let s = describe("0.1.0", Some("ééééééééééééé"), Some(&"9".repeat(500)));
    assert!(s.starts_with("0.1.0 (ééééééééé, 999"), "{s}");
    assert!(s.len() < 100, "{s}");
}

#[test]
fn this_build_reports_the_workspace_version() {
    assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
    let v = version_string();
    assert!(v.starts_with(VERSION), "{v}");
    // The same variables this test was compiled with decide the suffix.
    let sha = option_env!("DALICA_BUILD_SHA").map(str::trim).filter(|s| !s.is_empty());
    let date = option_env!("DALICA_BUILD_DATE").map(str::trim).filter(|s| !s.is_empty());
    assert_eq!((commit(), build_date()), (sha, date));
    assert_eq!(v, describe(VERSION, sha, date));
    if sha.is_none() && date.is_none() {
        assert!(v.ends_with(" (dev build)"), "{v}");
    }
    if let Some(s) = sha {
        assert!(v.contains(&s.chars().take(SHORT_SHA).collect::<String>()), "{v}");
    }
}
