// SPDX-License-Identifier: EUPL-1.2

mod summary_common;

use bang::ConfirmPrompt;
use summary_common::{DIM_SUMMARY, PLAIN_SUMMARY, find, run_confirm};

#[test]
fn no_color_keeps_the_summary_but_drops_the_dimming() {
    // SAFETY: this is the only test in the binary, so nothing reads the
    // environment concurrently.
    unsafe { std::env::set_var("NO_COLOR", "1") };
    let (output, _) = run_confirm(ConfirmPrompt::interaction, b"n");

    assert!(find(&output, PLAIN_SUMMARY).is_some(), "{output:?}");
    assert!(find(&output, DIM_SUMMARY).is_none());
}
