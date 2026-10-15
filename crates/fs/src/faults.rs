//! Named fault points (`architecture.md` §11): every step of a write that can fail on a real disk
//! has a name, and tests built with the `faults` feature arm one to fail on its next use.
//!
//! - [`Fault::Error`]: the step returns an I/O error and the operation cleans up as it would after a
//!   real failure (removes its temporary file).
//! - [`Fault::Crash`]: the operation stops right there without cleaning up, as if the process had
//!   been killed, and returns [`crate::Error::Crashed`]. A crash during a write first writes half
//!   the bytes, so a torn temporary file is left behind.
//!
//! A full disk is an error of its own: [`arm_full`] (or [`arm_full_global`]) arms a point to fail
//! as [`Fault::Error`] does, cleanup included, but with the error the OS gives when the volume is
//! full ([`disk_full`]: `ENOSPC` on Unix, `ERROR_DISK_FULL` on Windows, both
//! [`std::io::ErrorKind::StorageFull`]), so a test sees the reason the user would read. It is not a
//! third [`Fault`] variant: every step matches the two kinds of failure, and a full disk is the
//! first kind with a different message.
//!
//! A fault fires once. [`arm`] applies to the calling thread only, so parallel tests do not see
//! each other's faults; [`arm_global`] applies to whichever thread reaches the point first, for
//! tests whose writes happen on a worker thread. Without the feature, [`hit`] is a constant `None`.
//!
//! [`run_at`] plants a change instead of a failure: the hook runs when the calling thread next
//! reaches the point, just before the step, so a test can swap a folder for a link at the exact
//! moment between validation and write (`security.md` §5.6).
//!
//! **Test builds skip the slow, lasting effects of real I/O.** With the feature on in a debug build
//! (every `cargo test`; never a release build, a bench or a budget, which keep the product's I/O):
//!
//! - **No real `fsync`.** Every `sync_all`/`sync_data` goes through [`fsync`], which skips the
//!   system call unless the calling thread turned real syncing on with [`real_syncs`]. A simulated
//!   [`Fault::Crash`] never drops the page cache, so a real sync adds no coverage, only time (a
//!   pack or journal test on Windows or macOS spends most of its time there). The fault points at
//!   the sync steps are still reached and an armed error still fails them. One `fs` test turns
//!   real syncing on and checks every kind of sync works and comes in its order (the temporary
//!   file, the rename, the folder), recorded by [`synced`].
//! - **A stand-in trash** (`trash` module): `trash::send` and `trash::restore` use a folder of this
//!   process under the system temporary folder instead of the OS trash, which the tests would
//!   otherwise fill (and which costs seconds per item through the Windows shell). A test opts back
//!   in to the OS trash with [`real_trash`].

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

    /// Opening (or making) `<folder>/.dalica-originals/` (`originals::back_up`).
    pub const BACKUP_OPEN_DIR: &str = "originals.open_dir";
    /// Re-checking the photo's identity right before it moves into the backup.
    pub const BACKUP_CHECK: &str = "originals.check";
    /// Moving the photo into the backup (one rename that never replaces).
    pub const BACKUP_MOVE: &str = "originals.move";
    /// `fsync` of the backup folder and the photo's folder after the move.
    pub const BACKUP_FSYNC_DIR: &str = "originals.fsync_dir";
    /// Checking that the backup is the photo that moved.
    pub const BACKUP_VERIFY: &str = "originals.verify";
    /// Re-checking the saved file and the backup right before the backup moves back
    /// (`originals::restore`).
    pub const RESTORE_CHECK: &str = "originals.restore_check";
    /// Moving the backup back over the saved file.
    pub const RESTORE_MOVE: &str = "originals.restore_move";
    /// `fsync` of both folders after moving the backup back.
    pub const RESTORE_FSYNC_DIR: &str = "originals.restore_fsync_dir";
    /// Checking that the photo is the backup that moved back.
    pub const RESTORE_VERIFY: &str = "originals.restore_verify";

    /// Creating the temporary file of a new file (`create::new_file`).
    pub const NEW_TEMP: &str = "create.temp";
    /// Writing the new file's bytes into the temporary file.
    pub const NEW_WRITE: &str = "create.write";
    /// `fsync` of the temporary file.
    pub const NEW_FSYNC: &str = "create.fsync";
    /// Giving the temporary file the first free name, never replacing a file.
    pub const NEW_COMMIT: &str = "create.commit";
    /// Armed as an error, the volume answers that it can neither rename without replacing nor
    /// make hard links, so the new file is written under its name by exclusive creation instead
    /// (armed as a crash, the operation stops there).
    pub const NEW_NO_LINKS: &str = "create.no_links";
    /// Writing the bytes into the exclusively created file (volumes without links).
    pub const NEW_DIRECT_WRITE: &str = "create.direct_write";
    /// `fsync` of the exclusively created file (volumes without links).
    pub const NEW_DIRECT_FSYNC: &str = "create.direct_fsync";
    /// `fsync` of the folder after the new file got its name.
    pub const NEW_FSYNC_DIR: &str = "create.fsync_dir";
    /// Re-reading the new file to verify it.
    pub const NEW_VERIFY: &str = "create.verify";
    /// Making a new folder under the first free name (`create::new_folder`).
    pub const NEW_MKDIR: &str = "create.mkdir";
    /// `fsync` of the parent after making the new folder.
    pub const NEW_MKDIR_FSYNC: &str = "create.mkdir_fsync";

    /// Opening the source and destination folders of a move, a rename or a copy (`transfer`).
    pub const MOVE_OPEN_DIR: &str = "transfer.open_dir";
    /// Re-checking the source's identity right before it moves or is copied.
    pub const MOVE_CHECK: &str = "transfer.check";
    /// Armed as an error, the two folders answer that they are on different volumes, so a move
    /// takes the cross-volume path (copy, verify, then the source to the trash); armed as a crash,
    /// the operation stops there.
    pub const MOVE_CROSS_VOLUME: &str = "transfer.cross_volume";
    /// The rename that never replaces a file (one volume).
    pub const MOVE_RENAME: &str = "transfer.rename";
    /// `fsync` of both folders after the rename.
    pub const MOVE_FSYNC_DIR: &str = "transfer.fsync_dir";
    /// Checking that the file under the new name is the one that moved (or the verified copy).
    pub const MOVE_VERIFY: &str = "transfer.verify";

    /// Creating the copy's temporary file in the destination folder.
    pub const COPY_TEMP: &str = "transfer.copy_temp";
    /// Writing the source's bytes into the temporary file.
    pub const COPY_WRITE: &str = "transfer.copy_write";
    /// `fsync` of the temporary file.
    pub const COPY_FSYNC: &str = "transfer.copy_fsync";
    /// Armed as an error, one byte of the copy (of a file, or of the first file of a tree) is
    /// flipped on disk before it is verified, which the verification must catch; armed as a crash,
    /// the operation stops there.
    pub const COPY_FLIP: &str = "transfer.copy_flip";
    /// Comparing the copy byte for byte with the source.
    pub const COPY_VERIFY: &str = "transfer.copy_verify";
    /// Giving the verified copy its name, never replacing a file.
    pub const COPY_COMMIT: &str = "transfer.copy_commit";
    /// `fsync` of the destination folder after the copy got its name.
    pub const COPY_FSYNC_DIR: &str = "transfer.copy_fsync_dir";

    /// Re-checking, before the source of a cross-volume move goes to the trash, that the source is
    /// unchanged and its verified copy is still in place.
    pub const FINISH_CHECK: &str = "transfer.finish_check";

    /// Making a folder whose name must be free (`transfer::new_folder`).
    pub const FOLDER_MKDIR: &str = "transfer.mkdir";
    /// `fsync` of the parent after making it.
    pub const FOLDER_MKDIR_FSYNC: &str = "transfer.mkdir_fsync";
    /// Re-checking a folder's identity right before it is renamed, moved or copied.
    pub const FOLDER_CHECK: &str = "transfer.folder_check";
    /// Renaming or moving a folder in one step that never replaces one (one volume).
    pub const FOLDER_RENAME: &str = "transfer.folder_rename";
    /// `fsync` of both parents after the folder moved.
    pub const FOLDER_FSYNC_DIR: &str = "transfer.folder_fsync_dir";

    /// Making the temporary folder a cross-volume folder move copies the tree into, or a folder
    /// of the tree inside it.
    pub const TREE_MKDIR: &str = "transfer.tree_mkdir";
    /// Writing one file of the tree into the copy.
    pub const TREE_WRITE: &str = "transfer.tree_write";
    /// `fsync` of one copied file.
    pub const TREE_FSYNC: &str = "transfer.tree_fsync";
    /// Comparing one copied file byte for byte with its source.
    pub const TREE_VERIFY: &str = "transfer.tree_verify";
    /// Giving the verified copy of the tree its name, never replacing a folder.
    pub const TREE_COMMIT: &str = "transfer.tree_commit";

    /// Armed as an error, the volume answers that it has no trash (`trash::send` returns
    /// `Sent::NoTrash` and the item stays); armed as a crash, the operation stops there.
    pub const TRASH_NO_TRASH: &str = "trash.no_trash";
    /// Handing the item to the OS trash.
    pub const TRASH_SEND: &str = "trash.send";
    /// Moving items back out of the OS trash.
    pub const TRASH_RESTORE: &str = "trash.restore";
}

/// The atomic-replace points, in order.
pub const ATOMIC_POINTS: [&str; 7] =
    [points::OPEN_DIR, points::CREATE_TEMP, points::WRITE, points::FSYNC, points::RENAME, points::FSYNC_DIR, points::VERIFY];

/// The append and rotate points.
pub const APPEND_POINTS: [&str; 3] = [points::APPEND_WRITE, points::APPEND_FSYNC, points::ROTATE];

/// The stale-temporary cleanup points.
pub const STALE_POINTS: [&str; 1] = [points::STALE_REMOVE];

/// The points of moving a photo into its originals backup, in order.
pub const BACKUP_POINTS: [&str; 5] =
    [points::BACKUP_OPEN_DIR, points::BACKUP_CHECK, points::BACKUP_MOVE, points::BACKUP_FSYNC_DIR, points::BACKUP_VERIFY];

/// The points of moving a backup back over the saved file, in order.
pub const RESTORE_POINTS: [&str; 4] = [points::RESTORE_CHECK, points::RESTORE_MOVE, points::RESTORE_FSYNC_DIR, points::RESTORE_VERIFY];

/// The points of writing a new file, in order, on a volume with links.
pub const NEW_FILE_POINTS: [&str; 6] =
    [points::NEW_TEMP, points::NEW_WRITE, points::NEW_FSYNC, points::NEW_COMMIT, points::NEW_FSYNC_DIR, points::NEW_VERIFY];

/// The points a new file reaches, in order, once [`points::NEW_NO_LINKS`] armed as an error sent
/// it to exclusive creation.
pub const NEW_FILE_DIRECT_POINTS: [&str; 4] = [points::NEW_DIRECT_WRITE, points::NEW_DIRECT_FSYNC, points::NEW_FSYNC_DIR, points::NEW_VERIFY];

/// The points of making a new folder, in order.
pub const NEW_FOLDER_POINTS: [&str; 2] = [points::NEW_MKDIR, points::NEW_MKDIR_FSYNC];

/// The points of moving or renaming a file on one volume, in order.
pub const MOVE_POINTS: [&str; 5] = [points::MOVE_OPEN_DIR, points::MOVE_CHECK, points::MOVE_RENAME, points::MOVE_FSYNC_DIR, points::MOVE_VERIFY];

/// The points of copying a file (the first phase of a cross-volume move, after
/// [`points::MOVE_CROSS_VOLUME`] armed as an error), in order.
pub const COPY_POINTS: [&str; 9] = [
    points::MOVE_OPEN_DIR,
    points::MOVE_CHECK,
    points::COPY_TEMP,
    points::COPY_WRITE,
    points::COPY_FSYNC,
    points::COPY_VERIFY,
    points::COPY_COMMIT,
    points::COPY_FSYNC_DIR,
    points::MOVE_VERIFY,
];

/// The points a cross-volume move reaches after [`COPY_POINTS`]: the second phase, in order.
pub const FINISH_POINTS: [&str; 2] = [points::FINISH_CHECK, points::TRASH_SEND];

/// The points of making a folder whose name must be free, in order.
pub const FOLDER_NEW_POINTS: [&str; 2] = [points::FOLDER_MKDIR, points::FOLDER_MKDIR_FSYNC];

/// The points of renaming or moving a folder on one volume, in order.
pub const FOLDER_MOVE_POINTS: [&str; 4] = [points::MOVE_OPEN_DIR, points::FOLDER_CHECK, points::FOLDER_RENAME, points::FOLDER_FSYNC_DIR];

/// The points of moving a folder across volumes (after [`points::MOVE_CROSS_VOLUME`] armed as an
/// error), in order: the tree copied and verified file by file, then the source tree to the trash.
pub const TREE_POINTS: [&str; 9] = [
    points::MOVE_OPEN_DIR,
    points::FOLDER_CHECK,
    points::TREE_MKDIR,
    points::TREE_WRITE,
    points::TREE_FSYNC,
    points::TREE_VERIFY,
    points::TREE_COMMIT,
    points::FINISH_CHECK,
    points::TRASH_SEND,
];

/// The points of sending an item to the OS trash, and of bringing items back.
pub const TRASH_POINTS: [&str; 2] = [points::TRASH_SEND, points::TRASH_RESTORE];

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

    /// What a point is armed with: one of the two kinds of failure, or a full disk (an error whose
    /// reason is the OS's own, [`super::disk_full`]).
    #[derive(Clone, Copy)]
    pub enum Armed {
        Fault(Fault),
        Full,
    }

    thread_local! {
        static LOCAL: RefCell<Vec<(&'static str, Armed)>> = const { RefCell::new(Vec::new()) };
        static HOOKS: RefCell<Vec<(&'static str, Hook)>> = const { RefCell::new(Vec::new()) };
        /// The point whose last firing on this thread was a full disk, until `injected` makes the
        /// error for it (it fires as `Fault::Error`, so the step cleans up as after any error).
        static FULL: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
    }
    static GLOBAL: Mutex<Vec<(&'static str, Armed)>> = Mutex::new(Vec::new());

    pub fn arm(point: &'static str, armed: Armed) {
        LOCAL.with(|l| l.borrow_mut().push((point, armed)));
    }

    pub fn arm_global(point: &'static str, armed: Armed) {
        GLOBAL.lock().unwrap_or_else(PoisonError::into_inner).push((point, armed));
    }

    /// Whether `point` just fired as a full disk on this thread (once).
    pub fn full_at(point: &str) -> bool {
        FULL.with(|f| match f.get() {
            Some(p) if p == point => {
                f.set(None);
                true
            }
            _ => false,
        })
    }

    pub fn run_at(point: &'static str, hook: Hook) {
        HOOKS.with(|h| h.borrow_mut().push((point, hook)));
    }

    // A thread that turned real syncing on (`super::real_syncs`), with what it synced since; and a
    // thread that opted in to the OS trash (`super::real_trash`).
    thread_local! {
        static SYNCS: RefCell<Option<Vec<std::path::PathBuf>>> = const { RefCell::new(None) };
        static REAL_TRASH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    pub fn real_syncs(on: bool) {
        SYNCS.with(|s| *s.borrow_mut() = on.then(Vec::new));
    }

    pub fn synced() -> Vec<std::path::PathBuf> {
        SYNCS.with(|s| s.borrow().clone().unwrap_or_default())
    }

    /// Whether the calling thread syncs for real, noting `what` when it does.
    pub fn sync_now(what: &std::path::Path) -> bool {
        SYNCS.with(|s| s.borrow_mut().as_mut().map(|list| list.push(what.to_path_buf())).is_some())
    }

    pub fn real_trash(on: bool) {
        REAL_TRASH.with(|t| t.set(on));
    }

    pub fn uses_real_trash() -> bool {
        REAL_TRASH.with(std::cell::Cell::get)
    }

    pub fn disarm_all() {
        LOCAL.with(|l| l.borrow_mut().clear());
        HOOKS.with(|h| h.borrow_mut().clear());
        GLOBAL.lock().unwrap_or_else(PoisonError::into_inner).clear();
    }

    fn take(list: &mut Vec<(&'static str, Armed)>, point: &str) -> Option<Armed> {
        let i = list.iter().position(|(p, _)| *p == point)?;
        Some(list.remove(i).1)
    }

    pub fn hit(point: &'static str) -> Option<Fault> {
        // Taken out first and run after the borrow ends, so a hook may itself reach fault points.
        let hook = HOOKS.with(|h| {
            let mut h = h.borrow_mut();
            let i = h.iter().position(|(p, _)| *p == point)?;
            Some(h.remove(i).1)
        });
        if let Some(hook) = hook {
            hook();
        }
        let armed =
            LOCAL.with(|l| take(&mut l.borrow_mut(), point)).or_else(|| take(&mut GLOBAL.lock().unwrap_or_else(PoisonError::into_inner), point))?;
        Some(match armed {
            Armed::Fault(fault) => {
                FULL.with(|f| f.set(None));
                fault
            }
            Armed::Full => {
                FULL.with(|f| f.set(Some(point)));
                Fault::Error
            }
        })
    }
}

/// Arms `point` to fail with `fault` the next time the calling thread reaches it.
#[cfg(feature = "faults")]
pub fn arm(point: &'static str, fault: Fault) {
    armed::arm(point, armed::Armed::Fault(fault));
}

/// Arms `point` to fail with `fault` the next time any thread reaches it.
#[cfg(feature = "faults")]
pub fn arm_global(point: &'static str, fault: Fault) {
    armed::arm_global(point, armed::Armed::Fault(fault));
}

/// Arms `point` to fail as a full disk the next time the calling thread reaches it: the step fails
/// and cleans up as for [`Fault::Error`], with the OS's full-disk error ([`disk_full`]).
#[cfg(feature = "faults")]
pub fn arm_full(point: &'static str) {
    armed::arm(point, armed::Armed::Full);
}

/// Arms `point` to fail as a full disk the next time any thread reaches it ([`arm_full`]).
#[cfg(feature = "faults")]
pub fn arm_full_global(point: &'static str) {
    armed::arm_global(point, armed::Armed::Full);
}

/// The error the OS gives when a write finds the volume full, as std reads it from the OS:
/// `ENOSPC` on Unix, `ERROR_DISK_FULL` on Windows; [`std::io::ErrorKind::StorageFull`] either way.
/// What a point armed with [`arm_full`] fails with.
#[cfg(feature = "faults")]
pub fn disk_full() -> std::io::Error {
    #[cfg(unix)]
    {
        std::io::Error::from_raw_os_error(rustix::io::Errno::NOSPC.raw_os_error())
    }
    #[cfg(windows)]
    {
        /// `ERROR_DISK_FULL` (winerror.h), which std reads as `StorageFull`.
        const ERROR_DISK_FULL: i32 = 112;
        std::io::Error::from_raw_os_error(ERROR_DISK_FULL)
    }
    #[cfg(not(any(unix, windows)))]
    {
        std::io::ErrorKind::StorageFull.into()
    }
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

/// Turns real `fsync` on (or back off) for the calling thread, and starts a new record of what it
/// syncs ([`synced`]). Test builds skip real syncs by default (the module docs); this is for the
/// test that checks real syncing works.
#[cfg(feature = "faults")]
pub fn real_syncs(on: bool) {
    armed::real_syncs(on);
}

/// What the calling thread synced for real since it called [`real_syncs`]`(true)`, in order: each
/// file or folder synced. (An optimised build always syncs; it records only while turned on.)
#[cfg(feature = "faults")]
pub fn synced() -> Vec<std::path::PathBuf> {
    armed::synced()
}

/// Sends the calling thread's `trash::send` and `trash::restore` to the OS trash (`true`) or back
/// to this process's stand-in trash (`false`, the default in test builds; the module docs). An
/// optimised build always uses the OS trash.
#[cfg(feature = "faults")]
pub fn real_trash(on: bool) {
    armed::real_trash(on);
}

/// Makes `what` durable through `sync` (its `sync_all` or `sync_data`). In a test build (the
/// feature on, a debug build) the system call is skipped unless the calling thread turned real
/// syncing on ([`real_syncs`]); everywhere else it always runs.
#[inline]
pub(crate) fn fsync(what: &std::path::Path, sync: impl FnOnce() -> std::io::Result<()>) -> std::io::Result<()> {
    #[cfg(feature = "faults")]
    if !armed::sync_now(what) && cfg!(debug_assertions) {
        return Ok(());
    }
    #[cfg(not(feature = "faults"))]
    let _ = what;
    sync()
}

/// Whether `trash::send` and `trash::restore` use this process's stand-in trash: a test build whose
/// calling thread has not opted in to the OS trash ([`real_trash`]).
#[inline]
pub(crate) fn stand_in_trash() -> bool {
    #[cfg(feature = "faults")]
    {
        cfg!(debug_assertions) && !armed::uses_real_trash()
    }
    #[cfg(not(feature = "faults"))]
    {
        false
    }
}

/// The I/O error an armed [`Fault::Error`] returns: [`disk_full`] when the point was armed with
/// [`arm_full`].
pub(crate) fn injected(point: &'static str) -> std::io::Error {
    #[cfg(feature = "faults")]
    if armed::full_at(point) {
        return disk_full();
    }
    std::io::Error::other(format!("injected fault at {point}"))
}

/// The fault armed at `point` as the error it stands for (the step `op` on `path` failing, or a
/// crash), or `Ok` when nothing is armed. For steps whose caller has nothing to clean up.
pub(crate) fn at(point: &'static str, op: &'static str, path: &std::path::Path) -> crate::Result<()> {
    match hit(point) {
        Some(Fault::Error) => Err(crate::io_err(op, path)(injected(point))),
        Some(Fault::Crash) => Err(crate::Error::Crashed(point)),
        None => Ok(()),
    }
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

    /// A full disk fires once, as an error, and only the error made for its own point is the
    /// OS's full-disk error.
    #[test]
    fn a_full_disk_fires_once_as_the_oss_own_error() {
        arm_full(points::WRITE);
        assert_eq!(hit(points::WRITE), Some(Fault::Error));
        assert_eq!(injected(points::RENAME).kind(), std::io::ErrorKind::Other);
        let e = injected(points::WRITE);
        assert_eq!(e.kind(), std::io::ErrorKind::StorageFull);
        assert_eq!(e.to_string(), disk_full().to_string());
        assert!(e.raw_os_error().is_some() || cfg!(not(any(unix, windows))), "{e:?}");
        // Used up: the next error made for that point is an ordinary injected one.
        assert_eq!(injected(points::WRITE).kind(), std::io::ErrorKind::Other);
        assert_eq!(hit(points::WRITE), None);
        // An ordinary error that fires after a full disk at the same point is ordinary.
        arm_full(points::RENAME);
        arm(points::RENAME, Fault::Error);
        assert_eq!(hit(points::RENAME), Some(Fault::Error));
        assert_eq!(hit(points::RENAME), Some(Fault::Error));
        assert_eq!(injected(points::RENAME).kind(), std::io::ErrorKind::Other);
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
