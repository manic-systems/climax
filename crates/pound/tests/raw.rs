// SPDX-License-Identifier: EUPL-1.2

//! the `args_from_raw` bridge: turning a libc-style `(argc, argv)` into the
//! borrowed `&str` iterator the parser consumes.

use core::ffi::{
    c_char,
    c_int,
};
use std::ffi::CString;

use pound::{
    ArgSpec,
    CommandSpec,
    Error,
    Kind,
    Matches,
    Parse,
};

/// hold the `CString`s alive and hand out a stable `*const *const c_char` over
/// them, mirroring what a libc runtime passes `main`.
fn raw_argv(args: &[&str]) -> (Vec<CString>, Vec<*const c_char>) {
    let owned: Vec<CString> = args.iter().map(|s| CString::new(*s).unwrap()).collect();
    let ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
    (owned, ptrs)
}

fn argc(ptrs: &[*const c_char]) -> c_int {
    c_int::try_from(ptrs.len()).expect("argv fits in c_int")
}

#[test]
fn bridges_argv_in_order_and_utf8() {
    let (_keep, ptrs) = raw_argv(&["prog", "--flag", "café", "x"]);
    // SAFETY: ptrs holds argc valid C strings, kept alive by `_keep`.
    let got: Vec<&str> = unsafe { pound::args_from_raw(argc(&ptrs), ptrs.as_ptr()) }.collect();
    assert_eq!(got, ["prog", "--flag", "café", "x"]);
}

#[test]
fn negative_argc_yields_nothing() {
    let (_keep, ptrs) = raw_argv(&["prog"]);
    // SAFETY: a non-positive argc reads no entries, so the pointer is untouched.
    let count = unsafe { pound::args_from_raw(-1, ptrs.as_ptr()) }.count();
    assert_eq!(count, 0);
}

// a minimal hand-built command, so the bridge can be driven end-to-end through
// `try_parse_from` exactly as a `no_std` entry point would.
struct Cmd {
    name: String,
    loud: bool,
}

static CMD_ARGS: &[ArgSpec] = &[
    ArgSpec::new(Kind::Flag).long("loud").short('l'),
    ArgSpec::new(Kind::Positional).value_name("name").required(),
];
static CMD_SPEC: CommandSpec = CommandSpec::new("cmd")
    .version("0.1.0")
    .about("raw-argv demo")
    .args(CMD_ARGS);

impl Parse for Cmd {
    const SPEC: &'static CommandSpec = &CMD_SPEC;

    fn from_matches(spec: &'static CommandSpec, m: &Matches<'_>) -> Result<Self, Error> {
        Ok(Self {
            loud: m.flag(0),
            name: m.required::<String>(spec, 1)?,
        })
    }
}

#[test]
fn drives_the_parser_skipping_argv0() {
    let (_keep, ptrs) = raw_argv(&["cmd", "--loud", "atagen"]);
    // SAFETY: ptrs holds argc valid C strings, kept alive by `_keep`.
    let args = unsafe { pound::args_from_raw(argc(&ptrs), ptrs.as_ptr()) }.skip(1);
    let cmd = Cmd::try_parse_from(args).unwrap();
    assert!(cmd.loud);
    assert_eq!(cmd.name, "atagen");
}
