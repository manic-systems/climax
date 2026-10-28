// SPDX-License-Identifier: EUPL-1.2

use std::process::ExitCode;

use climax::PromptOutcome;

fn main() -> ExitCode {
    climax::main_with(|context| {
        let shell = match context
            .select("shell")
            .choice("bash", "bash")
            .choice("nushell", "nushell")
            .choice("zsh", "zsh")
            .interact()?
        {
            PromptOutcome::Submit(shell) => shell,
            PromptOutcome::Leave => return Ok(()),
        };

        context
            .output()
            .result(&shell)
            .text(|chosen| *chosen)
            .emit()
    })
}
