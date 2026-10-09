// SPDX-License-Identifier: EUPL-1.2

use pound::Parse as _;

#[derive(pound::Parse)]
struct Args {
    #[pound(long)]
    verbose: bool,
}

fn main() {
    let parsed = Args::try_parse_from(["--verbose"]).expect("fixture arguments parse");
    assert!(parsed.verbose);
}
