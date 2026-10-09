// SPDX-License-Identifier: EUPL-1.2

#[derive(pound::Parse)]
struct Args {
    #[pound(long)]
    name: Option<String>,
}

fn run(context: &climax::Context, command: &Args) -> climax::Result<()> {
    let _ = (context.output_format(), &command.name);
    let _status = context
        .status("direct screw widget")
        .widget(screw::widget("component escape hatch"));
    match context
        .select("shell")
        .choice("bash", "bash")
        .interact()?
    {
        climax::PromptOutcome::Submit(shell) => context.diagnostic().notice(shell),
        climax::PromptOutcome::Leave => Ok(()),
    }
}

fn main() -> climax::Result<()> {
    let _prompt = bang::text("name");
    let _rendered = screw::render_plain(&"component escape hatch");
    climax::run_with(Args { name: None }, |context, command| run(&context, &command))
}

#[cfg(test)]
mod tests {
    use bang::advanced::{
        Event,
        Key,
        scripted_interaction,
    };

    use super::*;

    #[test]
    fn direct_dependency_types_unify_with_climax() {
        let context = climax::Context::new()
            .with_interaction(scripted_interaction([[Event::key(Key::Enter)]]));
        run(&context, &Args { name: None }).unwrap();
    }
}
