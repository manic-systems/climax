// SPDX-License-Identifier: EUPL-1.2

#![cfg(feature = "interactive")]

use climax::{
    Context,
    ErrorKind,
    bang::advanced::{
        Event,
        Key,
        scripted_interaction,
    },
};

fn ask(context: &Context) -> climax::Result<&'static str> {
    Ok(context
        .select("Shell")
        .choice("bash", "bash")
        .choice("zsh", "zsh")
        .interact()?
        .or_cancel()?)
}

#[test]
fn leaving_a_prompt_with_or_cancel_ends_the_handler_as_a_cancellation() {
    let leave = scripted_interaction([[Event::key(Key::Esc)]]);

    climax::run_with((), |mut context, ()| {
        context.set_interaction(leave);
        let error = ask(&context).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Cancelled);
        Ok(())
    })
    .unwrap();
}

#[test]
fn submitting_passes_through_or_cancel() {
    let submit = scripted_interaction([[Event::key(Key::Enter)]]);

    climax::run_with((), |mut context, ()| {
        context.set_interaction(submit);
        assert_eq!(ask(&context).unwrap(), "bash");
        Ok(())
    })
    .unwrap();
}
