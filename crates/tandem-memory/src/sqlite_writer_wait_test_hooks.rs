//! Opt-in fixture witnesses for the exact SQLite connection used by a store.
//!
//! No observer is installed by default. A fixture observer is called only by
//! SQLite after a guarded transaction actually encounters a busy writer.

use super::MemoryDatabase;
use crate::store::{MemoryStoreError, MemoryStoreErrorKind, MemoryStoreResult};
use rusqlite::{ffi, Connection};
use std::ffi::c_void;
use std::sync::{Arc, Mutex};

/// The attempt number is SQLite's native busy counter. Return true to retry.
/// Fixture callbacks must use bounded waits and must not reenter this store.
pub type MemorySqliteWriterWaitObserver = Arc<dyn Fn(u32) -> bool + Send + Sync>;

pub(crate) type ObserverSlot = Arc<Mutex<Option<MemorySqliteWriterWaitObserver>>>;

/// Drop unregisters the observer for future transactions on this store.
/// An already-running transaction retains its callback until it finishes.
pub struct MemorySqliteWriterWaitGuard {
    slot: ObserverSlot,
}

impl Drop for MemorySqliteWriterWaitGuard {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.slot.lock() {
            *slot = None;
        }
    }
}

struct BusyCallbackState {
    observer: MemorySqliteWriterWaitObserver,
}

/// Holds the callback allocation until the connection restores its normal
/// timeout. The caller retains the connection mutex for this entire scope.
pub(crate) struct BusyCallbackScope {
    connection: *mut ffi::sqlite3,
    _state: Box<BusyCallbackState>,
}

impl Drop for BusyCallbackScope {
    fn drop(&mut self) {
        // SAFETY: the scope is dropped while the same connection mutex is held,
        // after its transaction has ended, and before its callback allocation.
        unsafe { ffi::sqlite3_busy_timeout(self.connection, 10_000) };
    }
}

unsafe extern "C" fn observe_busy_writer(context: *mut c_void, attempt: i32) -> i32 {
    // SAFETY: SQLite only invokes this while BusyCallbackScope owns the boxed
    // state and the caller holds the exact connection's exclusive mutex.
    let state = unsafe { &*(context.cast::<BusyCallbackState>()) };
    // A fixture panic must never unwind through the native SQLite callback.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        (state.observer)(attempt.max(0) as u32)
    }))
    .map(i32::from)
    .unwrap_or(0)
}

impl MemoryDatabase {
    pub fn observe_sqlite_writer_wait_for_test(
        &self,
        observer: MemorySqliteWriterWaitObserver,
    ) -> MemoryStoreResult<MemorySqliteWriterWaitGuard> {
        let mut slot = self.sqlite_writer_wait_observer.lock().map_err(|_| {
            MemoryStoreError::new(MemoryStoreErrorKind::Internal, "writer observer lock failed")
        })?;
        if slot.is_some() {
            return Err(MemoryStoreError::new(
                MemoryStoreErrorKind::Conflict,
                "this store already has a writer-wait observer",
            ));
        }
        *slot = Some(observer);
        Ok(MemorySqliteWriterWaitGuard {
            slot: self.sqlite_writer_wait_observer.clone(),
        })
    }

    pub(crate) fn install_sqlite_writer_wait_observer(
        &self,
        connection: &Connection,
    ) -> MemoryStoreResult<Option<BusyCallbackScope>> {
        let observer = self.sqlite_writer_wait_observer.lock().map_err(|_| {
            MemoryStoreError::new(MemoryStoreErrorKind::Internal, "writer observer lock failed")
        })?.clone();
        let Some(observer) = observer else { return Ok(None); };
        let mut state = Box::new(BusyCallbackState { observer });
        // SAFETY: this handle stays valid under the caller's connection mutex.
        let handle = unsafe { connection.handle() };
        let context = (&mut *state as *mut BusyCallbackState).cast::<c_void>();
        // SAFETY: the returned scope keeps userdata allocated and resets the
        // callback before freeing it. No other task can use this connection.
        let status = unsafe { ffi::sqlite3_busy_handler(handle, Some(observe_busy_writer), context) };
        if status != ffi::SQLITE_OK {
            unsafe { ffi::sqlite3_busy_timeout(handle, 10_000) };
            return Err(MemoryStoreError::new(
                MemoryStoreErrorKind::Internal,
                "could not install the SQLite writer-wait observer",
            ));
        }
        Ok(Some(BusyCallbackScope { connection: handle, _state: state }))
    }
}
