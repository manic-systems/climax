// SPDX-License-Identifier: EUPL-1.2

fn main() -> std::process::ExitCode {
    climax::main_with(|cx| cx.status("rendering only").spinner().finish())
}
