use bang::PromptOutcome;

fn main() -> climax::Result<()> {
    climax::run_with((), |context, ()| {
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

        context.output().result(&shell).text(|shell| *shell).emit()
    })
}
