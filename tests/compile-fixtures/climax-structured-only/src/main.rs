// SPDX-License-Identifier: EUPL-1.2

fn main() -> std::process::ExitCode {
    climax::main_with(|context| context.output().result(&42).text(|value| *value).emit())
}
