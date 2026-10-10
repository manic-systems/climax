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
    HttpServerError,
}

#[derive(Debug, PartialEq, ValueEnum)]
enum Empty {}

#[test]
fn spellings_follow_renames_and_kebab_case() {
    assert_eq!(Format::Json.as_str(), "json");
    assert_eq!(Format::Yaml.as_str(), "yml");
    assert_eq!(Format::PlainText.as_str(), "plain-text");
    assert_eq!(Format::HttpServerError.as_str(), "http-server-error");
}

#[test]
fn all_lists_variants_in_declaration_order() {
    assert_eq!(Format::ALL, [
        Format::Json,
        Format::Yaml,
        Format::PlainText,
        Format::HttpServerError
    ]);
    assert!(Empty::ALL.is_empty());
}

#[test]
fn every_spelling_parses_back_and_matches_possible_values() {
    for value in Format::ALL {
        assert_eq!(Format::from_arg(value.as_str()).unwrap(), *value);
    }
    let spellings: Vec<_> = Format::ALL.iter().map(Format::as_str).collect();
    assert_eq!(Format::POSSIBLE, Some(spellings.as_slice()));
}

impl core::fmt::Display for Format {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[test]
fn a_user_display_impl_does_not_collide() {
    assert_eq!(Format::PlainText.to_string(), "plain-text");
}
