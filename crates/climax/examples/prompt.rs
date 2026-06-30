use climax::prelude::*;

fn main() -> climax::Result<()> {
    let shell = prompt::select("shell")
        .option("bash")
        .option("nushell")
        .option("zsh")
        .run()?;

    output::print_value(&shell, output::Format::Text)
}
