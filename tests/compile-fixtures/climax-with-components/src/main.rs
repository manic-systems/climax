#[derive(pound::Parse)]
struct Args {
    #[pound(long)]
    name: Option<String>,
}

fn main() -> climax::Result<()> {
    let _prompt = bang::text("name");
    let _rendered = screw::render_plain(&"component escape hatch");
    climax::run_with(Args { name: None }, |context, command| {
        let _ = (context.output_format(), command.name);
        Ok(())
    })
}
