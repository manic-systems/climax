// SPDX-License-Identifier: EUPL-1.2

//! derive macros for pound. `#[derive(Parse)]` turns a struct into a flat
//! command and an enum into a subcommand tree, `#[derive(ValueEnum)]` wires a
//! unit enum up as a `FromArg` choice type. the expansion lives in
//! `pound-derive-impl`, rooted here at `::pound`.

use pound_derive_impl::Options;
use proc_macro::TokenStream;

const OPTIONS: Options = Options::new("::pound", cfg!(feature = "help"));

#[proc_macro_derive(Parse, attributes(pound))]
pub fn derive_parse(input: TokenStream) -> TokenStream {
    pound_derive_impl::derive_parse(input.into(), &OPTIONS).into()
}

#[proc_macro_derive(ValueEnum, attributes(pound))]
pub fn derive_value_enum(input: TokenStream) -> TokenStream {
    pound_derive_impl::derive_value_enum(input.into(), &OPTIONS).into()
}
