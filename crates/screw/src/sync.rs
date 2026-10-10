// SPDX-License-Identifier: EUPL-1.2

//! Shared lock policy.

use std::sync::{Mutex, MutexGuard};

/// Lock a mutex, recovering the guarded value if a holder panicked.
///
/// Every critical section in this crate finishes its mutation without a step
/// that can panic, so recovering a poisoned mutex never hands back a
/// half-updated value. Recovering keeps an unrelated thread's failure from
/// cascading into a second panic while a widget is rendering.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
