// SPDX-License-Identifier: EUPL-1.2

use std::{
    fs::File,
    io::{self, Read as _, Write as _},
    os::fd::{FromRawFd as _, RawFd},
    sync::mpsc,
    thread,
    time::Duration,
};

use bang::{ErrorKind, PromptOutcome};

const TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn a_second_session_on_another_handle_is_busy_and_the_first_is_unharmed() {
    let (mut first_master, first) = open_pty();
    let (_second_master, second) = open_pty();

    let (results_tx, results_rx) = mpsc::channel();
    thread::spawn(move || {
        let outcome = bang::text("first")
            .interaction(bang::Interaction::live_on(first))
            .interact();
        let _ = results_tx.send(outcome);
    });
    let mut buffer = [0_u8; 256];
    assert_ne!(first_master.read(&mut buffer).expect("first frame"), 0);

    let busy = bang::text("second")
        .interaction(bang::Interaction::live_on(second))
        .interact()
        .expect_err("the signal handlers are claimed");
    assert_eq!(busy.kind(), ErrorKind::InteractionBusy);

    first_master.write_all(b"ok\r").expect("answer the first");
    let outcome = results_rx
        .recv_timeout(TIMEOUT)
        .expect("first prompt timed out")
        .expect("first prompt succeeded");
    assert_eq!(outcome, PromptOutcome::Submit("ok".to_owned()));
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
