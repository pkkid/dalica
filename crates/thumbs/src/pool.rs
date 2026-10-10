//! The job pool (`architecture.md` §5): a fixed set of worker threads running submitted jobs,
//! highest priority first (first come, first served within a priority), at most one job per key.
//!
//! - Submitting a key that is already queued or running does not add work: a queued job keeps
//!   the higher of the two priorities, a running one simply delivers its result.
//! - Cancelling a key removes its queued job, or sets the running job's cancel flag; either way
//!   its result is never delivered.
//! - Results arrive on the pool's channel as `(key, output)`, read with [`Pool::try_recv`] or
//!   [`Pool::recv_timeout`] by the owner (the UI or the engine).

use std::collections::{BinaryHeap, HashMap};
use std::hash::Hash;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

/// Higher runs first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Priority(pub u8);

impl Priority {
    /// The edit view's main image.
    pub const EDIT: Priority = Priority(70);
    pub const HOVER: Priority = Priority(60);
    /// Thumbnails on screen.
    pub const VISIBLE: Priority = Priority(50);
    pub const FILMSTRIP: Priority = Priority(40);
    pub const EFFECT_TILES: Priority = Priority(30);
    /// The next and previous photo.
    pub const PREFETCH: Priority = Priority(20);
    /// Background thumbnail generation.
    pub const BACKGROUND: Priority = Priority(10);
}

/// The default number of workers: cores − 1, clamped to 2..=8.
pub fn default_workers() -> usize {
    std::thread::available_parallelism().map_or(2, |n| n.get().saturating_sub(1)).clamp(2, 8)
}

type Work<T> = Box<dyn FnOnce(&AtomicBool) -> T + Send>;

/// What happened to a submission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Submitted {
    /// Queued as a new job.
    Queued,
    /// The key was already queued: no new job; its priority is now the higher one.
    Merged,
    /// The key is running: no new job; its result will be delivered.
    Running,
}

struct Queued<K> {
    priority: Priority,
    seq: u64,
    key: K,
}

impl<K> PartialEq for Queued<K> {
    fn eq(&self, other: &Self) -> bool {
        (self.priority, self.seq) == (other.priority, other.seq)
    }
}
impl<K> Eq for Queued<K> {}
impl<K> PartialOrd for Queued<K> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<K> Ord for Queued<K> {
    /// Max-heap: higher priority first, then the earlier submission.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority.cmp(&other.priority).then_with(|| other.seq.cmp(&self.seq))
    }
}

struct Pending<T> {
    priority: Priority,
    seq: u64,
    work: Work<T>,
}

struct State<K, T> {
    /// May hold stale entries (cancelled, or re-queued at a higher priority); `pending` decides.
    heap: BinaryHeap<Queued<K>>,
    pending: HashMap<K, Pending<T>>,
    running: HashMap<K, Arc<AtomicBool>>,
    seq: u64,
    shutdown: bool,
}

struct Shared<K, T> {
    state: Mutex<State<K, T>>,
    wake: Condvar,
}

impl<K, T> Shared<K, T> {
    fn lock(&self) -> MutexGuard<'_, State<K, T>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A pool of worker threads. Dropping it cancels running jobs and joins the workers.
pub struct Pool<K, T> {
    shared: Arc<Shared<K, T>>,
    results: Receiver<(K, T)>,
    workers: Vec<JoinHandle<()>>,
}

impl<K, T> Pool<K, T>
where
    K: Eq + Hash + Clone + Send + 'static,
    T: Send + 'static,
{
    /// A pool with `workers` threads (at least one).
    pub fn new(workers: usize) -> Pool<K, T> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State { heap: BinaryHeap::new(), pending: HashMap::new(), running: HashMap::new(), seq: 0, shutdown: false }),
            wake: Condvar::new(),
        });
        let (tx, results) = mpsc::channel();
        let workers = (0..workers.max(1))
            .filter_map(|i| {
                let (shared, tx) = (Arc::clone(&shared), tx.clone());
                std::thread::Builder::new().name(format!("dalica-job-{i}")).spawn(move || worker(&shared, &tx)).ok()
            })
            .collect();
        Pool { shared, results, workers }
    }

    /// Submits `work` for `key` at `priority`. `work` gets a cancel flag it should check now and then.
    pub fn submit(&self, key: K, priority: Priority, work: impl FnOnce(&AtomicBool) -> T + Send + 'static) -> Submitted {
        let mut s = self.shared.lock();
        if s.running.contains_key(&key) {
            return Submitted::Running;
        }
        s.seq += 1;
        let seq = s.seq;
        if let Some(p) = s.pending.get_mut(&key) {
            if priority > p.priority {
                p.priority = priority;
                p.seq = seq;
                s.heap.push(Queued { priority, seq, key });
            }
            return Submitted::Merged;
        }
        s.pending.insert(key.clone(), Pending { priority, seq, work: Box::new(work) });
        s.heap.push(Queued { priority, seq, key });
        drop(s);
        self.shared.wake.notify_one();
        Submitted::Queued
    }

    /// Cancels `key`: a queued job is dropped, a running one is told to stop; neither delivers.
    /// Returns whether there was a job.
    pub fn cancel(&self, key: &K) -> bool {
        let mut s = self.shared.lock();
        if s.pending.remove(key).is_some() {
            return true;
        }
        match s.running.get(key) {
            Some(flag) => {
                flag.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    /// Jobs queued and not yet started.
    pub fn queued(&self) -> usize {
        self.shared.lock().pending.len()
    }

    /// A delivered result, if one is waiting.
    pub fn try_recv(&self) -> Option<(K, T)> {
        self.results.try_recv().ok()
    }

    /// Waits up to `timeout` for a delivered result.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<(K, T)> {
        self.results.recv_timeout(timeout).ok()
    }
}

fn worker<K: Eq + Hash + Clone, T>(shared: &Shared<K, T>, tx: &Sender<(K, T)>) {
    loop {
        let (key, work, cancel) = {
            let mut s = shared.lock();
            loop {
                if s.shutdown {
                    return;
                }
                match s.heap.pop() {
                    Some(q) => {
                        // Skip stale heap entries (cancelled, or superseded by a higher priority)
                        // without touching the key's current job.
                        let current = s.pending.get(&q.key).is_some_and(|p| p.seq == q.seq);
                        if let Some(p) = current.then(|| s.pending.remove(&q.key)).flatten() {
                            let cancel = Arc::new(AtomicBool::new(false));
                            s.running.insert(q.key.clone(), Arc::clone(&cancel));
                            break (q.key, p.work, cancel);
                        }
                    }
                    None => s = shared.wake.wait(s).unwrap_or_else(PoisonError::into_inner),
                }
            }
        };
        let output = work(&cancel);
        let mut s = shared.lock();
        s.running.remove(&key);
        // Deliver unless cancelled. The send happens under the lock, so a cancel either comes
        // before it (no delivery) or after it (nothing left to cancel).
        if !cancel.load(Ordering::SeqCst) {
            let _ = tx.send((key, output));
        }
    }
}

impl<K, T> Drop for Pool<K, T> {
    fn drop(&mut self) {
        {
            let mut s = self.shared.lock();
            s.shutdown = true;
            for flag in s.running.values() {
                flag.store(true, Ordering::SeqCst);
            }
        }
        self.shared.wake.notify_all();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
    }
}
