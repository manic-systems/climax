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
    #[pound(name = "yml", alias = "yaml")]
    Yaml,
    PlainText,
    #[pound(hidden)]
    Debug,
}

#[derive(Debug, PartialEq, ValueEnum)]
enum Empty {}

#[test]
fn every_spelling_parses_back_and_matches_possible_values() {
    for value in Format::ALL {
        assert_eq!(Format::from_arg(value.as_str()).unwrap(), *value);
    }
    let spellings = Format::ALL.iter().map(Format::as_str).collect::<Vec<_>>();
    let choices = Format::POSSIBLE.unwrap_or_default();
    assert_eq!(
        choices.iter().map(|choice| choice.name).collect::<Vec<_>>(),
        spellings
    );
    assert_eq!(Format::from_arg("yaml").unwrap(), Format::Yaml);
    assert!(
        choices
            .iter()
            .any(|choice| choice.name == "debug" && choice.hidden)
    );
    assert!(Empty::ALL.is_empty());
}
