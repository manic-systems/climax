// SPDX-License-Identifier: EUPL-1.2

use pound::Parse;

#[derive(Parse)]
struct Cli {
    #[pound(short, long, default_missing = "x")]
    force: bool,
}

fn main() {}
