// SPDX-License-Identifier: EUPL-1.2

#![cfg(test)]

use core::{
    ptr,
    time::Duration,
};
use std::{
    fs::File,
    io::{
        self,
        Write as _,
    },
    os::fd::{
        FromRawFd as _,
        RawFd,
    },
    sync::mpsc,
    thread,
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
        let _first_sent = first_done_tx.send(());
        let second = bang::text("second").interaction(interaction).interact();
        let _results_sent = results_tx.send((first, second));
    });

    master.write_all(b"alpha\r").expect("write first answer");
    master.flush().expect("flush PTY input");

    // send the second answer only after the first is read, since raw mode
    // toggles between sessions and an earlier send races unread bytes
    // against it
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
