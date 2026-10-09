// SPDX-License-Identifier: EUPL-1.2

use std::{
    fs::File,
    io::{self, Read, Write},
    os::fd::{FromRawFd as _, RawFd},
    sync::mpsc,
    thread,
    time::Duration,
};

const TIMEOUT: Duration = Duration::from_secs(5);
const STDERR: RawFd = 2;

#[test]
fn session_on_a_caller_owned_handle_submits_on_enter_without_touching_process_stderr() {
    let (mut master, slave) = open_pty();
    let captured_stderr = StderrCapture::install();

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = bang::text("value")
            .interaction(bang::Interaction::forced_on(slave))
            .interact();
        let _ = tx.send(result);
    });

    master.write_all(b"hello\r").expect("write PTY input");
    master.flush().expect("flush PTY input");

    let result = rx.recv_timeout(TIMEOUT).expect("live session timed out");
    let stderr_bytes = captured_stderr.restore();

    assert_eq!(
        result.expect("live session succeeded"),
        bang::PromptOutcome::Submit("hello".to_owned())
    );
    assert!(
        stderr_bytes.is_empty(),
        "session must not write to process stderr: {stderr_bytes:?}"
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

/// Redirects process stderr to a pipe for the duration of a test, so a claim
/// that some operation never writes to it can be checked rather than assumed.
struct StderrCapture {
    saved: RawFd,
    read: File,
}

impl StderrCapture {
    fn install() -> Self {
        // SAFETY: STDERR is a valid, open fd for this process.
        let saved = unsafe { libc::dup(STDERR) };
        assert!(saved >= 0, "dup stderr: {}", io::Error::last_os_error());
        let mut pipe = [-1_i32; 2];
        // SAFETY: pipe is a valid two-element buffer.
        let result = unsafe { libc::pipe(pipe.as_mut_ptr()) };
        assert_eq!(result, 0, "create pipe: {}", io::Error::last_os_error());
        let [read_fd, write_fd] = pipe;
        // SAFETY: write_fd is a valid, open fd; STDERR names the fd being replaced.
        let result = unsafe { libc::dup2(write_fd, STDERR) };
        assert_eq!(result, STDERR, "redirect stderr: {}", io::Error::last_os_error());
        // SAFETY: write_fd was just duplicated onto STDERR and its own copy is unused.
        unsafe { libc::close(write_fd) };
        // SAFETY: read_fd was just returned by pipe above as a live, owned descriptor.
        let read = unsafe { File::from_raw_fd(read_fd) };
        Self { saved, read }
    }

    /// Restores the original stderr and returns whatever was written to it
    /// while captured.
    fn restore(mut self) -> Vec<u8> {
        // SAFETY: self.saved is a live descriptor duplicated from stderr in `install`;
        // dup2 closes whatever was on STDERR (the pipe's write end) before replacing it.
        unsafe { libc::dup2(self.saved, STDERR) };
        // SAFETY: self.saved is no longer needed once restored.
        unsafe { libc::close(self.saved) };
        let mut buffer = Vec::new();
        self.read
            .read_to_end(&mut buffer)
            .expect("read captured stderr");
        buffer
    }
}
