// SPDX-License-Identifier: EUPL-1.2

use std::{
    fs::File,
    io::{self, Read as _},
    os::{
        fd::{FromRawFd as _, RawFd},
        unix::process::CommandExt as _,
    },
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn a_terminal_signal_exits_with_128_plus_its_number() {
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT] {
        assert_eq!(exit_code_after(signal), 128 + signal, "signal {signal}");
    }
}

fn exit_code_after(signal: libc::c_int) -> i32 {
    let (mut master, slave) = open_pty();
    let mut command = Command::new(env!("CARGO_BIN_EXE_bang"));
    command
        .args(["select", "--option", "alpha", "--option", "bravo"])
        .env("TERM", "xterm-256color")
        .stdin(Stdio::from(slave.try_clone().expect("clone slave for stdin")))
        .stdout(Stdio::from(slave.try_clone().expect("clone slave for stdout")))
        .stderr(Stdio::from(slave));
    // SAFETY: the hook only calls async-signal-safe libc functions.
    unsafe {
        // A parent that ran this test in the background or under nohup leaves
        // signals ignored or blocked, and the child inherits both across exec.
        command.pre_exec(|| {
            let mut empty = std::mem::zeroed::<libc::sigset_t>();
            libc::sigemptyset(&raw mut empty);
            libc::sigprocmask(libc::SIG_SETMASK, &raw const empty, std::ptr::null_mut());
            for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGQUIT] {
                libc::signal(signal, libc::SIG_DFL);
            }
            Ok(())
        });
    }
    let mut child = command.spawn().expect("spawn bang under a pty");

    let mut first_frame = [0_u8; 256];
    assert_ne!(master.read(&mut first_frame).expect("read the first frame"), 0);
    let pid = libc::pid_t::try_from(child.id()).expect("pid fits");
    // SAFETY: signalling the child this test spawned.
    assert_eq!(unsafe { libc::kill(pid, signal) }, 0);

    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("poll the child") {
            return status.code().expect("the child exited instead of dying by the signal");
        }
        if started.elapsed() > TIMEOUT {
            let _result = child.kill();
            panic!("bang ignored signal {signal}");
        }
        thread::sleep(Duration::from_millis(10));
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
