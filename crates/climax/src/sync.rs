// SPDX-License-Identifier: EUPL-1.2

//! sync: shared lock policy

use std::sync::{
    Mutex,
    MutexGuard,
    PoisonError,
};

/// locks a mutex, recovering the guarded value if a holder panicked
///
/// every guarded value is replaced or appended whole, so a poisoned lock is
/// never half-updated
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
