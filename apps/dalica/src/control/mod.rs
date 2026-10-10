//! The control channel of the desktop app (`control-protocol.md`): `--control [PORT]` and its
//! flags, the token file, the TCP transport and the two ways requests are answered: in the window
//! ([`play`]) or with no window on the headless driver (`--headless`, [`headless`]). The method
//! handlers themselves are `dalica_ui::control`'s, shared by both.

pub mod headless;
pub mod play;
pub mod token;
pub mod transport;

use std::ffi::OsString;
use std::net::TcpListener;
use std::path::{Path, PathBuf};

use dalica_ui::control::Config;

pub use transport::{Hub, Waker};

/// The port when `--control` names none and `DALICA_CONTROL_PORT` is not set (§1).
pub const DEFAULT_PORT: u16 = 7990;

/// The control flags of the command line (§1).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Flags {
    /// `--control PORT`; `None` for a bare `--control`.
    pub port: Option<u16>,
    pub token_file: Option<PathBuf>,
    pub read_roots: Vec<PathBuf>,
    pub write_roots: Vec<PathBuf>,
    pub allow_external: bool,
}

/// A port number for `--control`: 1–65535.
pub fn parse_port(text: &str) -> Result<u16, String> {
    match text.parse::<u16>() {
        Ok(0) | Err(_) => Err(format!("`{text}` is not a port: give a number 1–65535")),
        Ok(p) => Ok(p),
    }
}

/// The flags with their environment equivalents filled in where a flag is absent (§1): the port
/// (`DALICA_CONTROL_PORT`, only when `--control` is given without one; the variable never turns
/// the channel on), the token file and the automation roots (`:`-separated, `;` on Windows).
/// Relative roots are made absolute against `cwd`.
pub fn resolve(mut flags: Flags, var: impl Fn(&str) -> Option<OsString>, cwd: &Path) -> Result<(u16, Flags), String> {
    let port = match flags.port {
        Some(p) => p,
        None => match var("DALICA_CONTROL_PORT").filter(|v| !v.is_empty()) {
            Some(v) => parse_port(&v.to_string_lossy()).map_err(|e| format!("DALICA_CONTROL_PORT: {e}"))?,
            None => DEFAULT_PORT,
        },
    };
    if flags.token_file.is_none() {
        flags.token_file = var("DALICA_CONTROL_TOKEN_FILE").filter(|v| !v.is_empty()).map(PathBuf::from);
    }
    let roots = |given: &mut Vec<PathBuf>, name: &str| {
        if given.is_empty()
            && let Some(v) = var(name)
        {
            given.extend(std::env::split_paths(&v).filter(|p| !p.as_os_str().is_empty()));
        }
        for r in given.iter_mut() {
            if r.is_relative() {
                *r = cwd.join(&*r);
            }
        }
    };
    roots(&mut flags.read_roots, "DALICA_AUTOMATION_READ_ROOT");
    roots(&mut flags.write_roots, "DALICA_AUTOMATION_WRITE_ROOT");
    if let Some(t) = &mut flags.token_file
        && t.is_relative()
    {
        *t = cwd.join(&*t);
    }
    Ok((port, flags))
}

impl Flags {
    /// What automation may do (§5.6, §5.7).
    pub fn config(&self, demo: bool) -> Config {
        Config { read_roots: self.read_roots.clone(), write_roots: self.write_roots.clone(), allow_external: self.allow_external, demo }
    }
}

/// A bound listener and its token, before it accepts anything.
pub struct Listening {
    listener: TcpListener,
    token: token::Token,
    pub port: u16,
    pub token_file: TokenFileGuard,
}

/// The token file in use; [`TokenFileGuard::finish`] removes a default one at exit.
#[derive(Debug)]
pub struct TokenFileGuard {
    pub path: PathBuf,
    remove: bool,
}

impl TokenFileGuard {
    /// Removes the default token file (a named one is kept), on its own thread: the caller may be
    /// the thread that was the UI thread.
    pub fn finish(self) {
        if !self.remove {
            return;
        }
        let path = self.path;
        let shown = path.display().to_string();
        let removed = std::thread::Builder::new().name("dalica-cleanup".into()).spawn(move || dalica_fs::remove_own_file(&path)).map(|t| t.join());
        match removed {
            Ok(Ok(Ok(()))) => log::info!("control: removed the token file {shown}"),
            _ => log::warn!("control: the token file {shown} was not removed"),
        }
    }
}

/// Binds `127.0.0.1:port` (an error if it is taken: the app then exits 2), then sets up the token
/// file: the named one (`--control-token-file`), or a fresh default one for this launch.
pub fn listen(port: u16, token_file: Option<&Path>) -> Result<Listening, String> {
    let listener = transport::bind(port).map_err(|e| {
        let why = if e.kind() == std::io::ErrorKind::AddrInUse {
            "the port is in use (another Dalica, or another program)".to_string()
        } else {
            e.to_string()
        };
        format!("cannot listen on 127.0.0.1:{port}: {why}")
    })?;
    let file = match token_file {
        Some(path) => token::named(path)?,
        None => {
            let path = token::default_file(port, |k| std::env::var_os(k))
                .ok_or("no folder for the token file: set HOME (LOCALAPPDATA on Windows) or pass --control-token-file")?;
            token::fresh_default(&path)?
        }
    };
    let guard = TokenFileGuard { path: file.path.clone(), remove: file.remove_on_exit };
    Ok(Listening { listener, token: file.token, port, token_file: guard })
}

impl Listening {
    /// Starts accepting connections; `wake` is called when a request arrives.
    pub fn start(self, wake: Waker) -> Result<(Hub, TokenFileGuard), String> {
        let hub = transport::start(self.listener, self.token, wake).map_err(|e| format!("cannot start the control channel: {e}"))?;
        log::info!("control: listening on 127.0.0.1:{}; token file {}", self.port, self.token_file.path.display());
        Ok((hub, self.token_file))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let pairs: Vec<(String, String)> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| OsString::from(v))
    }

    fn cwd() -> PathBuf {
        std::env::temp_dir().join("cwd")
    }

    #[test]
    fn ports_are_1_to_65535() {
        assert_eq!(parse_port("7990"), Ok(7990));
        assert_eq!(parse_port("65535"), Ok(65535));
        for bad in ["0", "65536", "-1", "", "x", "18040x"] {
            assert!(parse_port(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_environment_fills_in_what_the_flags_leave_out() {
        let (port, f) = resolve(Flags::default(), env(&[]), &cwd()).unwrap();
        assert_eq!((port, f.clone()), (DEFAULT_PORT, Flags::default()));
        let sep = if cfg!(windows) { ";" } else { ":" };
        let a = cwd().join("a");
        let roots = format!("{}{sep}b", a.display());
        let vars = env(&[("DALICA_CONTROL_PORT", "18040"), ("DALICA_CONTROL_TOKEN_FILE", "t.token"), ("DALICA_AUTOMATION_WRITE_ROOT", &roots)]);
        let (port, f) = resolve(Flags::default(), vars, &cwd()).unwrap();
        assert_eq!(port, 18040);
        assert_eq!(f.token_file, Some(cwd().join("t.token")));
        assert_eq!(f.write_roots, vec![a.clone(), cwd().join("b")]);
        assert!(f.read_roots.is_empty());
        // A flag wins over its variable.
        let given = Flags { port: Some(18041), write_roots: vec![a.clone()], ..Flags::default() };
        let (port, f) = resolve(given, env(&[("DALICA_CONTROL_PORT", "18040"), ("DALICA_AUTOMATION_WRITE_ROOT", "/x")]), &cwd()).unwrap();
        assert_eq!((port, f.write_roots), (18041, vec![a]));
        assert!(resolve(Flags::default(), env(&[("DALICA_CONTROL_PORT", "0")]), &cwd()).is_err());
    }

    #[test]
    fn flags_make_the_automation_config() {
        let f = Flags { read_roots: vec![cwd()], allow_external: true, ..Flags::default() };
        let c = f.config(true);
        assert_eq!((c.read_roots, c.write_roots.is_empty(), c.allow_external, c.demo), (vec![cwd()], true, true, true));
    }

    #[test]
    fn listening_on_a_taken_port_fails_and_says_why() {
        let taken = transport::bind(0).unwrap();
        let port = taken.local_addr().unwrap().port();
        let dir = tempfile::tempdir().unwrap();
        let e = listen(port, Some(&dir.path().join("t.token"))).err().unwrap();
        assert!(e.contains(&format!("127.0.0.1:{port}")), "{e}");
        // The token file is not made when the port is taken.
        assert!(!dir.path().join("t.token").exists());
        drop(taken);
        let l = listen(port, Some(&dir.path().join("t.token"))).unwrap();
        assert!(dir.path().join("t.token").is_file());
        assert_eq!(l.port, port);
    }
}
