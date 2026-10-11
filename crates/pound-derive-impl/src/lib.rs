// SPDX-License-Identifier: EUPL-1.2

//! expansion behind pound's derive macros, as a plain library so more than one
//! proc-macro crate can root the generated code at its own path to the pound
//! runtime. `derive_parse` turns a struct into a flat command and an enum into
//! a subcommand tree, `derive_value_enum` wires a unit enum up as a `FromArg`
//! choice type. all of it just emits the static `CommandSpec` plus a
//! `from_matches` reader, the runtime does the work.

mod attr;
mod emit;
mod model;

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use venial::parse_item;

use crate::{
    emit::Emitter,
    model::{
        Derive,
        ValueEnum,
    },
};

/// How the expansion refers to the runtime crate and whether it bakes help
/// text, decided by the proc-macro crate that wraps this library.
pub struct Options {
    /// Path to the `pound` runtime as seen from the deriving crate, such as
    /// `::pound`.
    pub root: &'static str,
    /// Bake doc comments and `#[pound(heading)]` into the expansion.
    pub help: bool,
}

impl Options {
    /// options rooting the expansion at `root`, with doc comments baked in
    /// when `help` is set
    #[must_use]
    pub const fn new(root: &'static str, help: bool) -> Self {
        Self { root, help }
    }
}

/// expands `#[derive(Parse)]` for the struct or enum in `input`, or yields a
/// `compile_error!` naming the problem. the attributes it reads are the ones
/// documented on `pound_derive::Parse`.
///
/// a crate that re-exports pound wraps it in its own proc-macro, so the
/// generated code names that crate's path to the runtime.
///
/// ```ignore
/// const OPTIONS: Options = Options::new("::climax::pound", cfg!(feature = "help"));
///
/// #[proc_macro_derive(Parse, attributes(pound))]
/// pub fn derive_parse(input: TokenStream) -> TokenStream {
///     pound_derive_impl::derive_parse(input.into(), &OPTIONS).into()
/// }
/// ```
pub fn derive_parse(input: TokenStream2, options: &Options) -> TokenStream2 {
    match parse_item(input) {
        Ok(item) => {
            Derive::new(&item).map_or_else(|e| err(&e), |d| Emitter::new(options).derive(&d))
        },
        Err(e) => e.to_compile_error(),
    }
}

/// expands `#[derive(ValueEnum)]` for the unit-variant enum in `input`, or
/// yields a `compile_error!` naming the problem. see [`derive_parse`] for how a
/// re-exporting crate wraps it.
pub fn derive_value_enum(input: TokenStream2, options: &Options) -> TokenStream2 {
    match parse_item(input) {
        Ok(item) => {
            ValueEnum::new(&item).map_or_else(|e| err(&e), |v| Emitter::new(options).value_enum(&v))
        },
        Err(e) => e.to_compile_error(),
    }
}

fn err(msg: &str) -> TokenStream2 {
    quote! { ::core::compile_error!(#msg); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_variants_with_one_spelling_are_rejected() {
        let input = r#"enum Mode { Fast, #[pound(name = "fast")] Quick }"#
            .parse()
            .unwrap();

        let out = derive_value_enum(input, &Options::new("::pound", true)).to_string();

        assert!(
            out.contains("`Fast` and `Quick` both spell `fast`"),
            "{out}"
        );
    }
}
