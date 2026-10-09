// SPDX-License-Identifier: EUPL-1.2

use bang::{
    screw::Surface,
    terminal::Decoder,
};

fn shell_prompt() -> bang::SelectPrompt<&'static str> {
    bang::select("shell")
        .choice("bash", "bash")
        .choice("zsh", "zsh")
}

fn main() {
    let _prompts = (
        shell_prompt(),
        bang::text("name"),
        bang::confirm("proceed"),
        bang::password("secret"),
    );
    let _surface = Surface::new();
    let _decoder = Decoder::default();
}

#[cfg(test)]
mod tests {
    use bang::{
        PromptOutcome,
        advanced::{
            Event,
            Key,
            scripted_interaction,
        },
    };

    use super::*;

    #[test]
    fn a_scripted_interaction_drives_a_select_prompt() {
        let interaction = scripted_interaction([[Event::key(Key::Down), Event::key(Key::Enter)]]);
        let outcome = shell_prompt().interaction(interaction).interact().unwrap();
        assert_eq!(outcome, PromptOutcome::Submit("zsh"));
    }

    #[test]
    fn a_scripted_interaction_drives_a_text_prompt() {
        let interaction =
            scripted_interaction([[Event::char('a'), Event::char('b'), Event::key(Key::Enter)]]);
        let outcome = bang::text("name")
            .interaction(interaction)
            .interact()
            .unwrap();
        assert_eq!(outcome, PromptOutcome::Submit("ab".to_owned()));
    }
}
