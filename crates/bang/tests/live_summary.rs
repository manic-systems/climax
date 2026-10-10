// SPDX-License-Identifier: EUPL-1.2

mod summary_common;

use std::sync::Once;

use bang::{
    ConfirmPrompt,
    PromptOutcome,
};
use summary_common::{
    DIM_SUMMARY,
    find,
    rfind,
    run_confirm,
};

fn colours_on() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // SAFETY: runs once before any test in this binary reads the
        // environment through screw, and tests only ever remove the variable.
        unsafe { std::env::remove_var("NO_COLOR") };
    });
}

#[test]
fn n_answers_at_once_and_leaves_a_dim_summary_after_the_cleared_prompt() {
    colours_on();
    let (output, outcome) = run_confirm(ConfirmPrompt::interaction, b"n");

    assert_eq!(outcome, PromptOutcome::Submit(false));
    let summary = find(&output, DIM_SUMMARY).expect("summary line");
    let cleared = rfind(&output, b"\x1b[2K").expect("prompt was erased");
    assert!(cleared < summary, "summary must follow the erased prompt");
    assert!(
        output[summary + DIM_SUMMARY.len()..]
            .windows(b"Deploy".len())
            .all(|part| part != b"Deploy"),
        "nothing but the summary is durable"
    );
}

#[test]
fn enter_declines_by_default() {
    colours_on();
    let (output, outcome) = run_confirm(ConfirmPrompt::interaction, b"\r");

    assert_eq!(outcome, PromptOutcome::Submit(false));
    assert!(find(&output, DIM_SUMMARY).is_some());
}

#[test]
fn leaving_writes_no_summary() {
    colours_on();
    let (output, outcome) = run_confirm(ConfirmPrompt::interaction, b"\x03");

    assert_eq!(outcome, PromptOutcome::Leave);
    assert!(find(&output, "\u{203a}".as_bytes()).is_none());
}

#[test]
fn the_interaction_can_turn_summaries_off() {
    colours_on();
    let (output, outcome) = run_confirm(
        |prompt, interaction| prompt.interaction(interaction.with_summaries(false)),
        b"n",
    );

    assert_eq!(outcome, PromptOutcome::Submit(false));
    assert!(find(&output, "\u{203a}".as_bytes()).is_none());
}

#[test]
fn a_prompt_setting_overrides_the_interaction() {
    colours_on();
    let (output, _) = run_confirm(
        |prompt, interaction| {
            prompt
                .summary(true)
                .interaction(interaction.with_summaries(false))
        },
        b"n",
    );

    assert!(find(&output, DIM_SUMMARY).is_some());
}
