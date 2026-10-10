// SPDX-License-Identifier: EUPL-1.2

fn main() -> std::process::ExitCode {
    climax::main_with(|context| {
        match context
            .select("shell")
            .choice("bash", "bash")
            .choice("zsh", "zsh")
            .interact()?
        {
            climax::PromptOutcome::Submit(shell) => context.diagnostic().notice(shell),
            climax::PromptOutcome::Leave => Ok(()),
        }
    })
}
