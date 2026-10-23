// SPDX-License-Identifier: EUPL-1.2

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
        FromRawFd as _,
        RawFd,
    },
    sync::mpsc,
    thread,
};

use bang::{
    advanced::{
        Event,
        Reaction,
        Value,
        Widget,
        WidgetContext,
        WidgetId,
    },
    screw::{
        RenderCtx,
        Style,
        Surface,
        TickInterest,
        Widget as ScreenWidget,
    },
};

const TIMEOUT: Duration = Duration::from_secs(5);

struct Countdown {
    ticks: u32,
}

impl ScreenWidget for Countdown {
    fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
        out.write(format!("ticks {}", self.ticks), Style::default());
    }

    fn tick_interest(&self) -> TickInterest {
        TickInterest::EveryFrame
    }
}

impl Widget for Countdown {
    fn id(&self) -> WidgetId {
        WidgetId::from("countdown")
    }

    fn handle(&mut self, event: Event, _cx: &mut WidgetContext) -> Reaction {
        match event {
            Event::Tick => {
                self.ticks += 1;
                if self.ticks == 3 {
                    Reaction::Submit(Value::from("done"))
                } else {
                    Reaction::Changed
                }
            },
            Event::Key(_)
            | Event::Paste(_)
            | Event::UnknownEscape(_)
            | Event::Resize { .. }
            | _ => Reaction::Ignored,
        }
    }
}

#[test]
fn a_widget_that_wants_ticks_gets_them_without_input() {
    let (mut master, slave) = open_pty();
    let (results_tx, results_rx) = mpsc::channel();
    thread::spawn(move || {
        let outcome = bang::Interaction::live_on(slave).interact(Countdown { ticks: 0 }, []);
        let _sent = results_tx.send(outcome);
    });

    let value = results_rx
        .recv_timeout(TIMEOUT)
        .expect("the widget never received its ticks")
        .expect("widget succeeded");
    assert_eq!(value, Value::from("done"));

    let mut output = [0_u8; 4096];
    let read = master.read(&mut output).expect("read the frames");
    let text = String::from_utf8_lossy(&output[..read]);
    // Frames are diffed, so each tick rewrites only the changed digit.
    assert!(
        text.contains("ticks 0") && text.contains("6C1") && text.contains("6C2"),
        "{text:?}"
    );
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
