// SPDX-License-Identifier: EUPL-1.2

use std::process::ExitCode;

fn main() -> ExitCode {
    climax::main_with(|cx| {
        cx.status("working")
            .spinner()
            .final_message("done")
            .finish()
    })
}
