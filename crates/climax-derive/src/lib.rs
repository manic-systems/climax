// SPDX-License-Identifier: EUPL-1.2

//! pound's `Parse` and `ValueEnum` derives, rooted at `::climax::pound` so an
//! application depending only on `climax` can use them. Re-exported from
//! `climax` behind its `derive` feature.

use pound_derive_impl::Options;
use proc_macro::TokenStream;

const OPTIONS: Options = Options::new("::climax::pound", true);

#[proc_macro_derive(Parse, attributes(pound))]
pub fn derive_parse(input: TokenStream) -> TokenStream {
    pound_derive_impl::derive_parse(input.into(), &OPTIONS).into()
}

#[proc_macro_derive(ValueEnum, attributes(pound))]
pub fn derive_value_enum(input: TokenStream) -> TokenStream {
    pound_derive_impl::derive_value_enum(input.into(), &OPTIONS).into()
}
