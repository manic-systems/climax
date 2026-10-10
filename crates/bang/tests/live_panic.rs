// SPDX-License-Identifier: EUPL-1.2

use std::{
    fs::File,
    io::{
        self,
        Read as _,
        Write as _,
    },
    os::fd::{
        AsRawFd as _,
        FromRawFd as _,
        RawFd,
    },
    panic,
    thread,
};

use bang::{
    advanced::{
        Event,
        Key,
        KeyEvent,
        Reaction,
        Widget,
        WidgetContext,
        WidgetId,
    },
    screw::{
        RenderCtx,
        Role,
        Surface,
    },
};

// The signal handlers are process-wide, so both cases run in turn in one test.
#[test]
fn panics_during_a_prompt() {
    a_panicking_widget_leaves_the_terminal_as_it_found_it();
    a_panic_with_a_tall_frame_keeps_the_panic_message();
}

fn a_panicking_widget_leaves_the_terminal_as_it_found_it() {
    let (mut master, slave) = open_pty();
    let probe = slave.try_clone().expect("duplicate the slave");
    let before = local_modes(&probe);

    let worker = thread::spawn(move || {
        let _ = bang::text("name")
            .validator(|_| panic!("validator exploded"))
            .interaction(bang::Interaction::live_on(slave))
            .interact();
    });

    let mut buffer = [0_u8; 256];
    assert_ne!(master.read(&mut buffer).expect("read the first frame"), 0);
    master.write_all(b"x\r").expect("submit");

    assert!(worker.join().is_err());
    assert_eq!(local_modes(&probe), before);
}

struct Tall;

impl bang::screw::Widget for Tall {
    fn render(&self, ctx: &RenderCtx, out: &mut Surface) {
        let style = ctx.theme().style(Role::Prompt);
        for line in 0..4 {
            out.write(format!("frame line {line}"), style);
            out.newline();
        }
    }
}

impl Widget for Tall {
    fn id(&self) -> WidgetId {
        WidgetId::from("tall")
    }

    fn handle(&mut self, event: Event, _cx: &mut WidgetContext) -> Reaction {
        match event {
            Event::Key(KeyEvent {
                key: Key::Char('x'),
                ..
            }) => panic!("kaboom"),
            _ => Reaction::Ignored,
        }
    }
}

fn a_panic_with_a_tall_frame_keeps_the_panic_message() {
    let (mut master, slave) = open_pty();
    let tty = slave.try_clone().expect("duplicate the slave");
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |_| {
        let _ = (&tty).write_all(b"\nPANIC-MESSAGE\n");
    }));

    let worker = thread::spawn(move || {
        let _ = bang::Interaction::live_on(slave).interact(Tall, []);
    });

    let mut output = Vec::new();
    let mut buffer = [0_u8; 256];
    let read = master.read(&mut buffer).expect("read the first frame");
    output.extend_from_slice(&buffer[..read]);
    master.write_all(b"x").expect("trigger the panic");
    assert!(worker.join().is_err());
    panic::set_hook(previous);
    drain(&master, &mut output);

    let text = String::from_utf8_lossy(&output);
    let message = text
        .find("PANIC-MESSAGE")
        .expect("the panic message reached the terminal");
    assert!(text[..message].contains("frame line 3"));
    assert!(
        !has_cursor_up(&text[message..]),
        "cleanup moved up over the panic message: {:?}",
        &text[message..],
    );
}

fn has_cursor_up(text: &str) -> bool {
    text.split("\x1b[").skip(1).any(|rest| {
        rest.trim_start_matches(|c: char| c.is_ascii_digit())
            .starts_with('A')
    })
}

fn drain(master: &File, output: &mut Vec<u8>) {
    let mut buffer = [0_u8; 1024];
    loop {
        let mut poll = libc::pollfd {
            fd:      master.as_raw_fd(),
            events:  libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll points at one valid pollfd.
        if unsafe { libc::poll(&raw mut poll, 1, 300) } <= 0 {
            return;
        }
        match (&*master).read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => output.extend_from_slice(&buffer[..read]),
        }
    }
}

fn local_modes(file: &File) -> libc::tcflag_t {
    // SAFETY: a zeroed termios is overwritten by tcgetattr.
    let mut state = unsafe { std::mem::zeroed::<libc::termios>() };
    // SAFETY: the descriptor is open and state is valid for the call.
    assert_eq!(
        unsafe { libc::tcgetattr(file.as_raw_fd(), &raw mut state) },
        0
    );
    state.c_lflag
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
