// SPDX-License-Identifier: EUPL-1.2

#![cfg(feature = "derive")]

use pound::{
    ArgValue,
    FromArg,
    ValueEnum,
};

#[derive(Debug, Clone, Copy, PartialEq, ValueEnum)]
enum Format {
    Json,
    #[pound(name = "yml")]
    Yaml,
    PlainText,
}

#[derive(Debug, PartialEq, ValueEnum)]
enum Empty {}

#[test]
fn every_spelling_parses_back_and_matches_possible_values() {
    for value in Format::ALL {
        assert_eq!(Format::from_arg(value.as_str()).unwrap(), *value);
    }
    let spellings = Format::ALL.iter().map(Format::as_str).collect::<Vec<_>>();
    assert_eq!(Format::POSSIBLE, Some(spellings.as_slice()));
    assert!(Empty::ALL.is_empty());
}
