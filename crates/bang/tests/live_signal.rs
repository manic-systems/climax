// SPDX-License-Identifier: EUPL-1.2

#![cfg(test)]

use core::{
    mem,
    ptr,
    time::Duration,
};
use std::{
    fs::File,
    io::{
        self,
        Read as _,
        Write as _,
    },
    os::fd::{
        FromRawFd as _,
        RawFd,
    },
    sync::mpsc,
    thread,
};

use bang::{
    ErrorKind,
    PromptOutcome,
    terminal::Signal,
};

const TIMEOUT: Duration = Duration::from_secs(5);

// the signal handlers are process-wide, so both cases run in turn in one test
#[test]
fn signals_during_a_prompt() {
    let _inherited = DefaultDispositions::reset();
    a_signal_interrupts_a_prompt_without_ending_the_process();
    a_signal_the_host_ignored_does_not_interrupt_the_prompt();
}

fn a_signal_interrupts_a_prompt_without_ending_the_process() {
    for (raw, signal) in [
        (libc::SIGTERM, Signal::TERM),
        (libc::SIGHUP, Signal::HUP),
        (libc::SIGINT, Signal::INT),
        (libc::SIGQUIT, Signal::QUIT),
    ] {
        a_signal_interrupts_one_prompt(raw, signal);
    }
}

fn a_signal_interrupts_one_prompt(raw: libc::c_int, signal: Signal) {
    let (mut master, slave) = open_pty();
    let probe = slave.try_clone().expect("duplicate the slave");
    let before = termios(&probe);

    let (results_tx, results_rx) = mpsc::channel();
    thread::spawn(move || {
        let outcome = bang::text("name")
            .interaction(bang::Interaction::live_on(slave))
            .interact();
        let _sent = results_tx.send(outcome);
    });

    wait_for_first_frame(&mut master);
    raise(raw);
    let error = results_rx
        .recv_timeout(TIMEOUT)
        .expect("prompt timed out")
        .expect_err("a signal is an error");
    assert_eq!(error.kind(), ErrorKind::Interrupted);
    assert_eq!(error.signal(), Some(signal));
    assert_eq!(termios(&probe), before);
}

fn a_signal_the_host_ignored_does_not_interrupt_the_prompt() {
    let (mut master, slave) = open_pty();

    // SAFETY: SIG_IGN is a valid disposition
    let previous = unsafe { libc::signal(libc::SIGHUP, libc::SIG_IGN) };
    let (results_tx, results_rx) = mpsc::channel();
    thread::spawn(move || {
        let outcome = bang::text("name")
            .interaction(bang::Interaction::live_on(slave))
            .interact();
        let _sent = results_tx.send(outcome);
    });

    wait_for_first_frame(&mut master);
    raise(libc::SIGHUP);
    master.write_all(b"ok\r").expect("write the answer");
    let outcome = results_rx
        .recv_timeout(TIMEOUT)
        .expect("prompt timed out")
        .expect("an ignored signal leaves the prompt running");
    assert_eq!(outcome, PromptOutcome::Submit("ok".to_owned()));

    // SAFETY: restoring the disposition read above
    let after = unsafe { libc::signal(libc::SIGHUP, previous) };
    assert_eq!(after, libc::SIG_IGN);
}

const TERMINAL_SIGNALS: [libc::c_int; 4] =
    [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT];

// a parent in the background or under nohup leaves SIGINT, SIGQUIT or SIGHUP
// ignored, and the guard rightly leaves those alone
struct DefaultDispositions(Vec<(libc::c_int, libc::sighandler_t)>);

impl DefaultDispositions {
    fn reset() -> Self {
        // SAFETY: a zeroed sigset_t is plain data that sigemptyset initialises
        let mut set = unsafe { mem::zeroed::<libc::sigset_t>() };
        // SAFETY: set is a valid sigset_t for the call
        unsafe {
            libc::sigemptyset(&raw mut set);
        }
        let previous = TERMINAL_SIGNALS
            .map(|signal| {
                // SAFETY: set was initialised by sigemptyset above
                unsafe {
                    libc::sigaddset(&raw mut set, signal);
                }
                // SAFETY: SIG_DFL is a valid disposition
                (signal, unsafe { libc::signal(signal, libc::SIG_DFL) })
            })
            .to_vec();
        // SAFETY: set is initialised and a null old set is allowed
        let status =
            unsafe { libc::pthread_sigmask(libc::SIG_UNBLOCK, &raw const set, ptr::null_mut()) };
        assert_eq!(status, 0, "unblock the terminal signals");
        Self(previous)
    }
}

impl Drop for DefaultDispositions {
    fn drop(&mut self) {
        for &(signal, previous) in &self.0 {
            // SAFETY: restoring the disposition read in reset
            unsafe {
                libc::signal(signal, previous);
            }
        }
    }
}

fn raise(signal: libc::c_int) {
    // SAFETY: the prompt has installed a handler or the signal is ignored
    assert_eq!(unsafe { libc::raise(signal) }, 0);
}

fn wait_for_first_frame(master: &mut File) {
    let mut buffer = [0_u8; 256];
    assert_ne!(master.read(&mut buffer).expect("read the first frame"), 0);
}

fn termios(file: &File) -> Vec<u64> {
    use std::os::fd::AsRawFd as _;

    // SAFETY: a zeroed termios is overwritten by tcgetattr
    let mut state = unsafe { mem::zeroed::<libc::termios>() };
    // SAFETY: the descriptor is open and state is valid for the call
    let status = unsafe { libc::tcgetattr(file.as_raw_fd(), &raw mut state) };
    assert_eq!(status, 0);
    let mut flags: Vec<u64> = [state.c_iflag, state.c_oflag, state.c_cflag, state.c_lflag]
        .into_iter()
        .map(u64::from)
        .collect();
    flags.extend(state.c_cc.into_iter().map(u64::from));
    flags
}

fn open_pty() -> (File, File) {
    let mut master_fd: RawFd = -1;
    let mut slave_fd: RawFd = -1;
    // SAFETY: both fd pointers are valid, and null termios/winsize select
    // openpty's defaults
    let result = unsafe {
        libc::openpty(
            &raw mut master_fd,
            &raw mut slave_fd,
            ptr::null_mut(),
            ptr::null(),
            ptr::null(),
        )
    };
    assert_eq!(result, 0, "openpty failed: {}", io::Error::last_os_error());
    // SAFETY: successful openpty returned a newly owned master descriptor
    let master = unsafe { File::from_raw_fd(master_fd) };
    // SAFETY: successful openpty returned a newly owned slave descriptor
    let slave = unsafe { File::from_raw_fd(slave_fd) };
    (master, slave)
}
