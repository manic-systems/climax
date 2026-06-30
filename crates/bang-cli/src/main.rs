// SPDX-License-Identifier: EUPL-1.2

fn main() {
    match bang::run_from_env() {
        Ok(output) => {
            print!("{output}");
        },
        Err(error) => {
            eprintln!("bang: {error}");
            std::process::exit(2);
        },
    }
}
