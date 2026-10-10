// SPDX-License-Identifier: EUPL-1.2

use std::{
    fs::File,
    io::{
        self,
        Write,
    },
    os::fd::{
        FromRawFd as _,
        RawFd,
    },
    sync::mpsc,
    thread,
    time::Duration,
};

const TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn live_on_runs_two_prompts_in_a_row_on_the_same_handle() {
    let (mut master, slave) = open_pty();

    let (first_done_tx, first_done_rx) = mpsc::channel();
    let (results_tx, results_rx) = mpsc::channel();
    thread::spawn(move || {
        let interaction = bang::Interaction::live_on(slave);
        let first = bang::text("first")
            .interaction(interaction.clone())
            .interact();
        let _ = first_done_tx.send(());
        let second = bang::text("second").interaction(interaction).interact();
        let _ = results_tx.send((first, second));
    });

    master.write_all(b"alpha\r").expect("write first answer");
    master.flush().expect("flush PTY input");

    // The second prompt's answer is only sent after the first one has
    // actually been read, because raw mode toggles off and back on between
    // sessions on the same handle; sending it earlier races unread bytes
    // against that toggle and desyncs the line discipline.
    first_done_rx
        .recv_timeout(TIMEOUT)
        .expect("first prompt timed out");
    master.write_all(b"beta\r").expect("write second answer");
    master.flush().expect("flush PTY input");

    let (first, second) = results_rx.recv_timeout(TIMEOUT).expect("prompts timed out");
    assert_eq!(
        first.expect("first prompt succeeded"),
        bang::PromptOutcome::Submit("alpha".to_owned())
    );
    assert_eq!(
        second.expect("second prompt succeeded"),
        bang::PromptOutcome::Submit("beta".to_owned())
    );
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
