// SPDX-License-Identifier: EUPL-1.2

//! help and version rendering.
//!
//! with the `help` feature on, [`render`] builds an aligned, sectioned screen
//! from the [`CommandSpec`]. with it off, help strings are not stored and
//! [`render`] degrades to a one-line usage string.

#[cfg(feature = "help")] use core::fmt::Write as _;

#[cfg(not(feature = "std"))] use crate::alloc_prelude::*;
use crate::spec::{
    ArgSpec,
    CommandSpec,
};
#[cfg(feature = "help")]
use crate::spec::{
    Kind,
    SubSpec,
};

/// `name x.y.z (hash)`, omitting any part that is not set.
pub(crate) fn version_line(spec: &CommandSpec) -> String {
    let mut out = spec.name.to_owned();
    if !spec.version.is_empty() {
        out.push(' ');
        out.push_str(spec.version);
    }
    if let Some(hash) = spec.hash {
        out.push_str(" (");
        out.push_str(hash);
        out.push(')');
    }
    out
}

/// uppercase metavar for an arg, gnu style: `value_name`, else the long name,
/// else ARG.
#[cfg(feature = "help")]
fn metavar(a: &ArgSpec) -> String {
    let name = if a.value_name.is_empty() {
        a.long.unwrap_or("arg")
    } else {
        a.value_name
    };
    name.to_uppercase()
}

/// the positional token for the usage line, gnu style: `NAME`, `[NAME]`,
/// `[NAME]...`.
#[cfg(feature = "help")]
fn usage_positional(a: &ArgSpec) -> String {
    let meta = metavar(a);
    let dots = if a.multi || a.kind == Kind::Trailing {
        "..."
    } else {
        ""
    };
    if a.required {
        format!("{meta}{dots}")
    } else {
        format!("[{meta}]{dots}")
    }
}

/// the left invocation column for an option row, gnu style: `-f, --force`,
/// `-o, --output=FILE`, or `    --long=VAL` when there is no short.
#[cfg(feature = "help")]
fn invocation(a: &ArgSpec) -> String {
    let mut s = String::new();
    let takes_value = a.kind == Kind::Opt;
    match (a.short, a.long) {
        (Some(c), Some(l)) => {
            s.push('-');
            s.push(c);
            s.push_str(", --");
            s.push_str(l);
            if takes_value {
                s.push('=');
                s.push_str(&metavar(a));
            }
        },
        (Some(c), None) => {
            s.push('-');
            s.push(c);
            if takes_value {
                s.push(' ');
                s.push_str(&metavar(a));
            }
        },
        (None, Some(l)) => {
            // pad where `-x, ` would be so longs line up
            s.push_str("    --");
            s.push_str(l);
            if takes_value {
                s.push('=');
                s.push_str(&metavar(a));
            }
        },
        (None, None) => s.push_str(&metavar(a)),
    }
    s
}

/// the description column for an arg: its help, then a possible-value list.
#[cfg(feature = "help")]
fn help_text(a: &ArgSpec) -> String {
    let mut s = a.help.to_owned();
    if let Some(values) = a.possible
        && !values.is_empty()
    {
        if !s.is_empty() {
            s.push(' ');
        }
        let _ = write!(s, "[possible values: {}]", values.join(", "));
    }
    s
}

#[cfg(feature = "help")]
pub(crate) fn render(spec: &CommandSpec, globals: &[&ArgSpec]) -> String {
    let mut out = String::new();

    if !spec.about.is_empty() {
        out.push_str(spec.about);
        out.push_str("\n\n");
    }

    let visible_args: Vec<&ArgSpec> = spec.args.iter().filter(|a| !a.hidden).collect();
    let visible_subs: Vec<&SubSpec> = spec.subs.iter().filter(|s| !s.hidden).collect();

    // usage line
    out.push_str("Usage: ");
    out.push_str(spec.name);
    if visible_args.iter().any(|a| !a.is_positional()) || !globals.is_empty() {
        out.push_str(" [OPTION]...");
    }
    for a in visible_args.iter().filter(|a| a.is_positional()) {
        out.push(' ');
        out.push_str(&usage_positional(a));
    }
    if !visible_subs.is_empty() {
        out.push_str(" COMMAND");
    }
    out.push('\n');

    // subcommands
    if !visible_subs.is_empty() {
        out.push_str("\nCommands:\n");
        let width = visible_subs.iter().map(|s| s.name.len()).max().unwrap_or(0);
        for s in &visible_subs {
            let _ = writeln!(out, "  {:<width$}  {}", s.name, s.about);
        }
    }

    // positionals
    let positionals: Vec<(String, String)> = visible_args
        .iter()
        .filter(|a| a.is_positional())
        .map(|&a| (usage_positional(a), help_text(a)))
        .collect();
    if !positionals.is_empty() {
        out.push_str("\nArguments:\n");
        push_rows(&mut out, &positionals);
    }

    // options
    out.push_str("\nOptions:\n");
    let mut rows: Vec<(String, String)> = visible_args
        .iter()
        .filter(|a| !a.is_positional())
        .map(|&a| (invocation(a), help_text(a)))
        .collect();
    if spec.find_short('h').is_none() && spec.find_long("help").is_none() {
        rows.push((
            "-h, --help".to_owned(),
            "display this help and exit".to_owned(),
        ));
    }
    if spec.has_version_info()
        && spec.find_short('V').is_none()
        && spec.find_long("version").is_none()
    {
        rows.push((
            "-V, --version".to_owned(),
            "output version information and exit".to_owned(),
        ));
    }
    push_rows(&mut out, &rows);

    let grows: Vec<(String, String)> = globals
        .iter()
        .filter(|a| !a.hidden)
        .map(|&a| (invocation(a), help_text(a)))
        .collect();
    if !grows.is_empty() {
        out.push_str("\nGlobal options:\n");
        push_rows(&mut out, &grows);
    }

    out.truncate(out.trim_end().len());
    out
}

#[cfg(feature = "help")]
fn push_rows(out: &mut String, rows: &[(String, String)]) {
    let width = rows.iter().map(|(l, _)| l.len()).max().unwrap_or(0);
    for (left, help) in rows {
        if help.is_empty() {
            let _ = writeln!(out, "  {left}");
        } else {
            let _ = writeln!(out, "  {left:<width$}  {help}");
        }
    }
}

#[cfg(not(feature = "help"))]
pub(crate) fn render(spec: &CommandSpec, _globals: &[&ArgSpec]) -> String {
    let mut out = format!("Usage: {}", spec.name);
    if spec.has_subs() {
        out.push_str(" COMMAND");
    }
    out
}
