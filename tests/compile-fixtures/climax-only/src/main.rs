// SPDX-License-Identifier: EUPL-1.2

use std::sync::{
    Arc,
    atomic::{
        AtomicUsize,
        Ordering,
    },
};

use climax::{
    prelude::*,
    screw::{
        RenderCtx,
        Style,
        Surface,
        Widget,
        widget,
    },
    serde::Serialize,
};

#[derive(Clone, Copy, Debug, Serialize, ValueEnum)]
#[serde(crate = "climax::serde")]
enum Shell {
    Bash,
    Zsh,
}

/// configure a shell
#[derive(Parse)]
struct Args {
    /// shell to configure, asked for when absent
    #[pound(long)]
    shell: Option<Shell>,
}

struct Progress(Arc<AtomicUsize>);

impl Widget for Progress {
    fn render(&self, _ctx: &RenderCtx, out: &mut Surface) {
        out.write(format!("{} of 3", self.0.load(Ordering::Relaxed)), Style::PLAIN);
    }
}

fn run(context: &Context, args: &Args) -> climax::Result<()> {
    let shell = match args.shell {
        Some(shell) => shell,
        None => {
            match context
                .select("shell")
                .choice("bash", Shell::Bash)
                .choice("zsh", Shell::Zsh)
                .interact()?
            {
                PromptOutcome::Submit(shell) => shell,
                PromptOutcome::Leave => return Ok(()),
            }
        },
    };

    let done = Arc::new(AtomicUsize::new(0));
    let status = context
        .status("writing configuration")
        .spinner()
        .widget(widget(Progress(Arc::clone(&done))))
        .start();
    for _ in 0..3 {
        done.fetch_add(1, Ordering::Relaxed);
        status.mark_dirty()?;
    }
    status.finish()?;

    context
        .output()
        .result(&shell)
        .text(|shell| format!("{shell:?}"))
        .emit()
}

fn main() -> std::process::ExitCode {
    climax::main(|context, args: Args| run(&context, &args))
}

#[cfg(test)]
mod tests {
    use climax::bang::advanced::{
        Event,
        Key,
        scripted_interaction,
    };

    use super::*;

    #[test]
    fn a_scripted_prompt_drives_the_application() {
        let interaction = scripted_interaction([[Event::key(Key::Enter)]]);
        let context = Context::new().with_interaction(interaction);
        run(&context, &Args { shell: None }).unwrap();
    }
}
