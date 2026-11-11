// SPDX-License-Identifier: EUPL-1.2

//! climax-derive: pound's `Parse` and `ValueEnum` derives rooted at
//! `::climax::pound`, plus the `serde` attribute that roots serde's derives at
//! `::climax::serde`
//!
//! re-exported from `climax` behind its `derive` feature

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

/// derives `Parse` for a struct, which is one command, or an enum, which is a
/// subcommand tree
///
/// the attributes are pound's, written `#[pound(...)]`, and the expansion is
/// rooted at `::climax::pound`, so an application needs no `pound` dependency.
/// every attribute is listed on `climax::pound::Parse`, with the same field
/// and variant rules
///
/// this is the derive only, the trait it implements is `climax::pound::Parse`,
/// which `climax::prelude` brings into scope as `Parse as _` and a generic
/// bound names as `C: climax::pound::Parse`
///
/// | field                                             | meaning                        |
/// |---------------------------------------------------|--------------------------------|
/// | `bool`                                            | flag, true when present        |
/// | `T`                                               | required positional            |
/// | `Option<T>`                                       | optional positional            |
/// | `Vec<T>`                                          | variadic positional            |
/// | `T`, `Option<T>`, `Vec<T>` with `short` or `long` | option, optional, repeating    |
/// | integer with `count`                              | `-vvv` counted                 |
/// | `T` with `subcommand` or `flatten`                | subcommand tree, embedded args |
///
/// `T` is any `FromStr` type, a primitive, a `PathBuf` or a `ValueEnum`.
/// common attributes are `short`, `long`, `env = "NAME"`, `default = value`,
/// `global`, `alias`, `hidden` and `name = "other"`, and the doc comment on an
/// item is its help text
///
/// ```ignore
/// use climax::prelude::*;
///
/// /// ships the thing
/// #[derive(Parse)]
/// struct Args {
///     /// where to ship
///     #[pound(long, env = "SHIP_TARGET", default = "staging")]
///     target:  String,
///     /// print more
///     #[pound(short, count)]
///     verbose: u8,
///     /// files to ship
///     files:   Vec<String>,
/// }
/// ```
#[proc_macro_derive(Parse, attributes(pound))]
pub fn derive_parse(input: TokenStream) -> TokenStream {
    pound_derive_impl::derive_parse(input.into(), &OPTIONS).into()
}

/// derives a choice type from an enum of unit variants, so it can be a flag or
/// positional value
///
/// each variant is spelled in kebab case, `name = "other"` picks another
/// spelling and `alias = "other"` adds one. the type gains `ALL`, and `as_str`
/// from `ArgValue`, which `climax::prelude` brings into scope. it has no
/// `Display`
///
/// ```ignore
/// use climax::prelude::*;
///
/// #[derive(Clone, Copy, ValueEnum)]
/// enum Shell {
///     Bash,
///     #[pound(name = "nu")]
///     Nushell,
/// }
///
/// assert_eq!(Shell::Nushell.as_str(), "nu");
/// ```
#[proc_macro_derive(ValueEnum, attributes(pound))]
pub fn derive_value_enum(input: TokenStream) -> TokenStream {
    pound_derive_impl::derive_value_enum(input.into(), &OPTIONS).into()
}

/// roots serde's derives at `::climax::serde`, so a result type needs no
/// `serde` dependency
///
/// the arguments name the derives, `#[climax::serde(Serialize, Deserialize)]`,
/// and the attribute adds `#[serde(crate = "::climax::serde")]` for them. place
/// it above any other attribute that uses serde helper attributes, such as
/// `#[serde(rename_all = "kebab-case")]`, or rustc reports that `serde` is not
/// an attribute in scope
///
/// `#[derive(Serialize)]` does not resolve from the prelude, and
/// `#[derive(climax::serde::Serialize)]` fails with `E0658` about
/// `rustc_private`, so this attribute is the way to derive. json spells a
/// type as serde does, so a `ValueEnum` read as `lib` on the command line needs
/// the same `rename_all` to read `"lib"` in json
///
/// ```ignore
/// #[climax::serde(Serialize)]
/// #[serde(rename_all = "kebab-case")]
/// struct Report {
///     file_count: usize,
/// }
/// ```
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
            other @ (TokenTree::Group(_) | TokenTree::Punct(_) | TokenTree::Literal(_)) => {
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
    use core::str::FromStr as _;

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
