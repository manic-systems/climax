// SPDX-License-Identifier: EUPL-1.2

use std::process::exit;

#[expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "printing is the command line interface"
)]
fn main() {
    match bang::run_from_env() {
        Ok(output) => {
            print!("{output}");
        },
        Err(error) => {
            eprintln!("bang: {error}");
            exit(2);
        },
    }
}
