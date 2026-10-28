// SPDX-License-Identifier: EUPL-1.2

#![cfg(test)]

use core::time::Duration;
use std::{
    fs::File,
    io::Write as _,
    sync::mpsc,
    thread,
};

use bang::terminal::testing::pty;

const TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn a_custom_widget_runs_on_a_caller_owned_handle() {
    use bang::advanced::{
        Value,
        widgets::TextInput,
    };

    let (master_fd, slave) = pty(80, 24).expect("open a pty");
    let mut master = File::from(master_fd);
    let (results_tx, results_rx) = mpsc::channel();
    thread::spawn(move || {
        let outcome = bang::Interaction::live_on(slave)
            .interact(TextInput::new("custom").with_prompt("custom: "), []);
        let _sent = results_tx.send(outcome);
    });

    master.write_all(b"alpha\r").expect("write the answer");

    let value = results_rx
        .recv_timeout(TIMEOUT)
        .expect("widget timed out")
        .expect("widget succeeded");
    assert_eq!(value, Value::from("alpha"));
}
