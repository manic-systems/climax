// SPDX-License-Identifier: EUPL-1.2

//! A custom driver built on `bang::advanced::interaction_from_runner` can only
//! report cancellation or ended input through `bang::Error`'s own
//! constructors, since neither `ErrorKind` nor the fields of `Error` are
//! constructible from outside the crate.

#[test]
fn a_custom_runner_can_report_cancelled_and_input_ended() {
    let cancelled =
        bang::advanced::interaction_from_runner(|_widget| Err(bang::Error::cancelled()));
    assert_eq!(
        bang::text("name")
            .interaction(cancelled)
            .interact()
            .unwrap(),
        bang::PromptOutcome::Leave
    );

    let input_ended =
        bang::advanced::interaction_from_runner(|_widget| Err(bang::Error::input_ended()));
    assert_eq!(
        bang::text("name")
            .interaction(input_ended)
            .interact()
            .unwrap_err()
            .kind(),
        bang::ErrorKind::InputEnded
    );
}
