// SPDX-License-Identifier: EUPL-1.2

#![cfg(all(feature = "interactive", feature = "render"))]

use std::{
    fs::File,
    io::{self, Read as _, Write as _},
    os::fd::{FromRawFd as _, RawFd},
    thread,
    time::{Duration, Instant},
};

use climax::{Context, terminal::InteractionMode};

const STDERR: RawFd = 2;

static PROMPTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn with_terminal_shows_a_status_and_resolves_a_select_prompt_without_touching_stderr() {
    let _serial = PROMPTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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

    // `Context` holds an `Rc`-based `Interaction` and is not `Send`, so it has
    // to stay on this thread; the PTY write instead moves to its own thread.
    // `master` itself stays open on this thread until `interact` returns: on
    // Linux, closing a PTY's only master reference hangs up the slave, and
    // the writer thread would otherwise finish (and drop its handle) before
    // raw mode ever activates on it.
    let mut write_handle = master.try_clone().expect("clone master for the writer thread");
    let writer = thread::spawn(move || {
        write_handle.write_all(b"\r").expect("submit the highlighted choice");
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
    haystack.windows(needle.len()).any(|window| window == needle)
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
    unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
}

fn set_blocking(file: &File) {
    use std::os::fd::AsRawFd as _;
    let fd = file.as_raw_fd();
    // SAFETY: fd is owned by file for the duration of this call.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    // SAFETY: fd and flags are valid.
    unsafe { libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK) };
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

fn answered_select_output(context: &Context, mut master: File) -> Vec<u8> {
    let mut write_handle = master.try_clone().expect("clone master for the writer thread");
    let writer = thread::spawn(move || {
        write_handle.write_all(b"\r").expect("submit the highlighted choice");
    });
    context
        .select::<&str>("choice")
        .choice("first", "first")
        .interact()
        .expect("select prompt resolved");
    writer.join().expect("writer thread panicked");
    set_nonblocking(&master);
    let mut collected = Vec::new();
    let mut buffer = [0_u8; 4096];
    while let Ok(read) = master.read(&mut buffer) {
        if read == 0 {
            break;
        }
        collected.extend_from_slice(&buffer[..read]);
    }
    collected
}

#[test]
fn a_submitted_prompt_leaves_a_summary_on_the_terminal_handle_by_default() {
    let _serial = PROMPTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (master, slave) = open_pty();
    let context = Context::new()
        .with_terminal(slave)
        .expect("with_terminal")
        .with_interaction_mode(InteractionMode::Force);
    let output = answered_select_output(&context, master);
    assert!(contains(&output, "choice › first".as_bytes()), "got {output:?}");
}

#[test]
fn prompt_summaries_off_survives_a_later_terminal_and_mode_change() {
    let _serial = PROMPTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (master, slave) = open_pty();
    let context = Context::new()
        .with_prompt_summaries(false)
        .with_terminal(slave)
        .expect("with_terminal")
        .with_interaction_mode(InteractionMode::Force);
    let output = answered_select_output(&context, master);
    assert!(!contains(&output, "choice › first".as_bytes()), "got {output:?}");
}
