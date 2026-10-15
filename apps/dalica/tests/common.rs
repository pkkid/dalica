//! What the app's integration tests share: the real `dalica` binary in a sandbox of temporary
//! folders, a free port, and the quick or full switch.

#![allow(dead_code)]

use std::ffi::OsStr;
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Stdio};

/// Full strength (`DALICA_FULL_TESTS=1`, the review gate) or quick (a task's gate).
pub fn full() -> bool {
    std::env::var_os("DALICA_FULL_TESTS").is_some_and(|v| v == "1")
}

/// A port nobody listens on now (the OS picks it; it is free again once the probe closes).
pub fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// Points every folder `dalica` could touch (home, settings, caches, run and temp folders) inside
/// `home`, and drops the variables that would steer it elsewhere.
pub fn sandbox(c: &mut Command, home: &Path) {
    let tmp = home.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    for (k, v) in [
        ("HOME", home.to_path_buf()),
        ("XDG_CONFIG_HOME", home.join("config")),
        ("XDG_CACHE_HOME", home.join("cache")),
        ("DALICA_CACHE_DIR", home.join("dalica-cache")),
        ("XDG_DATA_HOME", home.join("data")),
        ("XDG_RUNTIME_DIR", home.join("run")),
        ("APPDATA", home.join("appdata")),
        ("LOCALAPPDATA", home.join("localappdata")),
        ("TMPDIR", tmp.clone()),
        ("TMP", tmp.clone()),
        ("TEMP", tmp),
    ] {
        c.env(k, v);
    }
    for k in [
        "DALICA_HOME",
        "DALICA_CONTROL_PORT",
        "DALICA_CONTROL_TOKEN_FILE",
        "DALICA_AUTOMATION_READ_ROOT",
        "DALICA_AUTOMATION_WRITE_ROOT",
        "DALICA_PROFILE",
        "RUST_LOG",
    ] {
        c.env_remove(k);
    }
}

/// `dalica ARGS` in the sandbox of `home` ([`sandbox`]), its standard error in
/// `home/stderr.log` (a file, not a pipe: a pipe nobody reads could fill and stall the app).
pub fn dalica<S: AsRef<OsStr>>(home: &Path, args: impl IntoIterator<Item = S>) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_dalica"));
    sandbox(&mut c, home);
    let log = std::fs::OpenOptions::new().create(true).append(true).open(home.join("stderr.log")).unwrap();
    c.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::from(log));
    c
}
