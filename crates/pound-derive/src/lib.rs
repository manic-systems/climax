// SPDX-License-Identifier: EUPL-1.2

//! derive macros for pound. `#[derive(Parse)]` turns a struct into a flat
//! command and an enum into a subcommand tree, `#[derive(ValueEnum)]` wires a
//! unit enum up as a `FromArg` choice type. the expansion lives in
//! `pound-derive-impl`, rooted here at `::pound`.

use pound_derive_impl::Options;
use proc_macro::TokenStream;

const OPTIONS: Options = Options::new("::pound", cfg!(feature = "help"));

/// derives `Parse` for a struct (one command) or an enum (a subcommand tree).
///
/// `--version` prints the crate version, followed by a build hash when the
/// `POUND_GIT_HASH` environment variable is set while the deriving crate
/// compiles. rustc tracks it, so changing it rebuilds the crate. nothing sets
/// it by default, so a build script supplies it.
///
/// ```ignore
/// // build.rs
/// fn main() {
///     let out = std::process::Command::new("git")
///         .args(["rev-parse", "--short=12", "HEAD"])
///         .output();
///     if let Ok(out) = out {
///         let hash = String::from_utf8_lossy(&out.stdout);
///         println!("cargo:rustc-env=POUND_GIT_HASH={}", hash.trim());
///     }
///     println!("cargo:rerun-if-changed=.git/HEAD");
/// }
/// ```
#[proc_macro_derive(Parse, attributes(pound))]
pub fn derive_parse(input: TokenStream) -> TokenStream {
    pound_derive_impl::derive_parse(input.into(), &OPTIONS).into()
}

#[proc_macro_derive(ValueEnum, attributes(pound))]
pub fn derive_value_enum(input: TokenStream) -> TokenStream {
    pound_derive_impl::derive_value_enum(input.into(), &OPTIONS).into()
}
