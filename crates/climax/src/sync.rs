// SPDX-License-Identifier: EUPL-1.2

//! Shared lock policy.

use std::sync::{
    Mutex,
    MutexGuard,
};

/// Lock a mutex, recovering the guarded value if a holder panicked.
///
/// Every mutex in this crate guards a value whose writers replace or append it
/// whole, so a panic elsewhere cannot leave one half-updated. Recovering keeps
/// an unrelated thread's failure from turning into either a second panic or a
/// spurious output error.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
