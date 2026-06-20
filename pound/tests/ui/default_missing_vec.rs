// SPDX-License-Identifier: EUPL-1.2

use pound::Parse;

#[derive(Parse)]
struct Cli {
    #[pound(long, default_missing = "x")]
    tags: Vec<String>,
}

fn main() {}
