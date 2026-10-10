// SPDX-License-Identifier: EUPL-1.2

#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::{
    fs::File,
    io::{
        self,
        Read,
        Write,
    },
    os::fd::{
        AsRawFd as _,
        FromRawFd as _,
        RawFd,
    },
    sync::Mutex,
    thread,
    time::{
        Duration,
        Instant,
    },
};

use bang::{
    ConfirmPrompt,
    Interaction,
    PromptOutcome,
};

const TIMEOUT: Duration = Duration::from_secs(5);
pub const DIM_SUMMARY: &[u8] = b"\x1b[2mDeploy to prod? \xe2\x80\xba no\x1b[0m\r\n";
pub const PLAIN_SUMMARY: &[u8] = b"Deploy to prod? \xe2\x80\xba no\r\n";

static SESSIONS: Mutex<()> = Mutex::new(());

pub fn run_confirm(
    configure: impl FnOnce(ConfirmPrompt, Interaction) -> ConfirmPrompt + Send + 'static,
    keys: &'static [u8],
) -> (Vec<u8>, PromptOutcome<bool>) {
    let _one_session_at_a_time = SESSIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut master, slave) = open_pty();
    set_nonblocking(&master);
    let prompt = thread::spawn(move || {
        configure(
            bang::confirm("Deploy to prod?"),
            Interaction::live_on(slave),
        )
        .interact()
        .expect("confirm succeeded")
    });

    let mut output = Vec::new();
    read_until(&mut master, &mut output, b"Deploy to prod?");
    master.write_all(keys).expect("write keys");
    let outcome = prompt.join().expect("prompt thread");
    drain(&mut master, &mut output);
    (output, outcome)
}

pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|part| part == needle)
}

pub fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .rposition(|part| part == needle)
}

fn read_until(master: &mut File, output: &mut Vec<u8>, needle: &[u8]) {
    let started = Instant::now();
    while find(output, needle).is_none() {
        assert!(
            started.elapsed() < TIMEOUT,
            "timed out waiting for {needle:?}"
        );
        drain(master, output);
        thread::sleep(Duration::from_millis(5));
    }
}

fn drain(master: &mut File, output: &mut Vec<u8>) {
    let mut buffer = [0; 4096];
    loop {
        match master.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => output.extend_from_slice(&buffer[..count]),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(_) => break,
        }
    }
}

fn set_nonblocking(file: &File) {
    // SAFETY: the descriptor is valid for the duration of both fcntl calls.
    unsafe {
        let flags = libc::fcntl(file.as_raw_fd(), libc::F_GETFL);
        assert!(flags >= 0, "F_GETFL failed: {}", io::Error::last_os_error());
        let result = libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK);
        assert_eq!(result, 0, "F_SETFL failed: {}", io::Error::last_os_error());
    }
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
