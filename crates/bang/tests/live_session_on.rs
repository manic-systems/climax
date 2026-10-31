// SPDX-License-Identifier: EUPL-1.2

#![cfg(test)]

use core::time::Duration;
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

use bang::terminal::testing::pty;

const TIMEOUT: Duration = Duration::from_secs(5);
const STDERR: RawFd = 2;

#[test]
fn session_on_a_caller_owned_handle_submits_on_enter_without_touching_process_stderr() {
    let (master_fd, slave) = pty(80, 24).expect("open a pty");
    let mut master = File::from(master_fd);
    let captured_stderr = StderrCapture::install();

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let outcome = bang::text("value")
            .interaction(bang::Interaction::forced_on(slave))
            .interact();
        let _sent = tx.send(outcome);
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

/// redirects process stderr to a pipe so a test can check nothing writes to it
struct StderrCapture {
    saved: RawFd,
    read:  File,
}

impl StderrCapture {
    fn install() -> Self {
        // SAFETY: STDERR is a valid, open fd for this process
        let saved = unsafe { libc::dup(STDERR) };
        assert!(saved >= 0, "dup stderr: {}", io::Error::last_os_error());
        let mut pipe = [-1_i32; 2];
        // SAFETY: pipe is a valid two-element buffer
        let created = unsafe { libc::pipe(pipe.as_mut_ptr()) };
        assert_eq!(created, 0, "create pipe: {}", io::Error::last_os_error());
        let [read_fd, write_fd] = pipe;
        // SAFETY: write_fd is a valid, open fd; STDERR names the fd being
        // replaced
        let redirected = unsafe { libc::dup2(write_fd, STDERR) };
        assert_eq!(
            redirected,
            STDERR,
            "redirect stderr: {}",
            io::Error::last_os_error()
        );
        // SAFETY: write_fd was just duplicated onto STDERR and its own copy is
        // unused
        unsafe {
            libc::close(write_fd);
        }
        // SAFETY: read_fd was just returned by pipe above as a live, owned
        // descriptor
        let read = unsafe { File::from_raw_fd(read_fd) };
        Self { saved, read }
    }

    /// restores stderr and returns what was written meanwhile
    fn restore(mut self) -> Vec<u8> {
        // SAFETY: self.saved is a live descriptor duplicated from stderr in
        // `install`; dup2 closes whatever was on STDERR (the pipe's
        // write end) before replacing it
        unsafe {
            libc::dup2(self.saved, STDERR);
        }
        // SAFETY: self.saved is no longer needed once restored
        unsafe {
            libc::close(self.saved);
        }
        let mut buffer = Vec::new();
        self.read
            .read_to_end(&mut buffer)
            .expect("read captured stderr");
        buffer
    }
}
