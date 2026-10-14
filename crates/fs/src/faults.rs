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
