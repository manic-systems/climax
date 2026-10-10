// SPDX-License-Identifier: EUPL-1.2

//! shared lock policy

use std::sync::{
    Mutex,
    MutexGuard,
    PoisonError,
};

/// locks a mutex, recovering the guarded value if a holder panicked
///
/// every critical section in this crate finishes its mutation without a step
/// that can panic, so a poisoned mutex never holds a half-updated value
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
