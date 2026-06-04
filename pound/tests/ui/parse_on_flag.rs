// SPDX-License-Identifier: EUPL-1.2

use pound::Parse;

fn parse_bool(_: &str) -> Result<bool, &'static str> {
    Ok(true)
}

#[derive(Parse)]
struct Cli {
    #[pound(long, parse = "parse_bool")]
    flag: bool,
}

fn main() {}
