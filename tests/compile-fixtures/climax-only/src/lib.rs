// SPDX-License-Identifier: EUPL-1.2

//! Runs the README snippets against a package that depends on `climax` alone,
//! so a snippet naming `bang`, `pound`, `screw` or `serde` directly fails.

#[cfg(doctest)]
#[doc = include_str!("../../../../README.md")]
struct ReadmeDoctests;
