//! Real crashes (`docs/standards/never-crash.md`, `data-model.md` §6): a child process writes in a
//! loop and is killed at a random moment (`SIGKILL` on Unix, `TerminateProcess` on Windows), then
//! this process checks the disk. The fault points stop a write at a named step; these kill it
//! wherever it happens to be, in or between system calls.
//!
//! - **Atomic replace:** the target holds the whole of the last version the child finished or of
//!   the one it was writing, never a mix and never an older one.
//! - **The originals backup before a Save** (back up, write the saved file, Undo Save, again): the
//!   original's bytes are whole under exactly one of the two names, and a saved file is whole.
//!
//! Five kills each in a task's gate, 50 at full strength (`DALICA_FULL_TESTS=1`). This test binary
//! doubles as the helper: run with `DALICA_CRASH_HELPER=<mode>:<dir>`, the `crash::helper` test
//! writes until it is killed (or gives up after a while), printing `HELPER:DONE <n>` after each
//! step that returned.

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dalica_fs::atomic::is_temp_name;
use dalica_fs::originals::{DIR_NAME, Recorded, back_up, restore};
use dalica_fs::{Expect, identity, replace};

const HELPER: &str = "DALICA_CRASH_HELPER";
const NAME: &str = "photo.jpg";
/// How long a helper writes before it gives up on being killed.
const HELPER_LIMIT: Duration = Duration::from_secs(30);
/// The bytes of each version: large enough that a write takes a while.
const SIZE: usize = 256 << 10;

/// Full strength (`DALICA_FULL_TESTS=1`, the review gate) or quick (a task's gate).
fn full() -> bool {
    std::env::var_os("DALICA_FULL_TESTS").is_some_and(|v| v == "1")
}

fn kills() -> u32 {
    if full() { 50 } else { 5 }
}

/// Version `n` of a file: a header naming it, then bytes that differ per version and position.
fn version(tag: &str, n: u64) -> Vec<u8> {
    let mut v = format!("{tag} {n:012}\n").into_bytes();
    let start = v.len() as u64;
    v.extend((start..SIZE as u64).map(|i| (i.wrapping_mul(31) ^ n.wrapping_mul(0x9e37_79b9)) as u8));
    v
}

/// Which whole version of `tag` `bytes` are, if any.
fn which(tag: &str, bytes: &[u8]) -> Option<u64> {
    let head = std::str::from_utf8(bytes.get(..tag.len() + 14)?).ok()?;
    let n = head.strip_prefix(tag)?.trim().parse().ok()?;
    (bytes == version(tag, n)).then_some(n)
}

#[test]
fn helper() {
    let Ok(spec) = std::env::var(HELPER) else { return };
    let (mode, dir) = spec.split_once(':').unwrap();
    let dir = Path::new(dir);
    let started = Instant::now();
    let result = match mode {
        "replace" => replace_loop(dir, started),
        "save" => save_loop(dir, started),
        other => Err(format!("unknown mode {other}")),
    };
    if let Err(e) = result {
        println!("HELPER:ERROR {e}");
    }
}

/// Replaces `NAME` with version 1, 2, … until killed.
fn replace_loop(dir: &Path, started: Instant) -> Result<(), String> {
    let target = dir.join(NAME);
    let mut id = identity(&target).map_err(|e| e.to_string())?.ok_or("no target")?;
    for n in 1.. {
        id = replace(&target, &version("replace", n), Expect::Unchanged(id)).map_err(|e| e.to_string())?;
        println!("HELPER:DONE {n}");
        if started.elapsed() > HELPER_LIMIT {
            break;
        }
    }
    Ok(())
}

fn original() -> Vec<u8> {
    version("original", 0)
}

/// The digest check of the originals record: the backup's bytes are the original's.
fn is_original(r: &mut dyn std::io::Read) -> std::io::Result<bool> {
    let mut got = Vec::new();
    r.read_to_end(&mut got)?;
    Ok(got == original())
}

/// A Save and its Undo, over and over: the original into the backup, saved rendering `n` under the
/// photo's name, the original back over it.
fn save_loop(dir: &Path, started: Instant) -> Result<(), String> {
    let photo = dir.join(NAME);
    let original_id = identity(&photo).map_err(|e| e.to_string())?.ok_or("no photo")?;
    let kept = Recorded { size: SIZE as u64, digest_matches: &is_original };
    for n in 1.. {
        back_up(dir, NAME, original_id, None).map_err(|e| format!("back up {n}: {e}"))?;
        let saved = replace(&photo, &version("saved", n), Expect::Absent).map_err(|e| format!("save {n}: {e}"))?;
        restore(dir, NAME, Expect::Unchanged(saved), &kept).map_err(|e| format!("restore {n}: {e}"))?;
        println!("HELPER:DONE {n}");
        if started.elapsed() > HELPER_LIMIT {
            break;
        }
    }
    Ok(())
}

/// A small random number generator (xorshift), seeded per run and named in every failure.
struct Rng(u64);

impl Rng {
    fn new() -> Rng {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64;
        Rng((nanos ^ u64::from(std::process::id()).rotate_left(32)) | 1)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

/// Runs the helper in `mode` on `dir`, kills it a random moment after its first finished step,
/// and returns the last step it said it finished.
fn run_and_kill(mode: &str, dir: &Path, rng: &mut Rng) -> u64 {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["crash::helper", "--exact", "--nocapture", "--test-threads=1"])
        .env(HELPER, format!("{mode}:{}", dir.display()))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            // libtest prints `test helper ... ` on the same line, before the helper's own output.
            if let Some((_, rest)) = line.split_once("HELPER:") {
                let _ = tx.send(rest.to_string());
            }
        }
    });
    let first = rx.recv_timeout(Duration::from_secs(120));
    if first.is_ok() {
        std::thread::sleep(Duration::from_micros(rng.below(20_000)));
    }
    let _ = child.kill();
    child.wait().unwrap();
    reader.join().unwrap();
    let lines: Vec<String> = first.into_iter().chain(rx.try_iter()).collect();
    assert!(!lines.is_empty(), "{mode}: the helper never finished a step");
    let mut last = 0;
    for line in &lines {
        match line.strip_prefix("DONE ") {
            Some(n) => last = n.parse().unwrap(),
            None => panic!("{mode}: the helper said {line}"),
        }
    }
    last
}

/// Every name in `dir` that is not a temporary file of an atomic write (a kill may leave one, for
/// `stale` to remove a day later), with its bytes (`None` for a folder).
fn contents(dir: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| !is_temp_name(&e.file_name()))
        .map(|e| {
            let p = e.path();
            (e.file_name().to_string_lossy().into_owned(), p.is_file().then(|| fs::read(&p).unwrap()))
        })
        .collect()
}

fn fresh_folder(root: &Path, i: u32, bytes: &[u8]) -> PathBuf {
    let dir = root.join(format!("run-{i}"));
    fs::create_dir(&dir).unwrap();
    replace(&dir.join(NAME), bytes, Expect::Absent).unwrap();
    dir
}

#[test]
fn a_replace_killed_at_any_moment_leaves_the_last_or_the_next_version_whole() {
    let root = tempfile::tempdir().unwrap();
    let mut rng = Rng::new();
    let seed = rng.0;
    for i in 0..kills() {
        let dir = fresh_folder(root.path(), i, &version("replace", 0));
        let last = run_and_kill("replace", &dir, &mut rng);
        let left = contents(&dir);
        assert_eq!(left.keys().collect::<Vec<_>>(), [NAME], "seed {seed}, kill {i}: {left:?}");
        let bytes = left[NAME].as_deref().unwrap();
        let on_disk = which("replace", bytes);
        assert!(
            on_disk.is_some_and(|n| n == last || n == last + 1),
            "seed {seed}, kill {i}: version {on_disk:?} on disk ({} bytes) after the child finished {last}",
            bytes.len()
        );
    }
}

#[test]
fn a_save_killed_at_any_moment_keeps_the_original_whole_under_exactly_one_name() {
    let root = tempfile::tempdir().unwrap();
    let mut rng = Rng::new();
    let seed = rng.0;
    for i in 0..kills() {
        let dir = fresh_folder(root.path(), i, &original());
        let last = run_and_kill("save", &dir, &mut rng);
        let photo = contents(&dir);
        let backups = dir.join(DIR_NAME);
        let backup = if backups.is_dir() { contents(&backups) } else { BTreeMap::new() };
        let at_photo = photo.get(NAME).cloned().flatten();
        let at_backup = backup.get(NAME).cloned().flatten();
        let case = format!(
            "seed {seed}, kill {i} after {last} Saves: photo {:?}, backup {:?}",
            at_photo.as_ref().map(Vec::len),
            at_backup.as_ref().map(Vec::len)
        );
        assert!(photo.keys().all(|k| k == NAME || k == DIR_NAME), "{case}: {:?}", photo.keys());
        assert!(backup.keys().all(|k| k == NAME), "{case}: {:?}", backup.keys());
        let original = original();
        match (at_photo, at_backup) {
            // Before a Save, or after its Undo.
            (Some(p), None) => assert_eq!(p, original, "{case}"),
            // Between the backup and the saved file.
            (None, Some(b)) => assert_eq!(b, original, "{case}"),
            // Saved: the original kept, the saved file whole (this Save's or the next one's).
            (Some(p), Some(b)) => {
                assert_eq!(b, original, "{case}");
                assert!(which("saved", &p).is_some_and(|n| n == last + 1), "{case}");
            }
            (None, None) => panic!("{case}: the original is gone"),
        }
    }
}
