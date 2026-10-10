// SPDX-License-Identifier: EUPL-1.2

//! pound's `Parse` and `ValueEnum` derives, rooted at `::climax::pound` so an
//! application depending only on `climax` can use them, and the `serde`
//! attribute that roots serde's derives at `::climax::serde`. Re-exported from
//! `climax` behind its `derive` feature.

use pound_derive_impl::Options;
use proc_macro::TokenStream;
use proc_macro2::{
    Span,
    TokenStream as TokenStream2,
    TokenTree,
};
use quote::{
    quote,
    quote_spanned,
};

const OPTIONS: Options = Options::new("::climax::pound", true);

#[proc_macro_derive(Parse, attributes(pound))]
pub fn derive_parse(input: TokenStream) -> TokenStream {
    pound_derive_impl::derive_parse(input.into(), &OPTIONS).into()
}

#[proc_macro_derive(ValueEnum, attributes(pound))]
pub fn derive_value_enum(input: TokenStream) -> TokenStream {
    pound_derive_impl::derive_value_enum(input.into(), &OPTIONS).into()
}

#[proc_macro_attribute]
pub fn serde(args: TokenStream, item: TokenStream) -> TokenStream {
    expand_serde(args.into(), &item.into()).into()
}

fn expand_serde(args: TokenStream2, item: &TokenStream2) -> TokenStream2 {
    match serde_derive_names(args) {
        Ok(names) => {
            quote! {
                #[derive(#(::climax::serde::#names),*)]
                #[serde(crate = "::climax::serde")]
                #item
            }
        },
        Err((span, message)) => quote_spanned! { span => compile_error!(#message); #item },
    }
}

fn serde_derive_names(args: TokenStream2) -> Result<Vec<proc_macro2::Ident>, (Span, String)> {
    let mut names = Vec::new();
    let mut tokens = args.into_iter();
    while let Some(token) = tokens.next() {
        match token {
            TokenTree::Ident(name) => names.push(name),
            other => {
                return Err((
                    other.span(),
                    "expected the name of a serde derive, such as `Serialize`".to_owned(),
                ));
            },
        }
        match tokens.next() {
            None => break,
            Some(TokenTree::Punct(comma)) if comma.as_char() == ',' => {},
            Some(other) => {
                return Err((
                    other.span(),
                    "expected `,` between serde derive names, which are plain names rather than \
                     paths"
                        .to_owned(),
                ));
            },
        }
    }
    if names.is_empty() {
        return Err((
            Span::call_site(),
            "expected at least one serde derive, such as `#[climax::serde(Serialize)]`".to_owned(),
        ));
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use super::*;

    fn expand(args: &str, item: &str) -> String {
        expand_serde(
            TokenStream2::from_str(args).unwrap(),
            &TokenStream2::from_str(item).unwrap(),
        )
        .to_string()
    }

    #[test]
    fn listed_names_become_climax_rooted_derives() {
        let out = expand(
            "Serialize, Deserialize,",
            "#[serde(rename_all = \"kebab-case\")] struct A;",
        );
        assert!(
            out.starts_with(
                "# [derive (:: climax :: serde :: Serialize , :: climax :: serde :: Deserialize)] \
                 # [serde (crate = \"::climax::serde\")] # [serde (rename_all = \"kebab-case\")] \
                 struct A ;"
            ),
            "{out}"
        );
    }

    #[test]
    fn an_empty_list_and_non_names_are_rejected_with_the_item_kept() {
        for args in [
            "",
            "Serialize Deserialize",
            "serde::Serialize",
            "1",
            "Serialize,,",
        ] {
            let out = expand(args, "struct A;");
            assert!(out.contains("compile_error"), "{args}: {out}");
            assert!(out.ends_with("struct A ;"), "{args}: {out}");
        }
    }
}
