//! Named fault points (`architecture.md` §11): every step of a write that can fail on a real disk
//! has a name, and tests built with the `faults` feature arm one to fail on its next use.
//!
//! - [`Fault::Error`]: the step returns an I/O error and the operation cleans up as it would after a
//!   real failure (removes its temporary file).
//! - [`Fault::Crash`]: the operation stops right there without cleaning up, as if the process had
//!   been killed, and returns [`crate::Error::Crashed`]. A crash during a write first writes half
//!   the bytes, so a torn temporary file is left behind.
//!
//! A fault fires once. [`arm`] applies to the calling thread only, so parallel tests do not see
//! each other's faults; [`arm_global`] applies to whichever thread reaches the point first, for
//! tests whose writes happen on a worker thread. Without the feature, [`hit`] is a constant `None`.
//!
//! [`run_at`] plants a change instead of a failure: the hook runs when the calling thread next
//! reaches the point, just before the step, so a test can swap a folder for a link at the exact
//! moment between validation and write (`security.md` §5.6).

/// The fault points, in the order an atomic replace or an append reaches them.
pub mod points {
    /// Opening the target's folder once, after it was validated (the handle every later step of an
    /// atomic replace works relative to on Unix).
    pub const OPEN_DIR: &str = "atomic.open_dir";
    /// Creating the temporary file next to the target.
    pub const CREATE_TEMP: &str = "atomic.create_temp";
    /// Writing the new bytes into the temporary file.
    pub const WRITE: &str = "atomic.write";
    /// `fsync` of the temporary file.
    pub const FSYNC: &str = "atomic.fsync";
    /// Renaming the temporary file over the target.
    pub const RENAME: &str = "atomic.rename";
    /// `fsync` of the folder after the rename (Unix).
    pub const FSYNC_DIR: &str = "atomic.fsync_dir";
    /// Re-reading the target to verify it.
    pub const VERIFY: &str = "atomic.verify";
    /// Writing a record to an append-only file.
    pub const APPEND_WRITE: &str = "append.write";
    /// `fsync` after an append.
    pub const APPEND_FSYNC: &str = "append.fsync";
    /// One rename while rotating log files.
    pub const ROTATE: &str = "append.rotate";
    /// Removing one stale temporary file (`stale::remove_stale_temps`).
    pub const STALE_REMOVE: &str = "stale.remove";
}

/// The atomic-replace points, in order.
pub const ATOMIC_POINTS: [&str; 7] =
    [points::OPEN_DIR, points::CREATE_TEMP, points::WRITE, points::FSYNC, points::RENAME, points::FSYNC_DIR, points::VERIFY];

/// The append and rotate points.
pub const APPEND_POINTS: [&str; 3] = [points::APPEND_WRITE, points::APPEND_FSYNC, points::ROTATE];

/// The stale-temporary cleanup points.
pub const STALE_POINTS: [&str; 1] = [points::STALE_REMOVE];

/// How an armed fault point fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// The step returns an I/O error; the operation cleans up.
    Error,
    /// The operation stops without cleaning up, like a killed process.
    Crash,
}

#[cfg(feature = "faults")]
mod armed {
    use std::cell::RefCell;
    use std::sync::{Mutex, PoisonError};

    use super::Fault;

    /// A change a test plants at a point ([`super::run_at`]).
    type Hook = Box<dyn FnOnce()>;

    thread_local! {
        static LOCAL: RefCell<Vec<(&'static str, Fault)>> = const { RefCell::new(Vec::new()) };
        static HOOKS: RefCell<Vec<(&'static str, Hook)>> = const { RefCell::new(Vec::new()) };
    }
    static GLOBAL: Mutex<Vec<(&'static str, Fault)>> = Mutex::new(Vec::new());

    pub fn arm(point: &'static str, fault: Fault) {
        LOCAL.with(|l| l.borrow_mut().push((point, fault)));
    }

    pub fn arm_global(point: &'static str, fault: Fault) {
        GLOBAL.lock().unwrap_or_else(PoisonError::into_inner).push((point, fault));
    }

    pub fn run_at(point: &'static str, hook: Hook) {
        HOOKS.with(|h| h.borrow_mut().push((point, hook)));
    }

    pub fn disarm_all() {
        LOCAL.with(|l| l.borrow_mut().clear());
        HOOKS.with(|h| h.borrow_mut().clear());
        GLOBAL.lock().unwrap_or_else(PoisonError::into_inner).clear();
    }

    fn take(list: &mut Vec<(&'static str, Fault)>, point: &str) -> Option<Fault> {
        let i = list.iter().position(|(p, _)| *p == point)?;
        Some(list.remove(i).1)
    }

    pub fn hit(point: &str) -> Option<Fault> {
        // Taken out first and run after the borrow ends, so a hook may itself reach fault points.
        let hook = HOOKS.with(|h| {
            let mut h = h.borrow_mut();
            let i = h.iter().position(|(p, _)| *p == point)?;
            Some(h.remove(i).1)
        });
        if let Some(hook) = hook {
            hook();
        }
        LOCAL.with(|l| take(&mut l.borrow_mut(), point)).or_else(|| take(&mut GLOBAL.lock().unwrap_or_else(PoisonError::into_inner), point))
    }
}

/// Arms `point` to fail with `fault` the next time the calling thread reaches it.
#[cfg(feature = "faults")]
pub fn arm(point: &'static str, fault: Fault) {
    armed::arm(point, fault);
}

/// Arms `point` to fail with `fault` the next time any thread reaches it.
#[cfg(feature = "faults")]
pub fn arm_global(point: &'static str, fault: Fault) {
    armed::arm_global(point, fault);
}

/// Runs `hook` when the calling thread next reaches `point`, just before that step (once). Tests use
/// it to change the disk at an exact moment, such as swapping the target's folder for a link.
#[cfg(feature = "faults")]
pub fn run_at(point: &'static str, hook: impl FnOnce() + 'static) {
    armed::run_at(point, Box::new(hook));
}

/// Disarms the calling thread's faults and hooks, and every global fault.
#[cfg(feature = "faults")]
pub fn disarm_all() {
    armed::disarm_all();
}

/// The fault armed for `point`, consumed; `None` when nothing is armed or the feature is off.
#[cfg(feature = "faults")]
pub(crate) fn hit(point: &'static str) -> Option<Fault> {
    armed::hit(point)
}

#[cfg(not(feature = "faults"))]
#[inline(always)]
pub(crate) fn hit(_point: &'static str) -> Option<Fault> {
    None
}

/// The I/O error an armed [`Fault::Error`] returns.
pub(crate) fn injected(point: &'static str) -> std::io::Error {
    std::io::Error::other(format!("injected fault at {point}"))
}

#[cfg(all(test, feature = "faults"))]
mod tests {
    use super::*;

    #[test]
    fn a_fault_fires_once_on_its_own_thread() {
        arm(points::RENAME, Fault::Error);
        assert_eq!(hit(points::WRITE), None);
        assert!(std::thread::spawn(|| hit(points::RENAME)).join().unwrap().is_none());
        assert_eq!(hit(points::RENAME), Some(Fault::Error));
        assert_eq!(hit(points::RENAME), None);
    }

    #[test]
    fn disarm_clears_local_faults() {
        arm(points::FSYNC, Fault::Crash);
        disarm_all();
        assert_eq!(hit(points::FSYNC), None);
    }

    #[test]
    fn a_hook_runs_once_before_its_point_on_its_own_thread() {
        use std::cell::Cell;
        use std::rc::Rc;
        let runs = Rc::new(Cell::new(0));
        let r = Rc::clone(&runs);
        run_at(points::OPEN_DIR, move || r.set(r.get() + 1));
        arm(points::OPEN_DIR, Fault::Error);
        assert!(std::thread::spawn(|| hit(points::OPEN_DIR)).join().unwrap().is_none());
        assert_eq!(hit(points::WRITE), None);
        assert_eq!(runs.get(), 0);
        // The hook runs, then the armed fault still fires.
        assert_eq!(hit(points::OPEN_DIR), Some(Fault::Error));
        assert_eq!(runs.get(), 1);
        assert_eq!(hit(points::OPEN_DIR), None);
        assert_eq!(runs.get(), 1);
        let r = Rc::clone(&runs);
        run_at(points::RENAME, move || r.set(r.get() + 10));
        disarm_all();
        assert_eq!(hit(points::RENAME), None);
        assert_eq!(runs.get(), 1);
    }
}
