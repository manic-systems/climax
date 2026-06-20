// SPDX-License-Identifier: EUPL-1.2

use pound::Parse;

#[derive(Parse)]
struct Cli {
    #[pound(default_missing = "x")]
    file: Option<String>,
}

fn main() {}
