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
        let _ = results_tx.send(outcome);
    });

    master.write_all(b"alpha\r").expect("write the answer");

    let value = results_rx
        .recv_timeout(TIMEOUT)
        .expect("widget timed out")
        .expect("widget succeeded");
    assert_eq!(value, Value::from("alpha"));
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
