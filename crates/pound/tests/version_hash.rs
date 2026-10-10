// SPDX-License-Identifier: EUPL-1.2

#![cfg(feature = "derive")]

use pound::{
    ErrorKind,
    Parse,
};

#[derive(Debug, Parse)]
#[pound(name = "hashed")]
struct Args {
    #[pound(long)]
    verbose: bool,
}

#[test]
fn the_hash_comes_from_the_build_environment_only() {
    assert!(!Args::try_parse_from([]).unwrap().verbose);
    let ErrorKind::Version(line) = Args::try_parse_from(["--version"]).unwrap_err().kind else {
        panic!("expected a version line");
    };
    let version = env!("CARGO_PKG_VERSION");
    match option_env!("POUND_GIT_HASH") {
        Some(hash) => assert_eq!(line, format!("hashed {version} ({hash})")),
        None => assert_eq!(line, format!("hashed {version}")),
    }
}
