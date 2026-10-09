// SPDX-License-Identifier: EUPL-1.2

#![cfg(all(feature = "interactive", feature = "render"))]
#![cfg(test)]
#![expect(
    clippy::non_ascii_literal,
    reason = "terminal glyphs read better as themselves than as escapes"
)]

use core::{
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
    sync::{
        Mutex,
        PoisonError,
    },
    thread,
    time::Instant,
};

use climax::{
    Context,
    terminal::InteractionMode,
};

const STDERR: RawFd = 2;

#[test]
fn with_terminal_shows_a_status_and_resolves_a_select_prompt_without_touching_stderr() {
    let (mut master, slave) = open_pty();
    let captured_stderr = StderrCapture::install();

    let context = Context::new()
        .with_terminal(slave)
        .expect("with_terminal")
        .with_interaction_mode(InteractionMode::Force);

    let status = context.status("loading").start();
    let status_bytes = read_until(&mut master, b"loading");
    status.finish().expect("finish status");
    assert!(
        contains(&status_bytes, b"loading"),
        "status text must render on the terminal handle: {status_bytes:?}"
    );

    // `Context` is not `Send`, so only the pty write moves to a thread, and
    // `master` stays open here because on Linux closing the last master
    // hangs up the slave before raw mode activates
    let mut write_handle = master
        .try_clone()
        .expect("clone master for the writer thread");
    let writer = thread::spawn(move || {
        write_handle
            .write_all(b"\r")
            .expect("submit the highlighted choice");
        write_handle.flush().expect("flush PTY input");
    });

    let outcome = context
        .select::<&str>("choice")
        .choice("first", "first")
        .choice("second", "second")
        .interact();
    writer.join().expect("writer thread panicked");
    drop(master);
    let stderr_bytes = captured_stderr.restore();

    assert_eq!(
        outcome.expect("select prompt resolved"),
        climax::PromptOutcome::Submit("first")
    );
    assert!(
        stderr_bytes.is_empty(),
        "nothing should leak to process stderr: {stderr_bytes:?}"
    );
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn read_until(master: &mut File, needle: &[u8]) -> Vec<u8> {
    set_nonblocking(master);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut collected = Vec::new();
    let mut buffer = [0_u8; 4096];
    while Instant::now() < deadline && !contains(&collected, needle) {
        match master.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => collected.extend_from_slice(&buffer[..read]),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            },
            Err(error) => panic!("read PTY output: {error}"),
        }
    }
    set_blocking(master);
    collected
}

fn set_nonblocking(file: &File) {
    use std::os::fd::AsRawFd as _;
    let fd = file.as_raw_fd();
    // SAFETY: fd is owned by file for the duration of this call.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    // SAFETY: fd and flags are valid.
    unsafe {
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
}

fn set_blocking(file: &File) {
    use std::os::fd::AsRawFd as _;
    let fd = file.as_raw_fd();
    // SAFETY: fd is owned by file for the duration of this call.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    // SAFETY: fd and flags are valid.
    unsafe {
        libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK);
    }
}

fn open_pty() -> (File, File) {
    let mut master_fd: RawFd = -1;
    let mut slave_fd: RawFd = -1;
    // SAFETY: both fd pointers are valid for this call, and null
    // termios/winsize ask openpty for its own defaults.
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
    // SAFETY: successful openpty returned a newly owned master descriptor.
    let master = unsafe { File::from_raw_fd(master_fd) };
    // SAFETY: successful openpty returned a newly owned slave descriptor.
    let slave = unsafe { File::from_raw_fd(slave_fd) };
    (master, slave)
}

/// redirects process stderr to a pipe for a test, to check that an operation
/// never writes to it
struct StderrCapture {
    saved: RawFd,
    read:  File,
}

impl StderrCapture {
    fn install() -> Self {
        // SAFETY: STDERR is a valid, open fd for this process.
        let saved = unsafe { libc::dup(STDERR) };
        assert!(saved >= 0, "dup stderr: {}", io::Error::last_os_error());
        let mut pipe = [-1_i32; 2];
        // SAFETY: pipe is a valid two-element buffer.
        let created = unsafe { libc::pipe(pipe.as_mut_ptr()) };
        assert_eq!(created, 0, "create pipe: {}", io::Error::last_os_error());
        let [read_fd, write_fd] = pipe;
        // SAFETY: write_fd is a valid, open fd; STDERR names the fd being
        // replaced.
        let redirected = unsafe { libc::dup2(write_fd, STDERR) };
        assert_eq!(
            redirected,
            STDERR,
            "redirect stderr: {}",
            io::Error::last_os_error()
        );
        // SAFETY: write_fd was just duplicated onto STDERR and its own copy is
        // unused.
        unsafe {
            libc::close(write_fd);
        }
        // SAFETY: read_fd was just returned by pipe above as a live, owned
        // descriptor.
        let read = unsafe { File::from_raw_fd(read_fd) };
        Self { saved, read }
    }

    /// restores stderr and returns what was written while captured
    fn restore(mut self) -> Vec<u8> {
        // SAFETY: self.saved is a live descriptor duplicated from stderr in
        // `install`, and dup2 closes the pipe's write end on STDERR
        // before replacing it.
        unsafe {
            libc::dup2(self.saved, STDERR);
        }
        // SAFETY: self.saved is no longer needed once restored.
        unsafe {
            libc::close(self.saved);
        }
        set_nonblocking(&self.read);
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            match self.read.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => buffer.extend_from_slice(&chunk[..read]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => panic!("read captured stderr: {error}"),
            }
        }
        buffer
    }
}
