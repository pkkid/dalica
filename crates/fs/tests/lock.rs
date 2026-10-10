//! The library lock across processes (`data-model.md` §10.6). This test binary doubles as the
//! helper process: run with `DALICA_LOCK_HELPER=<mode>:<dir>`, the `helper` test takes the lock
//! (`hold`: prints HELD and keeps it until its stdin closes) or tries to (`try`: prints LOCKED or
//! ACQUIRED and exits).

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};

use dalica_fs::{Error, LibraryLock};

const HELPER: &str = "DALICA_LOCK_HELPER";

#[test]
fn helper() {
    let Ok(spec) = std::env::var(HELPER) else { return };
    let (mode, dir) = spec.split_once(':').unwrap();
    let result = LibraryLock::acquire(Path::new(dir));
    let mut out = std::io::stdout();
    match (mode, result) {
        ("hold", Ok(lock)) => {
            writeln!(out, "HELPER:HELD").unwrap();
            out.flush().unwrap();
            let mut rest = String::new();
            std::io::stdin().read_to_string(&mut rest).unwrap();
            drop(lock);
        }
        (_, Ok(_)) => writeln!(out, "HELPER:ACQUIRED").unwrap(),
        (_, Err(Error::Locked(_))) => writeln!(out, "HELPER:LOCKED").unwrap(),
        (_, Err(e)) => writeln!(out, "HELPER:ERROR {e}").unwrap(),
    }
    out.flush().unwrap();
}

fn spawn(mode: &str, dir: &Path) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .args(["helper", "--exact", "--nocapture", "--test-threads=1"])
        .env(HELPER, format!("{mode}:{}", dir.display()))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

/// What follows the helper's first `HELPER:` marker (the test harness prints its own text around it).
fn helper_line(stdout: &mut BufReader<ChildStdout>) -> String {
    let mut line = String::new();
    loop {
        line.clear();
        assert!(stdout.read_line(&mut line).unwrap() > 0, "helper exited without a HELPER: line");
        // libtest prints `test helper ... ` on the same line, before the helper's own output.
        if let Some((_, rest)) = line.trim_end().split_once("HELPER:") {
            return rest.to_string();
        }
    }
}

fn try_lock_in_another_process(dir: &Path) -> String {
    let mut child = spawn("try", dir);
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let line = helper_line(&mut out);
    // Read to the end, so the helper's last writes never hit a closed pipe.
    out.read_to_string(&mut String::new()).unwrap();
    assert!(child.wait().unwrap().success());
    line
}

#[test]
fn a_second_process_is_refused_while_the_first_holds_the_lock() {
    let dir = tempfile::tempdir().unwrap();
    let mut holder = spawn("hold", dir.path());
    let mut out = BufReader::new(holder.stdout.take().unwrap());
    assert_eq!(helper_line(&mut out), "HELD");

    assert_eq!(try_lock_in_another_process(dir.path()), "LOCKED");
    assert!(matches!(LibraryLock::acquire(dir.path()), Err(Error::Locked(_))));

    // The first process exits normally → the lock is free.
    drop(holder.stdin.take());
    out.read_to_string(&mut String::new()).unwrap();
    assert!(holder.wait().unwrap().success());
    assert_eq!(try_lock_in_another_process(dir.path()), "ACQUIRED");
}

#[test]
fn a_killed_holder_releases_the_lock() {
    let dir = tempfile::tempdir().unwrap();
    let mut holder = spawn("hold", dir.path());
    let mut out = BufReader::new(holder.stdout.take().unwrap());
    assert_eq!(helper_line(&mut out), "HELD");
    assert_eq!(try_lock_in_another_process(dir.path()), "LOCKED");

    holder.kill().unwrap();
    holder.wait().unwrap();
    let lock = LibraryLock::acquire(dir.path()).unwrap();
    assert_eq!(try_lock_in_another_process(dir.path()), "LOCKED");
    drop(lock);
}
