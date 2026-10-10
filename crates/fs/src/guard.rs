//! The UI-thread guard (`architecture.md` §5): the thread that owns the `Session` never touches the
//! disk. The app marks that thread at startup; in debug and test builds every public `fs` call made
//! from it returns [`Error::OnUiThread`], so a violation fails tests instead of stuttering in
//! production. Release builds only keep the mark.

use std::cell::Cell;
use std::marker::PhantomData;

use crate::{Error, Result};

thread_local! {
    static MARKED: Cell<bool> = const { Cell::new(false) };
    static REFUSED: Cell<u64> = const { Cell::new(0) };
}

/// Keeps the current thread marked as the UI thread until dropped. Not `Send`: it unmarks the
/// thread that created it.
#[derive(Debug)]
#[must_use = "the thread is unmarked when this guard is dropped"]
pub struct UiThread {
    _not_send: PhantomData<*const ()>,
}

/// Marks the current thread as the UI thread.
pub fn mark_ui_thread() -> UiThread {
    MARKED.with(|m| m.set(true));
    UiThread { _not_send: PhantomData }
}

impl Drop for UiThread {
    fn drop(&mut self) {
        MARKED.with(|m| m.set(false));
    }
}

/// Whether the current thread is marked as the UI thread.
pub fn is_ui_thread() -> bool {
    MARKED.with(Cell::get)
}

/// How many file operations this thread has had refused as the UI thread. A caller may swallow
/// the error; this count cannot be swallowed, so tests assert it stays 0.
pub fn refused() -> u64 {
    REFUSED.with(Cell::get)
}

/// The first line of every public file operation.
pub(crate) fn check() -> Result<()> {
    if cfg!(debug_assertions) && is_ui_thread() {
        REFUSED.with(|r| r.set(r.get().saturating_add(1)));
        Err(Error::OnUiThread)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marking_is_per_thread_and_scoped() {
        assert!(check().is_ok());
        let before = refused();
        {
            let _ui = mark_ui_thread();
            assert!(is_ui_thread());
            assert!(matches!(check(), Err(Error::OnUiThread)));
            // Another thread is unaffected.
            assert!(std::thread::spawn(|| check().is_ok()).join().unwrap());
        }
        assert!(!is_ui_thread());
        assert!(check().is_ok());
        assert_eq!(refused(), before + 1, "the one refused call is counted, the allowed ones are not");
    }
}
