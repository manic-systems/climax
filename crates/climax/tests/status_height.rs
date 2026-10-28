// SPDX-License-Identifier: EUPL-1.2

#![cfg(feature = "render")]
#![cfg(test)]

use core::{
    ptr,
    time::Duration,
};
use std::{
    fs::File,
    io::{
        self,
        Read as _,
    },
    os::fd::{
        AsRawFd as _,
        FromRawFd as _,
        RawFd,
    },
    thread,
    time::Instant,
};

use climax::{
    Context,
    terminal::StatusMode,
};

const ROWS: u16 = 10;

fn open_pty() -> (File, File) {
    let mut master_fd: RawFd = -1;
    let mut slave_fd: RawFd = -1;
    let size = libc::winsize {
        ws_row:    ROWS,
        ws_col:    80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: both fd pointers and the winsize are valid for this call, and
    // null termios asks openpty for its defaults.
    let result = unsafe {
        libc::openpty(
            &raw mut master_fd,
            &raw mut slave_fd,
            ptr::null_mut(),
            ptr::null(),
            &raw const size,
        )
    };
    assert_eq!(result, 0, "openpty failed: {}", io::Error::last_os_error());
    // SAFETY: successful openpty returned a newly owned master descriptor.
    let master = unsafe { File::from_raw_fd(master_fd) };
    // SAFETY: successful openpty returned a newly owned slave descriptor.
    let slave = unsafe { File::from_raw_fd(slave_fd) };
    (master, slave)
}

fn drain(master: &mut File) -> Vec<u8> {
    let fd = master.as_raw_fd();
    // SAFETY: fd is owned by master for the duration of this call.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    // SAFETY: fd and flags are valid.
    unsafe {
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
    let deadline = Instant::now() + Duration::from_millis(600);
    let mut collected = Vec::new();
    let mut buffer = [0_u8; 4096];
    while Instant::now() < deadline {
        match master.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => collected.extend_from_slice(&buffer[..read]),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            },
            Err(error) => panic!("read PTY output: {error}"),
        }
    }
    collected
}

fn largest_upward_move(bytes: &[u8]) -> usize {
    let text = String::from_utf8_lossy(bytes);
    text.split('\u{1b}')
        .filter_map(|piece| {
            let digits = piece.strip_prefix('[')?;
            let end = digits.find('A')?;
            digits[..end].parse().ok()
        })
        .max()
        .unwrap_or(0)
}

#[test]
fn a_stack_taller_than_the_terminal_never_moves_the_cursor_past_the_top() {
    let (mut master, slave) = open_pty();
    let context = Context::new()
        .with_terminal(slave)
        .expect("with_terminal")
        .with_status_mode(StatusMode::Live)
        .expect("live mode");

    let statuses: Vec<_> = (0..30)
        .map(|index| context.status(format!("status {index}")).spinner().start())
        .collect();
    let bytes = drain(&mut master);
    drop(statuses);

    let move_up = largest_upward_move(&bytes);
    assert!(
        move_up < usize::from(ROWS),
        "cursor moved {move_up} rows up on a {ROWS} row terminal"
    );
}
