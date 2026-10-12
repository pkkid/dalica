//! Version and build provenance shared by every binary (`dalica --version`, `dalica-cli --version`,
//! Help ▸ About Dalica; `docs/releasing.md` → What gets built).
//!
//! Adapted from PhotoCraft `crates/engine/src/build_info.rs` (revision bb17dd55); MIT OR Apache-2.0.
//!
//! The version is the workspace version (`[workspace.package] version` in the root `Cargo.toml`,
//! managed by `cargo xtask version`). The commit and the build date are optional and come from
//! environment variables at compile time, set by CI and the packaging scripts:
//!
//! - `DALICA_BUILD_SHA`: the git commit (any length; shown shortened to [`SHORT_SHA`] characters);
//! - `DALICA_BUILD_DATE`: the build date, conventionally `YYYY-MM-DD`.
//!
//! Without them (a plain `cargo build`) the build reports itself as `<version> (dev build)`.
//! Nothing here runs git, so a build from a source tarball works the same. Cargo rebuilds this
//! crate when either variable changes (`option_env!` is tracked).

/// The workspace version, such as `0.1.0` or `0.2.0-rc.1`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// How many characters of the commit [`version_string`] shows.
pub const SHORT_SHA: usize = 9;

/// The longest build date shown; a longer value is cut (it is a compile-time input, but still text).
const MAX_DATE: usize = 32;

/// A trimmed, non-empty value, else `None`.
fn present(v: Option<&str>) -> Option<&str> {
    v.map(str::trim).filter(|s| !s.is_empty())
}

/// The full git commit this build was made from, when the build recorded it.
pub fn commit() -> Option<&'static str> {
    present(option_env!("DALICA_BUILD_SHA"))
}

/// The build date (`YYYY-MM-DD`), when the build recorded it.
pub fn build_date() -> Option<&'static str> {
    present(option_env!("DALICA_BUILD_DATE"))
}

/// The first `n` characters of `s` (never splits a character).
fn cut(s: &str, n: usize) -> &str {
    s.char_indices().nth(n).and_then(|(i, _)| s.get(..i)).unwrap_or(s)
}

/// `version` with its provenance: `0.1.0 (3f2a9c1d0, 2026-10-01)`, `0.1.0 (3f2a9c1d0)`,
/// `0.1.0 (2026-10-01)`, or `0.1.0 (dev build)` when neither is known. Blank values count as
/// missing; the commit is shortened to [`SHORT_SHA`] characters.
pub fn describe(version: &str, sha: Option<&str>, date: Option<&str>) -> String {
    let sha = present(sha).map(|s| cut(s, SHORT_SHA));
    let date = present(date).map(|d| cut(d, MAX_DATE));
    match (sha, date) {
        (Some(s), Some(d)) => format!("{version} ({s}, {d})"),
        (Some(s), None) => format!("{version} ({s})"),
        (None, Some(d)) => format!("{version} ({d})"),
        (None, None) => format!("{version} (dev build)"),
    }
}

/// This build's version string: [`describe`] of [`VERSION`], [`commit`] and [`build_date`].
pub fn version_string() -> String {
    describe(VERSION, commit(), build_date())
}
