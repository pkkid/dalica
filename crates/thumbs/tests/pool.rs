//! The job pool with fake jobs (`architecture.md` §11): strict priority order under contention, one
//! execution per duplicate key, cancelled jobs never delivered, no lost results under stress.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dalica_thumbs::{Pool, Priority, Submitted};

const WAIT: Duration = Duration::from_secs(10);

/// Occupies one worker until the returned sender is dropped or sent to. Returns once it runs.
fn block(pool: &Pool<u64, u64>, key: u64) -> mpsc::Sender<()> {
    let (release, wait) = mpsc::channel::<()>();
    let (started_tx, started) = mpsc::channel();
    pool.submit(key, Priority(255), move |_| {
        let _ = started_tx.send(());
        let _ = wait.recv();
        key
    });
    started.recv_timeout(WAIT).expect("the blocking job did not start");
    release
}

fn drain(pool: &Pool<u64, u64>, n: usize) -> Vec<u64> {
    (0..n).map(|_| pool.recv_timeout(WAIT).expect("a result went missing").0).collect()
}

/// SplitMix64, for reproducible "random" priorities.
fn rng(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *seed;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

#[test]
fn queued_jobs_run_in_strict_priority_order() {
    let pool: Pool<u64, u64> = Pool::new(1);
    let gate = block(&pool, u64::MAX);
    let mut seed = 1;
    let mut expected: Vec<(Priority, u64)> = Vec::new();
    for key in 0..300u64 {
        let p = Priority((rng(&mut seed) % 8) as u8 * 10);
        assert_eq!(pool.submit(key, p, move |_| key), Submitted::Queued);
        expected.push((p, key));
    }
    // Highest priority first; within a priority, first submitted first.
    expected.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    drop(gate);
    let order = drain(&pool, 301);
    assert_eq!(order[0], u64::MAX);
    assert_eq!(&order[1..], expected.iter().map(|e| e.1).collect::<Vec<_>>());
    assert!(pool.recv_timeout(Duration::from_millis(50)).is_none());
}

#[test]
fn a_duplicate_key_runs_once() {
    let pool: Pool<u64, u64> = Pool::new(1);
    let gate = block(&pool, u64::MAX);
    let runs = Arc::new(AtomicUsize::new(0));
    for p in [10, 5, 60, 20] {
        let r = Arc::clone(&runs);
        let first = p == 10;
        let s = pool.submit(7, Priority(p), move |_| {
            r.fetch_add(1, Ordering::SeqCst);
            7
        });
        assert_eq!(s, if first { Submitted::Queued } else { Submitted::Merged });
    }
    // Key 7 was raised to priority 60, so it overtakes this priority-50 job.
    pool.submit(8, Priority(50), |_| 8);
    drop(gate);
    assert_eq!(drain(&pool, 3), [u64::MAX, 7, 8]);
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    // A running key is not started again; its one result is delivered.
    let gate = block(&pool, 9);
    assert_eq!(pool.submit(9, Priority(1), |_| 99), Submitted::Running);
    drop(gate);
    assert_eq!(drain(&pool, 1), [9]);
    assert!(pool.recv_timeout(Duration::from_millis(50)).is_none());
}

#[test]
fn cancelled_jobs_are_never_delivered() {
    let pool: Pool<u64, u64> = Pool::new(2);
    let gates = [block(&pool, u64::MAX), block(&pool, u64::MAX - 1)];
    for key in 0..100u64 {
        pool.submit(key, Priority::VISIBLE, move |_| key);
    }
    for key in (0..100u64).step_by(2) {
        assert!(pool.cancel(&key));
    }
    assert!(!pool.cancel(&1000));
    drop(gates);
    let got: HashSet<u64> = drain(&pool, 52).into_iter().collect();
    let want: HashSet<u64> = (1..100).step_by(2).chain([u64::MAX, u64::MAX - 1]).collect();
    assert_eq!(got, want);
    // A running job told to stop is not delivered either, and the pool goes on.
    let (started_tx, started) = mpsc::channel();
    pool.submit(500, Priority::EDIT, move |cancel: &AtomicBool| {
        let _ = started_tx.send(());
        while !cancel.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(1));
        }
        500
    });
    started.recv_timeout(WAIT).unwrap();
    assert!(pool.cancel(&500));
    pool.submit(501, Priority::EDIT, |_| 501);
    assert_eq!(drain(&pool, 1), [501]);
    assert!(pool.recv_timeout(Duration::from_millis(50)).is_none());
}

/// 10 000 submissions over 3 000 keys on 4 workers, with cancellations: every key that was not
/// cancelled is delivered exactly once, and no cancelled key is delivered.
#[test]
fn ten_thousand_jobs_lose_nothing() {
    let pool: Pool<u64, u64> = Pool::new(4);
    let gates: Vec<_> = (0..4).map(|i| block(&pool, u64::MAX - i)).collect();
    let runs = Arc::new(Mutex::new(HashMap::<u64, usize>::new()));
    let mut seed = 42;
    let mut live = HashSet::new();
    for _ in 0..10_000 {
        let key = rng(&mut seed) % 3000;
        let r = Arc::clone(&runs);
        pool.submit(key, Priority((rng(&mut seed) % 7) as u8 * 10), move |_| {
            *r.lock().unwrap().entry(key).or_default() += 1;
            key
        });
        live.insert(key);
        if rng(&mut seed).is_multiple_of(10) {
            let victim = rng(&mut seed) % 3000;
            if pool.cancel(&victim) {
                live.remove(&victim);
            }
        }
    }
    drop(gates);
    let mut got = drain(&pool, live.len() + 4);
    got.retain(|k| *k < 3000);
    assert!(pool.recv_timeout(Duration::from_millis(100)).is_none(), "an extra result arrived");
    let unique: HashSet<u64> = got.iter().copied().collect();
    assert_eq!(unique.len(), got.len(), "a key was delivered twice");
    assert_eq!(unique, live);
    assert!(runs.lock().unwrap().values().all(|&n| n == 1));
    assert_eq!(pool.queued(), 0);
}

#[test]
fn dropping_the_pool_stops_running_jobs() {
    let pool: Pool<u64, u64> = Pool::new(2);
    let (started_tx, started) = mpsc::channel();
    pool.submit(1, Priority::BACKGROUND, move |cancel: &AtomicBool| {
        let _ = started_tx.send(());
        while !cancel.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(1));
        }
        1
    });
    started.recv_timeout(WAIT).unwrap();
    drop(pool); // joins: returns only because the job saw its cancel flag
}
