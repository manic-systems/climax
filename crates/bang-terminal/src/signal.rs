// SPDX-License-Identifier: EUPL-1.2

use std::io;
#[cfg(unix)]
use std::sync::atomic::{
    AtomicI32,
    Ordering,
};

#[cfg(unix)]
static SIGNAL_WRITE_FD: AtomicI32 = AtomicI32::new(-1);

#[cfg(unix)]
pub const TERMINAL_SIGNALS: &[libc::c_int] =
    &[libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT];

/// convert signals into events
#[derive(Debug)]
pub struct SignalGuard {
    #[cfg(unix)]
    read_fd:  libc::c_int,
    #[cfg(unix)]
    write_fd: libc::c_int,
    #[cfg(unix)]
    previous: Vec<(libc::c_int, libc::sigaction)>,
}

impl SignalGuard {
    pub fn install_terminal_handlers() -> io::Result<Self> {
        install_terminal_handlers()
    }

    pub fn poll_signal(&mut self) -> io::Result<Option<i32>> {
        poll_signal(self)
    }
}

impl Drop for SignalGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            for (signal, previous) in &self.previous {
                // SAFETY: previous was returned by sigaction for this signal
                let _result = unsafe { libc::sigaction(*signal, previous, std::ptr::null_mut()) };
            }
            SIGNAL_WRITE_FD.store(-1, Ordering::SeqCst);
            // SAFETY: both fds are owned by guard
            unsafe {
                libc::close(self.read_fd);
                libc::close(self.write_fd);
            }
        }
    }
}

#[cfg(unix)]
fn install_terminal_handlers() -> io::Result<SignalGuard> {
    let (read_fd, write_fd) = pipe()?;
    set_nonblocking(read_fd)?;
    set_nonblocking(write_fd)?;
    set_cloexec(read_fd)?;
    set_cloexec(write_fd)?;

    SIGNAL_WRITE_FD.store(write_fd, Ordering::SeqCst);

    let mut previous = Vec::new();
    for signal in TERMINAL_SIGNALS {
        match install_handler(*signal) {
            Ok(old) => previous.push((*signal, old)),
            Err(error) => {
                for (signal, old) in &previous {
                    // SAFETY: old was returned by sigaction
                    let _result = unsafe { libc::sigaction(*signal, old, std::ptr::null_mut()) };
                }
                SIGNAL_WRITE_FD.store(-1, Ordering::SeqCst);
                // SAFETY: both fds are owned locally
                unsafe {
                    libc::close(read_fd);
                    libc::close(write_fd);
                }
                return Err(error);
            },
        }
    }

    Ok(SignalGuard {
        read_fd,
        write_fd,
        previous,
    })
}

#[cfg(unix)]
fn poll_signal(guard: &mut SignalGuard) -> io::Result<Option<i32>> {
    let mut buffer = [0_u8; 32];
    let mut first = None;

    loop {
        // SAFETY: buffer is valid, read_fd is owned by guard
        let read = unsafe {
            libc::read(
                guard.read_fd,
                buffer.as_mut_ptr().cast::<libc::c_void>(),
                buffer.len(),
            )
        };
        if read > 0 {
            if first.is_none() {
                first = buffer[..usize::try_from(read).expect("positive read fits usize")]
                    .iter()
                    .copied()
                    .find(|value| *value != 0)
                    .map(i32::from);
            }
            continue;
        }
        if read == 0 {
            return Ok(first);
        }

        let error = io::Error::last_os_error();
        match error.raw_os_error() {
            Some(code) if code == libc::EAGAIN || code == libc::EWOULDBLOCK => return Ok(first),
            Some(libc::EINTR) => {},
            _ => return Err(error),
        }
    }
}

#[cfg(unix)]
fn pipe() -> io::Result<(libc::c_int, libc::c_int)> {
    let mut fds = [0; 2];
    // SAFETY: fds are valid
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(fds.into())
}

#[cfg(unix)]
fn set_nonblocking(fd: libc::c_int) -> io::Result<()> {
    // SAFETY: fd is owned by guard
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fd and flags are valid
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
fn set_cloexec(fd: libc::c_int) -> io::Result<()> {
    // SAFETY: fd is owned by guard
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fd and flags are valid
    if unsafe { libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
fn install_handler(signal: libc::c_int) -> io::Result<libc::sigaction> {
    // SAFETY: zeroed sigaction is immediately initialized
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    action.sa_sigaction = signal_handler as *const () as usize;
    action.sa_flags = 0;
    // SAFETY: sa_mask points to valid memory
    if unsafe { libc::sigemptyset(&raw mut action.sa_mask) } != 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: zeroed storage will be written by sigaction
    let mut previous = unsafe { std::mem::zeroed::<libc::sigaction>() };
    // SAFETY: both pointers are valid
    if unsafe { libc::sigaction(signal, &raw const action, &raw mut previous) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(previous)
}

#[cfg(unix)]
extern "C" fn signal_handler(signal: libc::c_int) {
    let fd = SIGNAL_WRITE_FD.load(Ordering::SeqCst);
    if fd < 0 {
        return;
    }
    let byte = u8::try_from(signal).unwrap_or(0);
    // SAFETY: write is safe
    let _result = unsafe { libc::write(fd, (&raw const byte).cast::<libc::c_void>(), 1) };
}

#[cfg(unix)]
pub fn restore_default_and_raise(signal: i32) -> io::Result<()> {
    let signal = libc::c_int::try_from(signal)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "signal out of range"))?;
    // SAFETY: SIG_DFL is a valid value
    if unsafe { libc::signal(signal, libc::SIG_DFL) } == libc::SIG_ERR {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: raising a signal is safe
    if unsafe { libc::raise(signal) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
