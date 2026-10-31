// SPDX-License-Identifier: EUPL-1.2

#![cfg(test)]

use core::time::Duration;
use std::{
    fs::File,
    io::{
        Read as _,
        Write as _,
    },
    sync::mpsc,
    thread,
};

use bang::{
    ErrorKind,
    PromptOutcome,
    terminal::testing::pty,
};

const TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn a_second_session_on_another_handle_is_busy_and_the_first_is_unharmed() {
    let (first_master_fd, first) = pty(80, 24).expect("open a pty");
    let mut first_master = File::from(first_master_fd);
    let (_second_master, second) = pty(80, 24).expect("open a pty");

    let (results_tx, results_rx) = mpsc::channel();
    thread::spawn(move || {
        let outcome = bang::text("first")
            .interaction(bang::Interaction::live_on(first))
            .interact();
        let _sent = results_tx.send(outcome);
    });
    let mut buffer = [0_u8; 256];
    assert_ne!(first_master.read(&mut buffer).expect("first frame"), 0);

    let busy = bang::text("second")
        .interaction(bang::Interaction::live_on(second))
        .interact()
        .expect_err("the signal handlers are claimed");
    assert_eq!(busy.kind(), ErrorKind::InteractionBusy);

    first_master.write_all(b"ok\r").expect("answer the first");
    let outcome = results_rx
        .recv_timeout(TIMEOUT)
        .expect("first prompt timed out")
        .expect("first prompt succeeded");
    assert_eq!(outcome, PromptOutcome::Submit("ok".to_owned()));
}
