// SPDX-License-Identifier: EUPL-1.2

use std::process::ExitCode;

fn main() -> ExitCode {
    climax::main_with(|context| {
        let shell = context
            .select("shell")
            .choice("bash", "bash")
            .choice("nushell", "nushell")
            .choice("zsh", "zsh")
            .interact()?
            .or_cancel()?;

        context
            .output()
            .result(&shell)
            .text(|chosen| *chosen)
            .emit()
    })
}
