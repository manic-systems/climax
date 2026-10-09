// SPDX-License-Identifier: EUPL-1.2

#[derive(climax::Parse)]
struct Args {
    #[pound(long)]
    verbose: bool,
}

fn main() -> std::process::ExitCode {
    climax::main(|_context, args: Args| {
        let _ = args.verbose;
        Ok(())
    })
}
