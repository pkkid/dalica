//! The control token (`control-protocol.md` §2.1, `security.md` §6.3): 256 bits from the OS random
//! source, written as 64 hex characters to a file only its owner may read, and compared in constant
//! time. The token is never logged, printed or put in a reply; only the file's path is.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Hex characters in a token.
pub const TOKEN_HEX: usize = 64;
/// The longest token file read (its first line is the token; the rest is ignored).
const MAX_FILE: u64 = 4096;

/// A bearer token. `Debug` never shows it.
#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(…)")
    }
}

impl Token {
    /// A fresh token from the OS random source.
    pub fn generate() -> Result<Token, String> {
        let mut bytes = [0u8; TOKEN_HEX / 2];
        getrandom::fill(&mut bytes).map_err(|e| format!("the OS random source failed: {e}"))?;
        Ok(Token(bytes.iter().map(|b| format!("{b:02x}")).collect()))
    }

    /// The token on the first line of a token file's text: exactly 64 hex characters.
    pub fn from_file_text(text: &[u8]) -> Option<Token> {
        let line = text.split(|&b| b == b'\n').next().unwrap_or_default();
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let ok = line.len() == TOKEN_HEX && line.iter().all(u8::is_ascii_hexdigit);
        ok.then(|| Token(String::from_utf8_lossy(line).into_owned()))
    }

    /// Whether `given` is this token. The time taken depends only on the token's length, never on
    /// where `given` differs or how long it is.
    pub fn matches(&self, given: &str) -> bool {
        let (a, b) = (self.0.as_bytes(), given.as_bytes());
        let mut diff = u8::from(a.len() != b.len());
        for (i, x) in a.iter().enumerate() {
            diff |= x ^ b.get(i).copied().unwrap_or(0);
        }
        std::hint::black_box(diff) == 0
    }

    /// The token itself, for tests that play the client.
    #[cfg(test)]
    pub fn reveal_for_test(&self) -> &str {
        &self.0
    }

    fn file_text(&self) -> String {
        format!("{}\n", self.0)
    }
}

/// Where Dalica keeps the default token files: `$XDG_RUNTIME_DIR/dalica` on Linux when set, else the
/// cache folder's `run` (Linux `$XDG_CACHE_HOME/dalica/run`, macOS `~/Library/Caches/Dalica/run`,
/// Windows `%LOCALAPPDATA%\Dalica\cache\run`). `None` without a home.
pub fn run_dir(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let abs = |k: &str| var(k).filter(|v| !v.is_empty()).map(PathBuf::from).filter(|p| p.is_absolute());
    if cfg!(windows) {
        abs("LOCALAPPDATA").map(|p| p.join("Dalica").join("cache").join("run"))
    } else if cfg!(target_os = "macos") {
        abs("HOME").map(|h| h.join("Library").join("Caches").join("Dalica").join("run"))
    } else if let Some(run) = abs("XDG_RUNTIME_DIR") {
        Some(run.join("dalica"))
    } else {
        abs("XDG_CACHE_HOME").or_else(|| abs("HOME").map(|h| h.join(".cache"))).map(|c| c.join("dalica").join("run"))
    }
}

/// The default token file for `port`: `<run>/control-<port>.token`.
pub fn default_file(port: u16, var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    run_dir(var).map(|d| d.join(format!("control-{port}.token")))
}

/// The token in use and the file that holds it.
#[derive(Debug)]
pub struct TokenFile {
    pub path: PathBuf,
    pub token: Token,
    /// The default file is made for this launch and removed on exit; a named one is kept.
    pub remove_on_exit: bool,
}

/// `--control-token-file PATH`: reuses an existing private file, or creates it with a fresh token.
/// Refuses a link, a folder, a file others may read or write (Unix), or one without a token.
pub fn named(path: &Path) -> Result<TokenFile, String> {
    let shown = path.display();
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(format!("the token file {shown} is a symbolic link; give the real file"));
            }
            if !meta.is_file() {
                return Err(format!("the token file {shown} is not a regular file"));
            }
            check_private(path, &meta)?;
            let read = dalica_fs::read(path, MAX_FILE).map_err(|e| format!("cannot read the token file {shown}: {e}"))?;
            let text = read.map(|r| r.bytes).unwrap_or_default();
            let token = Token::from_file_text(&text).ok_or_else(|| {
                format!("the token file {shown} does not start with a {TOKEN_HEX}-character hex token; delete it to make a new one")
            })?;
            Ok(TokenFile { path: path.to_path_buf(), token, remove_on_exit: false })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let token = write_new(path, dalica_fs::Expect::Absent)?;
            Ok(TokenFile { path: path.to_path_buf(), token, remove_on_exit: false })
        }
        Err(e) => Err(format!("cannot look at the token file {shown}: {e}")),
    }
}

/// The default token file: a fresh token for this launch, replacing what an earlier launch on the
/// same port left behind; removed on exit.
pub fn fresh_default(path: &Path) -> Result<TokenFile, String> {
    let expect = match dalica_fs::identity(path) {
        Ok(Some(id)) => dalica_fs::Expect::Unchanged(id),
        Ok(None) => dalica_fs::Expect::Absent,
        Err(e) => return Err(format!("cannot use the token file {}: {e}", path.display())),
    };
    let token = write_new(path, expect)?;
    Ok(TokenFile { path: path.to_path_buf(), token, remove_on_exit: true })
}

/// Writes a fresh token to `path` (owner-only), making its folder (0700) when missing.
fn write_new(path: &Path, expect: dalica_fs::Expect) -> Result<Token, String> {
    let token = Token::generate()?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty())
        && !dir.is_dir()
    {
        dalica_fs::create_private_dir_all(dir).map_err(|e| format!("cannot make the folder for the token file: {e}"))?;
    }
    dalica_fs::replace_private(path, token.file_text().as_bytes(), expect)
        .map_err(|e| format!("cannot write the token file {}: {e}", path.display()))?;
    Ok(token)
}

/// On Unix, a token file group or others may read or write is refused (`security.md` §6.3).
#[cfg(unix)]
fn check_private(path: &Path, meta: &std::fs::Metadata) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = meta.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        let p = path.display();
        return Err(format!(
            "the token file {p} can be read or written by other users (mode {mode:03o}); run `chmod 600 {p}`, or delete it to make a new one"
        ));
    }
    Ok(())
}

/// Elsewhere (Windows) there are no mode bits: the file keeps the ACL of the folder it is in, which
/// in the user's profile is private to the user.
#[cfg(not(unix))]
fn check_private(_path: &Path, _meta: &std::fs::Metadata) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let pairs: Vec<(String, String)> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| OsString::from(v))
    }

    #[test]
    fn a_token_is_64_hex_characters_and_never_shown_by_debug() {
        let (a, b) = (Token::generate().unwrap(), Token::generate().unwrap());
        assert_ne!(a, b);
        assert_eq!(a.0.len(), TOKEN_HEX);
        assert!(a.0.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        assert_eq!(format!("{a:?}"), "Token(…)");
        assert!(a.matches(&a.0.clone()));
        assert!(!a.matches(&b.0));
    }

    #[test]
    fn matching_needs_the_whole_token_exactly() {
        let t = Token("ab".repeat(32));
        assert!(t.matches(&"ab".repeat(32)));
        for bad in [String::new(), "ab".repeat(31), "ab".repeat(33), format!("{}ac", "ab".repeat(31)), "AB".repeat(32), "x".repeat(1 << 20)] {
            assert!(!t.matches(&bad), "{}", bad.len());
        }
        // A given value that is the token plus a NUL is not the token.
        assert!(!t.matches(&format!("{}\0", "ab".repeat(32))));
    }

    #[test]
    fn a_token_file_holds_the_token_on_its_first_line() {
        let hex = "0123456789abcdef".repeat(4);
        assert_eq!(Token::from_file_text(hex.as_bytes()), Some(Token(hex.clone())));
        assert_eq!(Token::from_file_text(format!("{hex}\r\nmore").as_bytes()), Some(Token(hex.clone())));
        for bad in ["", "abc", &format!("{hex}0"), &format!(" {hex}"), &"g".repeat(64)] {
            assert_eq!(Token::from_file_text(bad.as_bytes()), None, "{bad:?}");
        }
        assert_eq!(Token::from_file_text(&[0xff; 64]), None);
    }

    #[test]
    fn the_default_token_file_is_in_the_run_folder() {
        assert_eq!(default_file(7990, env(&[])), None);
        if cfg!(windows) {
            let local = "C:\\Users\\a\\AppData\\Local";
            let want = PathBuf::from(local).join("Dalica").join("cache").join("run").join("control-7990.token");
            assert_eq!(default_file(7990, env(&[("LOCALAPPDATA", local)])), Some(want));
        } else if cfg!(target_os = "macos") {
            assert_eq!(
                default_file(7990, env(&[("HOME", "/Users/a")])),
                Some(PathBuf::from("/Users/a/Library/Caches/Dalica/run/control-7990.token"))
            );
        } else {
            assert_eq!(default_file(1, env(&[("HOME", "/home/a")])), Some(PathBuf::from("/home/a/.cache/dalica/run/control-1.token")));
            assert_eq!(default_file(1, env(&[("HOME", "/home/a"), ("XDG_CACHE_HOME", "/c")])), Some(PathBuf::from("/c/dalica/run/control-1.token")));
            let run = env(&[("HOME", "/home/a"), ("XDG_RUNTIME_DIR", "/run/user/1000")]);
            assert_eq!(default_file(18040, run), Some(PathBuf::from("/run/user/1000/dalica/control-18040.token")));
            // A relative value is ignored.
            assert_eq!(
                default_file(1, env(&[("HOME", "/home/a"), ("XDG_RUNTIME_DIR", "run")])),
                Some(PathBuf::from("/home/a/.cache/dalica/run/control-1.token"))
            );
        }
    }

    #[test]
    fn a_named_file_is_created_private_then_reused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tokens").join("agent.token");
        let made = named(&path).unwrap();
        assert!(!made.remove_on_exit);
        let text = std::fs::read(&path).unwrap();
        assert_eq!(Token::from_file_text(&text), Some(made.token.clone()));
        let again = named(&path).unwrap();
        assert_eq!(again.token, made.token);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
            assert_eq!(std::fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        }
    }

    #[test]
    fn a_named_file_without_a_token_or_that_is_a_folder_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t");
        dalica_fs::replace_private(&path, b"not a token\n", dalica_fs::Expect::Absent).unwrap();
        assert!(named(&path).unwrap_err().contains("does not start with"));
        assert!(named(dir.path()).unwrap_err().contains("not a regular file"));
    }

    #[cfg(unix)]
    #[test]
    fn a_named_file_others_may_read_or_a_link_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t");
        named(&path).unwrap();
        for mode in [0o640, 0o604, 0o620, 0o602] {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            let e = named(&path).unwrap_err();
            assert!(e.contains("chmod 600") && !e.contains(&named_token(&path)), "{e}");
        }
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(named(&link).unwrap_err().contains("symbolic link"));
    }

    #[cfg(unix)]
    fn named_token(path: &Path) -> String {
        String::from_utf8(std::fs::read(path).unwrap()).unwrap().trim().to_string()
    }

    #[test]
    fn the_default_file_is_fresh_for_every_launch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run").join("control-18040.token");
        let a = fresh_default(&path).unwrap();
        assert!(a.remove_on_exit);
        let b = fresh_default(&path).unwrap();
        assert_ne!(a.token, b.token);
        assert_eq!(Token::from_file_text(&std::fs::read(&path).unwrap()), Some(b.token));
    }
}
