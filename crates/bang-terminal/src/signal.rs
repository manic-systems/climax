// SPDX-License-Identifier: EUPL-1.2

use std::{fmt, io, os::fd::OwnedFd, sync::OnceLock};

use rustix::{
    io::{Errno, read},
    pipe::{PipeFlags, pipe_with},
};

use crate::cleanup::{CleanupFailures, CleanupStage, staged};

/// One of the terminal signals a [`SignalGuard`] catches.
///
/// Bang keeps its own type so the crate that delivers the signal number is not
/// part of its public API.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Signal(i32);

impl Signal {
    /// `SIGINT`.
    pub const INT: Self = Self(libc::SIGINT);
    /// `SIGTERM`.
    pub const TERM: Self = Self(libc::SIGTERM);
    /// `SIGHUP`.
    pub const HUP: Self = Self(libc::SIGHUP);
    /// `SIGQUIT`.
    pub const QUIT: Self = Self(libc::SIGQUIT);

    /// The platform's signal number.
    #[must_use]
    pub const fn as_raw(self) -> i32 {
        self.0
    }

    /// The conventional name, such as `SIGTERM`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self.0 {
            libc::SIGINT => "SIGINT",
            libc::SIGTERM => "SIGTERM",
            libc::SIGHUP => "SIGHUP",
            _ => "SIGQUIT",
        }
    }

    fn from_raw(raw: i32) -> Option<Self> {
        TERMINAL_SIGNALS.into_iter().find(|signal| signal.0 == raw)
    }
}

impl fmt::Display for Signal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

const TERMINAL_SIGNALS: [Signal; 4] = [Signal::INT, Signal::TERM, Signal::HUP, Signal::QUIT];

/// Both ends stay open for the life of the process, so a handler running on
/// another thread can never write into a descriptor number that was closed and
/// reused.
static PIPE: OnceLock<Pipe> = OnceLock::new();

#[derive(Debug)]
struct Pipe {
    read_fd: OwnedFd,
    _write_fd: OwnedFd,
}

fn pipe() -> io::Result<&'static Pipe> {
    if let Some(pipe) = PIPE.get() {
        return Ok(pipe);
    }
    let (read_fd, write_fd) = pipe_with(PipeFlags::CLOEXEC | PipeFlags::NONBLOCK)?;
    Ok(PIPE.get_or_init(|| {
        handlers::publish(&write_fd);
        Pipe {
            read_fd,
            _write_fd: write_fd,
        }
    }))
}

/// Reads the signals a [`SignalGuard`] has queued.
///
/// It is a copyable view of the one process-wide queue, so an event source can
/// poll while the guard that owns the handlers stays with the caller. Every
/// poller reads the same queue and each signal goes to whichever poll reaches
/// it first. Installing a guard discards what an earlier one left queued, and
/// a poller kept past its guard reads signals queued for the next. Nothing is
/// queued while no guard is installed.
#[derive(Clone, Copy, Debug)]
pub struct SignalPoller {
    read_fd: &'static OwnedFd,
}

impl SignalPoller {
    /// Take the next queued signal without blocking.
    pub fn poll_signal(&self) -> io::Result<Option<Signal>> {
        let mut record = [0_u8; 2];
        loop {
            match read(self.read_fd, &mut record) {
                Ok(0) | Err(Errno::AGAIN) => return Ok(None),
                Ok(2) => {
                    if record[0] == handlers::generation()
                        && let Some(signal) = Signal::from_raw(i32::from(record[1]))
                    {
                        return Ok(Some(signal));
                    }
                },
                Ok(_) | Err(Errno::INTR) => {},
                Err(errno) => return Err(errno.into()),
            }
        }
    }
}

/// Catches `SIGINT`, `SIGTERM`, `SIGHUP` and `SIGQUIT` and queues them for polling.
///
/// A signal the process was ignoring when the guard was installed stays
/// ignored and is never queued. Any other previous handler is replaced while
/// the guard is installed and is never chained to. The previous handlers come back on
/// [`SignalGuard::restore`] or on drop.
#[derive(Debug)]
pub struct SignalGuard {
    poller: SignalPoller,
    previous: Vec<(Signal, handlers::Previous)>,
    active: bool,
}

impl SignalGuard {
    /// Install handlers for the conventional terminal signals.
    ///
    /// Only one guard can be active in a process at a time.
    pub fn install_terminal_handlers() -> io::Result<Self> {
        handlers::claim()?;
        let installed = install_terminal_handlers_exclusive();
        if installed.is_err() {
            handlers::release();
        }
        installed
    }

    /// A view of the signal queue that does not borrow this guard.
    #[must_use]
    pub const fn poller(&self) -> SignalPoller {
        self.poller
    }

    /// Take the next queued signal without blocking.
    pub fn poll_signal(&mut self) -> io::Result<Option<Signal>> {
        self.poller.poll_signal()
    }

    /// Put the previous handlers back and report any failure.
    pub fn restore(mut self) -> Result<(), CleanupFailures> {
        self.restore_active().0
    }

    /// Put the previous handlers back and take one signal that was still
    /// queued.
    ///
    /// The queue is read before another guard can be installed, so the signal
    /// is always one this guard owned the handlers for.
    pub fn restore_polling(mut self) -> (Result<(), CleanupFailures>, Option<Signal>) {
        self.restore_active()
    }

    fn restore_active(&mut self) -> (Result<(), CleanupFailures>, Option<Signal>) {
        if !self.active {
            return (Ok(()), None);
        }
        let failures = self
            .previous
            .iter()
            .filter_map(|(signal, previous)| handlers::restore(*signal, previous).err())
            .collect();
        let late = self.poller.poll_signal().ok().flatten();
        handlers::release();
        self.active = false;
        (staged(CleanupStage::Signals, failures), late)
    }
}

impl Drop for SignalGuard {
    fn drop(&mut self) {
        let _result = self.restore_active();
    }
}

fn install_terminal_handlers_exclusive() -> io::Result<SignalGuard> {
    let poller = SignalPoller {
        read_fd: &pipe()?.read_fd,
    };
    handlers::next_generation();
    while poller.poll_signal()?.is_some() {}

    let mut previous = Vec::new();
    for signal in TERMINAL_SIGNALS {
        match handlers::install(signal) {
            Ok(Some(old)) => previous.push((signal, old)),
            Ok(None) => {},
            Err(error) => {
                for (signal, old) in &previous {
                    let _result = handlers::restore(*signal, old);
                }
                return Err(error);
            },
        }
    }

    Ok(SignalGuard {
        poller,
        previous,
        active: true,
    })
}

/// The raw `sigaction` calls rustix does not cover, kept in one place.
mod handlers {
    use std::{
        io,
        os::fd::{AsRawFd as _, BorrowedFd, OwnedFd},
        sync::atomic::{AtomicBool, AtomicI32, AtomicU8, Ordering},
    };

    use super::Signal;

    pub(super) type Previous = libc::sigaction;

    static WRITE_FD: AtomicI32 = AtomicI32::new(-1);
    static ACTIVE: AtomicBool = AtomicBool::new(false);
    static GENERATION: AtomicU8 = AtomicU8::new(0);

    pub(super) fn generation() -> u8 {
        GENERATION.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    pub(super) fn write_fd() -> i32 {
        WRITE_FD.load(Ordering::SeqCst)
    }

    pub(super) fn next_generation() {
        GENERATION.fetch_add(1, Ordering::SeqCst);
    }

    pub(super) fn claim() -> io::Result<()> {
        ACTIVE
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map(drop)
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "terminal signal handlers are already installed",
                )
            })
    }

    pub(super) fn release() {
        ACTIVE.store(false, Ordering::SeqCst);
    }

    pub(super) fn publish(write_fd: &OwnedFd) {
        WRITE_FD.store(write_fd.as_raw_fd(), Ordering::SeqCst);
    }

    /// Install the handler and return the disposition it replaced, or `None`
    /// when the signal was ignored and has been left that way.
    pub(super) fn install(signal: Signal) -> io::Result<Option<Previous>> {
        // SAFETY: zeroed storage is written by sigaction before it is read
        let mut previous = unsafe { std::mem::zeroed::<libc::sigaction>() };
        // SAFETY: a null new action only queries, and previous is valid for the call
        if unsafe { libc::sigaction(signal.as_raw(), std::ptr::null(), &raw mut previous) } != 0 {
            return Err(io::Error::last_os_error());
        }
        if previous.sa_sigaction == libc::SIG_IGN {
            return Ok(None);
        }

        // SAFETY: a zeroed sigaction is a valid starting point and is fully set below
        let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
        action.sa_sigaction = handle as *const () as usize;
        action.sa_flags = 0;
        // SAFETY: sa_mask points to valid memory
        if unsafe { libc::sigemptyset(&raw mut action.sa_mask) } != 0 {
            return Err(io::Error::last_os_error());
        }

        // SAFETY: both pointers are valid for the call
        if unsafe { libc::sigaction(signal.as_raw(), &raw const action, &raw mut previous) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Some(previous))
    }

    pub(super) fn restore(signal: Signal, previous: &Previous) -> io::Result<()> {
        // SAFETY: previous was returned by sigaction for this signal
        if unsafe { libc::sigaction(signal.as_raw(), previous, std::ptr::null_mut()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn errno_slot() -> *mut libc::c_int {
        #[cfg(target_os = "linux")]
        // SAFETY: the errno location is always valid for the calling thread
        return unsafe { libc::__errno_location() };
        #[cfg(any(target_os = "android", target_os = "openbsd", target_os = "netbsd"))]
        // SAFETY: the errno location is always valid for the calling thread
        return unsafe { libc::__errno() };
        #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
        // SAFETY: the errno location is always valid for the calling thread
        return unsafe { libc::__error() };
        #[cfg(target_os = "dragonfly")]
        // SAFETY: the errno location is always valid for the calling thread
        return unsafe { libc::__errno_location() };
        #[cfg(any(target_os = "solaris", target_os = "illumos"))]
        // SAFETY: the errno location is always valid for the calling thread
        return unsafe { libc::___errno() };
        #[cfg(target_os = "haiku")]
        // SAFETY: the errno location is always valid for the calling thread
        return unsafe { libc::_errnop() };
        #[cfg(not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "openbsd",
            target_os = "netbsd",
            target_os = "dragonfly",
            target_os = "solaris",
            target_os = "illumos",
            target_os = "haiku",
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd"
        )))]
        return std::ptr::null_mut();
    }

    extern "C" fn handle(signal: libc::c_int) {
        let slot = errno_slot();
        // SAFETY: a non-null slot is the interrupted thread's errno
        let saved = if slot.is_null() { 0 } else { unsafe { *slot } };
        let generation = GENERATION.load(Ordering::SeqCst);
        let fd = WRITE_FD.load(Ordering::SeqCst);
        if fd >= 0 {
            let byte = u8::try_from(signal).unwrap_or(0);
            // SAFETY: the pipe is never closed once published
            let write_fd = unsafe { BorrowedFd::borrow_raw(fd) };
            let _result = rustix::io::write(write_fd, &[generation, byte]);
        }
        if !slot.is_null() {
            // SAFETY: as above
            unsafe { *slot = saved };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raise(signal: libc::c_int) {
        // SAFETY: raising a signal that has a handler or is ignored
        assert_eq!(unsafe { libc::raise(signal) }, 0);
    }

    #[test]
    fn signals_carry_their_number_and_name() {
        assert_eq!(Signal::TERM.as_raw(), libc::SIGTERM);
        assert_eq!(Signal::QUIT.to_string(), "SIGQUIT");
        assert_eq!(Signal::from_raw(libc::SIGHUP), Some(Signal::HUP));
        assert_eq!(Signal::from_raw(libc::SIGUSR1), None);
    }

    #[test]
    fn terminal_signal_handlers_are_a_process_singleton() {
        let _serial = crate::testing::SIGNAL_LOCK.lock().unwrap();
        let guard = SignalGuard::install_terminal_handlers().unwrap();
        let error = SignalGuard::install_terminal_handlers().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        drop(guard);

        SignalGuard::install_terminal_handlers()
            .unwrap()
            .restore()
            .unwrap();
    }

    #[test]
    fn successive_guards_share_one_queue() {
        let _serial = crate::testing::SIGNAL_LOCK.lock().unwrap();
        for signal in [libc::SIGTERM, libc::SIGHUP] {
            let mut guard = SignalGuard::install_terminal_handlers().unwrap();
            raise(signal);
            assert_eq!(
                guard.poll_signal().unwrap().map(Signal::as_raw),
                Some(signal)
            );
            assert_eq!(guard.poll_signal().unwrap(), None);
            guard.restore().unwrap();
        }
    }

    #[test]
    fn a_stale_handler_write_never_reaches_a_later_guard() {
        let _serial = crate::testing::SIGNAL_LOCK.lock().unwrap();
        let first = SignalGuard::install_terminal_handlers().unwrap();
        let stale = handlers::generation();
        first.restore().unwrap();

        let mut second = SignalGuard::install_terminal_handlers().unwrap();
        let fd = handlers::write_fd();
        // SAFETY: the write end is never closed once published
        let write_fd = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
        rustix::io::write(write_fd, &[stale, u8::try_from(libc::SIGTERM).unwrap()]).unwrap();
        assert_eq!(second.poll_signal().unwrap(), None);

        raise(libc::SIGHUP);
        assert_eq!(second.poll_signal().unwrap(), Some(Signal::HUP));
        second.restore().unwrap();
    }

    #[test]
    fn restore_polling_returns_a_signal_queued_before_release() {
        let _serial = crate::testing::SIGNAL_LOCK.lock().unwrap();
        let guard = SignalGuard::install_terminal_handlers().unwrap();
        raise(libc::SIGTERM);
        let (result, late) = guard.restore_polling();
        result.unwrap();
        assert_eq!(late, Some(Signal::TERM));
    }

    #[test]
    fn an_ignored_signal_stays_ignored_and_is_not_queued() {
        let _serial = crate::testing::SIGNAL_LOCK.lock().unwrap();
        // SAFETY: SIG_IGN is a valid disposition
        let before = unsafe { libc::signal(libc::SIGHUP, libc::SIG_IGN) };
        let mut guard = SignalGuard::install_terminal_handlers().unwrap();

        raise(libc::SIGHUP);
        assert_eq!(guard.poll_signal().unwrap(), None);
        guard.restore().unwrap();

        // SAFETY: restoring the disposition read above
        let after = unsafe { libc::signal(libc::SIGHUP, before) };
        assert_eq!(after, libc::SIG_IGN);
    }
}
