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
fn a_custom_widget_runs_on_a_caller_owned_handle() {
    use bang::advanced::{
        Value,
        widgets::TextInput,
    };

    let (mut master, slave) = open_pty();
    let (results_tx, results_rx) = mpsc::channel();
    thread::spawn(move || {
        let outcome = bang::Interaction::live_on(slave)
            .interact(TextInput::new("custom").with_prompt("custom: "), []);
        let _sent = results_tx.send(outcome);
    });

    master.write_all(b"alpha\r").expect("write the answer");

    let value = results_rx
        .recv_timeout(TIMEOUT)
        .expect("widget timed out")
        .expect("widget succeeded");
    assert_eq!(value, Value::from("alpha"));
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
