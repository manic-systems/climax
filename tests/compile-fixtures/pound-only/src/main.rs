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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flag_is_parsed() {
        assert!(Args::try_parse_from(["--verbose"]).unwrap().verbose);
        assert!(!Args::try_parse_from(Vec::<&str>::new()).unwrap().verbose);
    }

    #[test]
    fn the_program_name_is_not_an_argument() {
        assert!(Args::try_parse_from(["fixture", "--verbose"]).is_err());
    }
}
