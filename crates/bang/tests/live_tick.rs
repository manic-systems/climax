// SPDX-License-Identifier: EUPL-1.2

#![cfg(test)]

use core::time::Duration;
use std::{
    fs::File,
    io::Read as _,
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
    terminal::testing::pty,
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
    let (master_fd, slave) = pty(80, 24).expect("open a pty");
    let mut master = File::from(master_fd);
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
    // frames are diffed, so each tick rewrites only the changed digit
    assert!(
        text.contains("ticks 0") && text.contains("6C1") && text.contains("6C2"),
        "{text:?}"
    );
}
