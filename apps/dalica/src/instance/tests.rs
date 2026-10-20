//! The hand-off between two launches in one process (adapted from PhotoCraft
//! `apps/photocraft/src/single_instance.rs` @ d95fdf31, its tests). The two-process tests are
//! `tests/instance.rs`.

use super::*;
use std::sync::mpsc::channel;

const FAST: Timing = Timing { handoff: Duration::from_millis(1500), ack: Duration::from_millis(500), request: Duration::from_millis(800) };

fn abs(name: &str) -> PathBuf {
    std::env::temp_dir().join(name)
}

/// Launches with `paths` on another thread (it waits until the first Dalica's UI takes them).
fn launch(dir: &Path, paths: &[PathBuf], timing: Timing) -> std::thread::JoinHandle<Claim> {
    let (dir, paths) = (dir.to_path_buf(), paths.to_vec());
    std::thread::spawn(move || claim_with(&dir, &paths, timing))
}

/// Plays the UI: takes until `n` launches arrived (or 5 s passed).
fn drain(ui: &Handoffs, n: usize) -> Vec<Vec<PathBuf>> {
    let mut got = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    while got.len() < n && Instant::now() < deadline {
        got.extend(ui.take());
        std::thread::sleep(Duration::from_millis(10));
    }
    got
}

fn primary(dir: &Path) -> Primary {
    match claim_with(dir, &[], FAST) {
        Claim::Primary(p) => p,
        _ => panic!("the first launch is the primary"),
    }
}

#[test]
fn a_second_launch_hands_its_paths_to_the_first() {
    let dir = tempfile::tempdir().unwrap();
    let primary = primary(dir.path());
    let ui = primary.connect(None);
    let paths = vec![abs("one.jpg"), abs("two é.png")];
    let second = launch(dir.path(), &paths, TIMING);
    assert_eq!(drain(&ui, 1), vec![paths]);
    assert!(matches!(second.join().unwrap(), Claim::Forwarded));
    // A plain second start (no files) only brings the window forward.
    let third = launch(dir.path(), &[], TIMING);
    assert_eq!(drain(&ui, 1), vec![Vec::<PathBuf>::new()]);
    assert!(matches!(third.join().unwrap(), Claim::Forwarded));
}

#[test]
fn the_wake_is_called_when_paths_arrive() {
    let dir = tempfile::tempdir().unwrap();
    let primary = primary(dir.path());
    let woken = Arc::new(AtomicUsize::new(0));
    let w = Arc::clone(&woken);
    let ui = primary.connect(Some(Arc::new(move || {
        w.fetch_add(1, Ordering::SeqCst);
    })));
    let second = launch(dir.path(), &[abs("a.jpg")], TIMING);
    assert_eq!(drain(&ui, 1).len(), 1);
    assert!(matches!(second.join().unwrap(), Claim::Forwarded));
    assert!(woken.load(Ordering::SeqCst) >= 1);
}

#[test]
fn simultaneous_launches_all_land_in_one_dalica() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = channel();
    for i in 0..6 {
        let (dir, tx) = (dir.path().to_path_buf(), tx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(claim(&dir, &[abs(&format!("file{i}.jpg"))]));
        });
    }
    drop(tx);
    let (mut primaries, mut forwarded, mut taken) = (Vec::new(), 0, 0);
    let mut ui: Option<Handoffs> = None;
    let deadline = Instant::now() + Duration::from_secs(10);
    while primaries.len() + forwarded < 6 && Instant::now() < deadline {
        while let Ok(c) = rx.try_recv() {
            match c {
                Claim::Primary(p) => primaries.push(p),
                Claim::Forwarded => forwarded += 1,
                Claim::Standalone(reason) => panic!("standalone: {reason}"),
            }
        }
        if ui.is_none()
            && let Some(p) = primaries.first()
        {
            ui = Some(p.connect(None));
        }
        if let Some(ui) = &ui {
            taken += ui.take().len();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!((primaries.len(), forwarded, taken), (1, 5, 5));
}

#[test]
fn a_crashed_dalica_leaves_nothing_in_the_way() {
    let dir = tempfile::tempdir().unwrap();
    // The info file of a process that is gone; the lock is free.
    let stale = format!(r#"{{"port":1,"token":"{}","pid":1}}"#, "0".repeat(64));
    dalica_fs::replace(&dir.path().join(INFO_FILE), stale.as_bytes(), dalica_fs::Expect::Absent).unwrap();
    let first = primary(dir.path());
    assert!(wire::read_info(dir.path()).is_some_and(|(port, _)| port != 1));
    drop(first);
    // Released with the handle, as on process exit.
    assert!(matches!(claim_with(dir.path(), &[], FAST), Claim::Primary(_)));
}

/// A launch waiting on a Dalica that shuts down is not told `ok`: it takes the lock over and opens
/// the file itself.
#[test]
fn a_launch_waiting_on_a_closing_dalica_takes_over() {
    let dir = tempfile::tempdir().unwrap();
    let first = primary(dir.path());
    // The UI never takes (it is closing).
    let waiting = launch(dir.path(), &[abs("a.jpg")], FAST);
    std::thread::sleep(Duration::from_millis(200));
    first.shutdown();
    assert!(matches!(waiting.join().unwrap(), Claim::Primary(_)), "the waiting launch becomes the primary");
    // After shutdown new connections are refused too.
    let (port, token) = wire::read_info(dir.path()).unwrap();
    drop(first);
    let request = wire::Request::new(&[abs("b.jpg")]).unwrap();
    assert!(send(port, &token, &request, Instant::now() + Duration::from_millis(300)).is_err());
}

/// A Dalica whose UI never takes the paths (hung) does not acknowledge them, and does not keep them
/// to open later as well.
#[test]
fn a_hung_dalica_never_swallows_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let first = primary(dir.path());
    let ui = first.connect(None);
    let started = Instant::now();
    assert!(matches!(claim_with(dir.path(), &[abs("a.jpg")], FAST), Claim::Standalone(_)));
    assert!(started.elapsed() < FAST.handoff + Duration::from_secs(1), "{:?}", started.elapsed());
    assert!(ui.take().is_empty(), "withdrawn, not opened later");
}

#[test]
fn a_stalled_client_does_not_hold_up_the_next_launch() {
    let dir = tempfile::tempdir().unwrap();
    let first = primary(dir.path());
    let ui = first.connect(None);
    let (port, _) = wire::read_info(dir.path()).unwrap();
    // Connect and never send a line.
    let stalled: Vec<TcpStream> = (0..3).map(|_| TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap()).collect();
    let started = Instant::now();
    let next = launch(dir.path(), &[abs("b.jpg")], FAST);
    assert_eq!(drain(&ui, 1), vec![vec![abs("b.jpg")]]);
    assert!(matches!(next.join().unwrap(), Claim::Forwarded));
    assert!(started.elapsed() < FAST.request, "took {:?}", started.elapsed());
    drop(stalled);
}

/// A client that trickles a byte at a time cannot keep its slot past the request's deadline.
#[test]
fn a_trickling_client_is_cut_off_at_the_request_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let _first = primary(dir.path());
    let (port, _) = wire::read_info(dir.path()).unwrap();
    let mut slow = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    let started = Instant::now();
    let mut cut_off = false;
    while started.elapsed() < FAST.request * 4 {
        if slow.write_all(b"{").is_err() {
            cut_off = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(cut_off, "still connected after {:?}", started.elapsed());
    assert!(started.elapsed() < FAST.request * 3, "{:?}", started.elapsed());
}

#[test]
fn an_unreachable_dalica_falls_back_to_a_launch_of_its_own() {
    let dir = tempfile::tempdir().unwrap();
    // Someone holds the lock but nothing listens on the advertised port.
    let held = open_lock(dir.path()).unwrap();
    held.lock().unwrap();
    let closed = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port();
    wire::write_info(dir.path(), closed, &wire::Secret::generate().unwrap()).unwrap();
    assert!(matches!(claim_with(dir.path(), &[abs("a.jpg")], FAST), Claim::Standalone(_)));
}

#[test]
fn a_lock_file_that_is_not_a_plain_file_is_a_launch_of_its_own() {
    let dir = tempfile::tempdir().unwrap();
    dalica_fs::create_dir_all(&dir.path().join(LOCK_FILE)).unwrap();
    assert!(matches!(claim_with(dir.path(), &[], FAST), Claim::Standalone(_)));
}

/// Who takes part: the per-user settings folder, with files the request can carry.
#[test]
fn only_per_user_launches_with_files_that_fit_take_part() {
    assert_eq!(skip_reason(true, &[]), None);
    assert_eq!(skip_reason(true, &[abs("a.jpg")]), None);
    assert!(skip_reason(false, &[]).is_some(), "--library, --demo, DALICA_HOME or portable");
    assert!(skip_reason(true, &[PathBuf::from("relative.jpg")]).is_some());
    assert!(skip_reason(true, &[abs(&"x".repeat(MAX_PATH_BYTES))]).is_some());
    assert!(skip_reason(true, &vec![abs("a.jpg"); MAX_PATHS + 1]).is_some());
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        let odd = std::env::temp_dir().join(std::ffi::OsString::from_vec(b"\xff.jpg".to_vec()));
        assert!(skip_reason(true, &[odd]).is_some());
    }
}
