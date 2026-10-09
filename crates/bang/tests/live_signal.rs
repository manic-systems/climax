// SPDX-License-Identifier: EUPL-1.2

use std::{
    fs::File,
    io::{self, Read as _, Write as _},
    os::fd::{FromRawFd as _, RawFd},
    sync::mpsc,
    thread,
    time::Duration,
};

use bang::{ErrorKind, PromptOutcome, terminal::Signal};

const TIMEOUT: Duration = Duration::from_secs(5);

// The signal handlers are process-wide, so both cases run in turn in one test.
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
        let _ = results_tx.send(outcome);
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

    // SAFETY: SIG_IGN is a valid disposition.
    let previous = unsafe { libc::signal(libc::SIGHUP, libc::SIG_IGN) };
    let (results_tx, results_rx) = mpsc::channel();
    thread::spawn(move || {
        let outcome = bang::text("name")
            .interaction(bang::Interaction::live_on(slave))
            .interact();
        let _ = results_tx.send(outcome);
    });

    wait_for_first_frame(&mut master);
    raise(libc::SIGHUP);
    master.write_all(b"ok\r").expect("write the answer");
    let outcome = results_rx
        .recv_timeout(TIMEOUT)
        .expect("prompt timed out")
        .expect("an ignored signal leaves the prompt running");
    assert_eq!(outcome, PromptOutcome::Submit("ok".to_owned()));

    // SAFETY: restoring the disposition read above.
    let after = unsafe { libc::signal(libc::SIGHUP, previous) };
    assert_eq!(after, libc::SIG_IGN);
}

const TERMINAL_SIGNALS: [libc::c_int; 4] =
    [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT];

// A parent that started this process in the background or under nohup leaves
// SIGINT, SIGQUIT or SIGHUP ignored, and the guard rightly leaves those alone.
struct DefaultDispositions(Vec<(libc::c_int, libc::sighandler_t)>);

impl DefaultDispositions {
    fn reset() -> Self {
        // SAFETY: SIG_DFL is a valid disposition and the set is initialised
        // before pthread_sigmask reads it.
        unsafe {
            let mut set = std::mem::zeroed::<libc::sigset_t>();
            libc::sigemptyset(&raw mut set);
            let previous = TERMINAL_SIGNALS
                .map(|signal| {
                    libc::sigaddset(&raw mut set, signal);
                    (signal, libc::signal(signal, libc::SIG_DFL))
                })
                .to_vec();
            assert_eq!(
                libc::pthread_sigmask(libc::SIG_UNBLOCK, &raw const set, std::ptr::null_mut()),
                0
            );
            Self(previous)
        }
    }
}

impl Drop for DefaultDispositions {
    fn drop(&mut self) {
        for &(signal, previous) in &self.0 {
            // SAFETY: restoring the disposition read in reset.
            unsafe { libc::signal(signal, previous) };
        }
    }
}

fn raise(signal: libc::c_int) {
    // SAFETY: the prompt has installed a handler or the signal is ignored.
    assert_eq!(unsafe { libc::raise(signal) }, 0);
}

fn wait_for_first_frame(master: &mut File) {
    let mut buffer = [0_u8; 256];
    assert_ne!(master.read(&mut buffer).expect("read the first frame"), 0);
}

fn termios(file: &File) -> Vec<u8> {
    use std::os::fd::AsRawFd as _;

    // SAFETY: a zeroed termios is overwritten by tcgetattr.
    let mut state = unsafe { std::mem::zeroed::<libc::termios>() };
    // SAFETY: the descriptor is open and state is valid for the call.
    assert_eq!(unsafe { libc::tcgetattr(file.as_raw_fd(), &raw mut state) }, 0);
    let mut flags = Vec::new();
    for value in [state.c_iflag, state.c_oflag, state.c_cflag, state.c_lflag] {
        flags.extend(value.to_ne_bytes());
    }
    flags.extend(state.c_cc);
    flags
}

fn open_pty() -> (File, File) {
    let mut master: RawFd = -1;
    let mut slave: RawFd = -1;
    // SAFETY: both fd pointers are valid for this call, and null termios/winsize
    // ask openpty for its own defaults.
    let result = unsafe {
        libc::openpty(
            &raw mut master,
            &raw mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    assert_eq!(result, 0, "openpty failed: {}", io::Error::last_os_error());
    // SAFETY: successful openpty returned two newly owned file descriptors.
    unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) }
}
