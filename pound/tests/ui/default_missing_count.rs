// SPDX-License-Identifier: EUPL-1.2

use pound::Parse;

#[derive(Parse)]
struct Cli {
    #[pound(short, long, count, default_missing = "1")]
    verbose: u8,
}

fn main() {}
