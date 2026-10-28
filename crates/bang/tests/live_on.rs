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
fn live_on_runs_two_prompts_in_a_row_on_the_same_handle() {
    let (master_fd, slave) = pty(80, 24).expect("open a pty");
    let mut master = File::from(master_fd);

    let (first_done_tx, first_done_rx) = mpsc::channel();
    let (results_tx, results_rx) = mpsc::channel();
    thread::spawn(move || {
        let interaction = bang::Interaction::live_on(slave);
        let first = bang::text("first")
            .interaction(interaction.clone())
            .interact();
        let _first_sent = first_done_tx.send(());
        let second = bang::text("second").interaction(interaction).interact();
        let _results_sent = results_tx.send((first, second));
    });

    master.write_all(b"alpha\r").expect("write first answer");
    master.flush().expect("flush PTY input");

    // send the second answer only after the first is read, since raw mode
    // toggles between sessions and an earlier send races unread bytes
    // against it
    first_done_rx
        .recv_timeout(TIMEOUT)
        .expect("first prompt timed out");
    master.write_all(b"beta\r").expect("write second answer");
    master.flush().expect("flush PTY input");

    let (first, second) = results_rx.recv_timeout(TIMEOUT).expect("prompts timed out");
    assert_eq!(
        first.expect("first prompt succeeded"),
        bang::PromptOutcome::Submit("alpha".to_owned())
    );
    assert_eq!(
        second.expect("second prompt succeeded"),
        bang::PromptOutcome::Submit("beta".to_owned())
    );
}
