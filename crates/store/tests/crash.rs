//! Real crashes and a full disk in the index and the file-operation journal (`architecture.md` §4,
//! `data-model.md` §8). A child process writes in a loop and is killed at a random moment
//! (`SIGKILL` on Unix, `TerminateProcess` on Windows), then this process opens what it left:
//!
//! - **The journal append** (`index.log`): every record before the kill is whole and in order, at
//!   most a torn last one is dropped, every op the child saw land is there, and the index opens
//!   without a rebuild.
//! - **The intents file** (`file-ops.json`, replaced whole on every write): it reads cleanly, as one
//!   of the states the child wrote, and nothing is set aside as damaged.
//!
//! Five kills each in a task's gate, 50 at full strength (`DALICA_FULL_TESTS=1`). This test binary
//! doubles as the helper (`DALICA_STORE_CRASH_HELPER=<mode>:<dir>`, the `crash::helper` test),
//! printing `HELPER:DONE <n>` after each step that returned. A full disk (`dalica-fs`'s
//! `arm_full`) fails a write with the reason the user reads and loses nothing written before it.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::common::{NOW, full};
use dalica_core::{PhotoId, Timestamp};
use dalica_fs::atomic::is_temp_name;
use dalica_fs::faults::{arm_full, disarm_all, disk_full, points};
use dalica_library::{Folder, Library, Op, Photo};
use dalica_store::intents::{self, Direction, Intent, Intents, Item, Kind, State};
use dalica_store::{Recovery, Store, journal};

const HELPER: &str = "DALICA_STORE_CRASH_HELPER";
/// How long a helper writes before it gives up on being killed.
const HELPER_LIMIT: Duration = Duration::from_secs(30);
/// Items per file operation in the intents loop.
const ITEMS: usize = 3;

fn kills() -> u32 {
    if full() { 50 } else { 5 }
}

/// The library every journal run starts from: one folder holding one photo.
fn setup_ops(root: &Path) -> (Vec<Op>, PhotoId) {
    let mut lib = Library::new();
    let folder = lib.next_folder_id();
    let add = Op::AddFolder { folder: Folder { id: folder, path: root.join("photos") } };
    lib.apply(add.clone()).unwrap();
    let id = lib.next_photo_id();
    let photo = Photo { id, folder, name: "IMG_0001.jpg".into(), size: 1, mtime: Timestamp::EPOCH, width: 4, height: 3, date: None, star: false };
    (vec![add, Op::AddPhotos { photos: vec![photo] }], id)
}

/// Caption `n`: about 2 KiB, so a record takes more than one small write.
fn caption(id: PhotoId, n: u64) -> Op {
    Op::SetCaptions { captions: vec![(id, format!("caption {n:08} {}", "x".repeat(2000)))] }
}

/// A file operation of `ITEMS` items, all pending, numbered `id`.
fn file_op(dir: &Path, id: u64) -> Intent {
    let items = (0..ITEMS)
        .map(|i| Item {
            from: dir.join("a").join(format!("IMG_{i}.jpg")),
            to: Some(dir.join("b").join(format!("IMG_{i}.jpg"))),
            state: State::Pending,
        })
        .collect();
    Intent { id, kind: Kind::Move, direction: Direction::Do, items }
}

#[test]
fn helper() {
    let Ok(spec) = std::env::var(HELPER) else { return };
    let (mode, dir) = spec.split_once(':').unwrap();
    let dir = Path::new(dir);
    let started = Instant::now();
    let result = match mode {
        "journal" => journal_loop(dir, started),
        "intents" => intents_loop(dir, started),
        other => Err(format!("unknown mode {other}")),
    };
    if let Err(e) = result {
        println!("HELPER:ERROR {e}");
    }
}

/// Appends caption 1, 2, … to the index until killed.
fn journal_loop(dir: &Path, started: Instant) -> Result<(), String> {
    let opened = Store::open(dir, NOW).map_err(|e| e.to_string())?;
    let mut store = opened.store;
    let (_, id) = setup_ops(dir);
    for n in 1.. {
        store.append(&caption(id, n)).map_err(|e| e.to_string())?;
        println!("HELPER:DONE {n}");
        if started.elapsed() > HELPER_LIMIT {
            break;
        }
    }
    Ok(())
}

/// Operation 1, 2, …: entered, each item done, ended; until killed.
fn intents_loop(dir: &Path, started: Instant) -> Result<(), String> {
    let mut journal = Intents::open(dir, NOW).map_err(|e| e.to_string())?.intents;
    for n in 1.. {
        journal.begin(file_op(dir, n)).map_err(|e| e.to_string())?;
        for i in 0..ITEMS {
            journal.item(n, i, State::Done).map_err(|e| e.to_string())?;
        }
        journal.end(n).map_err(|e| e.to_string())?;
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

/// The names in `dir` other than the temporary files a killed atomic write may leave.
fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> =
        fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name()).filter(|n| !is_temp_name(n)).map(|n| n.to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

#[test]
fn a_journal_append_killed_at_any_moment_keeps_every_op_that_landed() {
    let root = tempfile::tempdir().unwrap();
    let mut rng = Rng::new();
    let seed = rng.0;
    for i in 0..kills() {
        let dir = root.path().join(format!("run-{i}"));
        fs::create_dir(&dir).unwrap();
        let (setup, id) = setup_ops(&dir);
        {
            let mut store = Store::open(&dir, NOW).unwrap().store;
            for op in &setup {
                store.append(op).unwrap();
            }
        }
        let last = run_and_kill("journal", &dir, &mut rng);
        let case = format!("seed {seed}, kill {i} after {last} appends");

        // The records: whole and in order, at most a torn last one.
        let bytes = fs::read(dir.join(journal::FILE)).unwrap();
        let contents = journal::read(&bytes);
        assert!(contents.header.is_some(), "{case}");
        assert!(matches!(contents.stop, None | Some(journal::Stop::Torn)), "{case}: {:?}", contents.stop);
        let (head, captions) = contents.ops.split_at(setup.len().min(contents.ops.len()));
        assert_eq!(head, setup.as_slice(), "{case}");
        let landed = captions.len() as u64;
        assert!(landed == last || landed == last + 1, "{case}: {landed} captions on disk");
        for (n, op) in (1..).zip(captions) {
            assert_eq!(*op, caption(id, n), "{case}: record {n}");
        }

        // The index opens on them, dropping only a torn tail.
        let opened = Store::open(&dir, NOW).unwrap();
        assert!(opened.report.iter().all(|r| matches!(r, Recovery::JournalTail { .. })), "{case}: {:?}", opened.report);
        assert_eq!(opened.store.seq(), setup.len() as u64 + landed, "{case}");
        let want = if landed == 0 { String::new() } else { format!("caption {landed:08} {}", "x".repeat(2000)) };
        assert_eq!(opened.library.caption(id), want, "{case}");
    }
}

#[test]
fn an_intents_write_killed_at_any_moment_leaves_a_state_it_wrote() {
    let root = tempfile::tempdir().unwrap();
    let mut rng = Rng::new();
    let seed = rng.0;
    for i in 0..kills() {
        let dir = root.path().join(format!("run-{i}"));
        fs::create_dir(&dir).unwrap();
        let last = run_and_kill("intents", &dir, &mut rng);
        let case = format!("seed {seed}, kill {i} after {last} operations");
        assert_eq!(names(&dir), [intents::FILE], "{case}: nothing set aside");
        let opened = Intents::open(&dir, NOW).unwrap();
        assert!(opened.problems.is_empty(), "{case}: {:?}", opened.problems);
        let ops: Vec<&Intent> = opened.intents.ops().collect();
        match ops.as_slice() {
            // Between two operations: every ended one is gone.
            [] => {}
            // The operation in progress, as one of the states written for it.
            [op] => {
                assert_eq!(op.id, last + 1, "{case}");
                let done = op.items.iter().take_while(|it| it.state == State::Done).count();
                let mut want = file_op(&dir, last + 1);
                for item in want.items.iter_mut().take(done) {
                    item.state = State::Done;
                }
                assert_eq!(**op, want, "{case}");
            }
            more => panic!("{case}: {} operations", more.len()),
        }
    }
}

/// A full disk under the intents file: the operation is refused with the OS's reason, the file
/// keeps what it held, and the next write (space freed) goes through.
#[test]
fn a_full_disk_refuses_an_intent_with_the_reason_and_keeps_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let mut journal = Intents::open(d, NOW).unwrap().intents;
    journal.begin(file_op(d, 1)).unwrap();
    let before = fs::read(d.join(intents::FILE)).unwrap();
    arm_full(points::WRITE);
    let err = journal.begin(file_op(d, 2)).unwrap_err();
    disarm_all();
    assert!(matches!(&err, intents::Error::Fs(text) if text.ends_with(&disk_full().to_string())), "{err:?}");
    assert_eq!(fs::read(d.join(intents::FILE)).unwrap(), before);
    assert_eq!(names(d), [intents::FILE], "no temporary file left");
    assert!(journal.get(2).is_none());
    journal.begin(file_op(d, 2)).unwrap();
    assert_eq!(Intents::open(d, NOW).unwrap().intents.ops().count(), 2);
}

/// A full disk part-way through a journal append: the error names the reason, and the index
/// reopens with every op before it (the half-written record dropped).
#[test]
fn a_full_disk_during_an_append_names_the_reason_and_keeps_the_ops_before() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let (setup, id) = setup_ops(d);
    let mut store = Store::open(d, NOW).unwrap().store;
    for op in &setup {
        store.append(op).unwrap();
    }
    store.append(&caption(id, 1)).unwrap();
    arm_full(points::APPEND_WRITE);
    let err = store.append(&caption(id, 2)).unwrap_err();
    disarm_all();
    let fs_err = match &err {
        dalica_store::Error::Fs(dalica_fs::Error::Io { source, .. }) => source,
        other => panic!("{other:?}"),
    };
    assert_eq!(fs_err.kind(), std::io::ErrorKind::StorageFull);
    assert!(err.to_string().ends_with(&disk_full().to_string()), "{err}");
    drop(store);
    let opened = Store::open(d, NOW).unwrap();
    assert!(matches!(opened.report.as_slice(), [Recovery::JournalTail { kept: 3, .. }]), "{:?}", opened.report);
    assert_eq!(opened.library.caption(id), format!("caption {:08} {}", 1, "x".repeat(2000)));
}
