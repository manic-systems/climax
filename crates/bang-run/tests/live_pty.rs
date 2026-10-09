// SPDX-License-Identifier: EUPL-1.2

use std::{
    fs::File,
    io::{self, Read, Write},
    os::fd::{AsRawFd as _, FromRawFd as _, RawFd},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn live_search_tracks_cursor_and_cleans_up_multiline_updates() {
    let output = run_in_pty(
        &[
            "search",
            "--page-size",
            "3",
            "--option",
            "alpha\nalpha detail",
            "--option",
            "bravo\nbravo detail",
            "--option",
            "charlie\ncharlie detail",
        ],
        b"\x1b[B\x1b[B\x1b[Ab\x7fc\r",
    );

    assert!(!contains(&output, b"\x1b[?1049"), "must remain inline");
    let initial_hide = find(&output, b"\x1b[?25l");
    let input_show = find(&output, b"\x1b[?25h");
    assert!(
        initial_hide < input_show,
        "input cursor must become visible"
    );
    assert!(
        contains(&output, b"alpha detail")
            && contains(&output, b"bravo")
            && contains(&output, b"charlie detail"),
        "initial multiline rows must all be rendered: {}",
        String::from_utf8_lossy(&output)
    );
    assert!(
        output
            .windows(b"\x1b[2K".len())
            .filter(|part| *part == b"\x1b[2K")
            .count()
            >= 6,
        "filtering and cleanup must erase the multiline retained block"
    );

    let cleanup = rfind(&output, b"\r\x1b[2K\x1b[?25h");
    let result = rfind(&output, b"charlie\r\n");
    assert!(
        cleanup < result,
        "durable output must follow terminal cleanup"
    );
}

#[test]
fn page_down_uses_the_physical_multiline_viewport() {
    let output = run_in_pty_size(
        &[
            "search",
            "--page-size",
            "9",
            "--option",
            "alpha\nalpha detail",
            "--option",
            "bravo\nbravo detail",
            "--option",
            "charlie\ncharlie detail",
        ],
        b"\x1b[6~\x1b[6~\r",
        5,
    );

    assert!(
        contains(&output, b"alpha detail")
            && contains(&output, b"bravo")
            && contains(&output, b"charlie detail"),
        "physical pages should reveal each multiline item: {}",
        String::from_utf8_lossy(&output)
    );
    let cleanup = rfind(&output, b"\r\x1b[2K\x1b[?25h");
    let result = rfind(&output, b"charlie detail\r\n");
    assert!(cleanup < result, "the third physical page should submit");
}

fn run_in_pty(args: &[&str], input: &[u8]) -> Vec<u8> {
    run_in_pty_size(args, input, 24)
}

fn run_in_pty_size(args: &[&str], input: &[u8], rows: u16) -> Vec<u8> {
    let (mut master, slave) = open_pty(rows);
    let stdin = slave.try_clone().expect("clone PTY slave for stdin");
    let stdout = slave.try_clone().expect("clone PTY slave for stdout");
    let stderr = slave.try_clone().expect("clone PTY slave for stderr");
    let mut child = Command::new(env!("CARGO_BIN_EXE_bang"))
        .args(args)
        .env("TERM", "xterm-256color")
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .expect("spawn bang under PTY");
    drop(slave);

    master.write_all(input).expect("write PTY input");
    master.flush().expect("flush PTY input");
    set_nonblocking(&master);

    let started = Instant::now();
    let mut output = Vec::new();
    loop {
        drain(&mut master, &mut output);
        if child.try_wait().expect("poll bang child").is_some() {
            drain(&mut master, &mut output);
            return output;
        }
        if started.elapsed() > TIMEOUT {
            let _result = child.kill();
            panic!(
                "bang PTY session timed out; output: {}",
                String::from_utf8_lossy(&output)
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn open_pty(rows: u16) -> (File, File) {
    let mut master: RawFd = -1;
    let mut slave: RawFd = -1;
    let size = libc::winsize {
        ws_row: rows,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: both fd pointers and the winsize pointer are valid for this call.
    let result = unsafe {
        libc::openpty(
            &raw mut master,
            &raw mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            &raw const size,
        )
    };
    assert_eq!(result, 0, "openpty failed: {}", io::Error::last_os_error());
    // SAFETY: successful openpty returned two newly owned file descriptors.
    unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) }
}

fn set_nonblocking(file: &File) {
    let fd = file.as_raw_fd();
    // SAFETY: fd belongs to a live File and F_GETFL does not modify memory.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    assert!(flags >= 0, "get PTY flags: {}", io::Error::last_os_error());
    // SAFETY: fd remains live and flags came from F_GETFL.
    let result = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
    assert_eq!(
        result,
        0,
        "set PTY nonblocking: {}",
        io::Error::last_os_error()
    );
}

fn drain(master: &mut File, output: &mut Vec<u8>) {
    let mut buffer = [0_u8; 4096];
    loop {
        match master.read(&mut buffer) {
            Ok(0) => return,
            Ok(read) => output.extend_from_slice(&buffer[..read]),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                return;
            },
            // Linux PTY masters report EIO once the final slave closes.
            Err(error) if error.raw_os_error() == Some(libc::EIO) => return,
            Err(error) => panic!("read PTY output: {error}"),
        }
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|part| part == needle)
}

fn find(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .position(|part| part == needle)
        .unwrap_or_else(|| {
            panic!(
                "missing {needle:?} in PTY output: {}",
                String::from_utf8_lossy(haystack)
            )
        })
}

fn rfind(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .rposition(|part| part == needle)
        .unwrap_or_else(|| {
            panic!(
                "missing {needle:?} in PTY output: {}",
                String::from_utf8_lossy(haystack)
            )
        })
}
