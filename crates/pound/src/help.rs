// SPDX-License-Identifier: EUPL-1.2

//! help and version rendering.

#[cfg(feature = "help")]
use core::fmt::Write as _;

#[cfg(not(feature = "std"))]
use crate::alloc_prelude::*;
use crate::spec::{ArgSpec, CommandSpec};
#[cfg(feature = "help")]
use crate::spec::{Kind, SubSpec};

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

#[cfg(feature = "help")]
fn metavar(a: &ArgSpec) -> String {
    let name = if a.value_name.is_empty() {
        a.long.unwrap_or("arg")
    } else {
        a.value_name
    };
    name.to_uppercase()
}

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

/// the `=VALUE` tail of a long option, bracketed when the value may be omitted
#[cfg(feature = "help")]
fn value_suffix(a: &ArgSpec) -> String {
    let meta = metavar(a);
    if a.default_missing.is_some() {
        format!("[={meta}]")
    } else {
        format!("={meta}")
    }
}

/// the long spelling, folding a `no-` negation into the `--[no-]name` form and
/// listing any other negation as a second spelling
#[cfg(feature = "help")]
fn long_form(a: &ArgSpec, long: &str) -> String {
    match a.negate {
        Some(negate) if negate.strip_prefix("no-") == Some(long) => format!("--[no-]{long}"),
        Some(negate) => format!("--{long}, --{negate}"),
        None => format!("--{long}"),
    }
}

#[cfg(feature = "help")]
fn invocation(a: &ArgSpec) -> String {
    let mut s = String::new();
    let takes_value = a.kind == Kind::Opt;
    match (a.short, a.long) {
        (Some(c), Some(l)) => {
            s.push('-');
            s.push(c);
            s.push_str(", ");
            s.push_str(&long_form(a, l));
            if takes_value {
                s.push_str(&value_suffix(a));
            }
        },
        (Some(c), None) => {
            s.push('-');
            s.push(c);
            if takes_value {
                s.push(' ');
                if a.default_missing.is_some() {
                    let _ = write!(s, "[{}]", metavar(a));
                } else {
                    s.push_str(&metavar(a));
                }
            }
        },
        (None, Some(l)) => {
            s.push_str("    ");
            s.push_str(&long_form(a, l));
            if takes_value {
                s.push_str(&value_suffix(a));
            }
        },
        (None, None) => s.push_str(&metavar(a)),
    }
    s
}

#[cfg(feature = "help")]
fn help_text(a: &ArgSpec, long: bool) -> String {
    let text = if long {
        a.long_help.unwrap_or(a.help)
    } else {
        a.help
    };
    let mut s = text.to_owned();
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

/// the listing for `-h`/`--help` or `-V`/`--version`. the parser answers each
/// spelling independently, so a command that takes `-h` for its own flag still
/// gets `--help`, and the row has to say so.
#[cfg(feature = "help")]
fn builtin_row(
    spec: &CommandSpec,
    short: char,
    long: &'static str,
    help: &'static str,
) -> Option<(String, String)> {
    let free_short = spec.find_short(short).is_none();
    let free_long = spec.find_long(long).is_none();
    if !free_short && !free_long {
        return None;
    }

    let mut a = ArgSpec::new(Kind::Flag);
    if free_short {
        a = a.short(short);
    }
    if free_long {
        a = a.long(long);
    }
    Some((invocation(&a), help.to_owned()))
}

#[cfg(feature = "help")]
pub(crate) fn usage_line(spec: &CommandSpec, globals: &[&ArgSpec]) -> String {
    let visible_args: Vec<&ArgSpec> = spec.args.iter().filter(|a| !a.hidden).collect();

    let mut out = String::from("Usage: ");
    out.push_str(spec.name);
    if visible_args.iter().any(|a| !a.is_positional()) || !globals.is_empty() {
        out.push_str(" [OPTION]...");
    }
    for a in visible_args.iter().filter(|a| a.is_positional()) {
        out.push(' ');
        out.push_str(&usage_positional(a));
    }
    if spec.subs.iter().any(|s| !s.hidden) {
        out.push_str(" COMMAND");
    }
    out
}

#[cfg(feature = "help")]
pub(crate) fn render(spec: &CommandSpec, globals: &[&ArgSpec], long: bool) -> String {
    let mut out = String::new();

    let about = if long && !spec.long_about.is_empty() {
        spec.long_about
    } else {
        spec.about
    };
    if !about.is_empty() {
        out.push_str(about);
        out.push_str("\n\n");
    }

    let visible_args: Vec<&ArgSpec> = spec.args.iter().filter(|a| !a.hidden).collect();
    let visible_subs: Vec<&SubSpec> = spec.subs.iter().filter(|s| !s.hidden).collect();

    out.push_str(&usage_line(spec, globals));
    out.push('\n');

    if !visible_subs.is_empty() {
        out.push_str("\nCommands:\n");
        let width = visible_subs.iter().map(|s| s.name.len()).max().unwrap_or(0);
        for s in &visible_subs {
            let _ = writeln!(out, "  {:<width$}  {}", s.name, s.about);
        }
    }

    let positionals: Vec<(String, String)> = visible_args
        .iter()
        .filter(|a| a.is_positional())
        .map(|&a| (usage_positional(a), help_text(a, long)))
        .collect();

    let mut sections = vec![("Options", vec![])];
    for &a in visible_args.iter().filter(|a| !a.is_positional()) {
        let heading = a.heading.unwrap_or("Options");
        let row = (invocation(a), help_text(a, long));
        match sections.iter_mut().find(|(name, _)| *name == heading) {
            Some((_, rows)) => rows.push(row),
            None => sections.push((heading, vec![row])),
        }
    }

    let builtins = &mut sections[0].1;
    if let Some(row) = builtin_row(spec, 'h', "help", "display this help and exit") {
        builtins.push(row);
    }
    if spec.has_version_info()
        && let Some(row) = builtin_row(
            spec,
            'V',
            "version",
            "output version information and exit",
        )
    {
        builtins.push(row);
    }

    let grows: Vec<(String, String)> = globals
        .iter()
        .filter(|a| !a.hidden)
        .map(|&a| (invocation(a), help_text(a, long)))
        .collect();

    // one width across every section, so the help column lines up throughout
    let width = positionals
        .iter()
        .chain(sections.iter().flat_map(|(_, rows)| rows))
        .chain(&grows)
        .map(|(left, _)| left.len())
        .max()
        .unwrap_or(0);

    if !positionals.is_empty() {
        out.push_str("\nArguments:\n");
        push_rows(&mut out, &positionals, width);
    }
    for (heading, rows) in &sections {
        if rows.is_empty() {
            continue;
        }
        let _ = write!(out, "\n{heading}:\n");
        push_rows(&mut out, rows, width);
    }
    if !grows.is_empty() {
        out.push_str("\nGlobal options:\n");
        push_rows(&mut out, &grows, width);
    }

    out.truncate(out.trim_end().len());
    out
}

#[cfg(feature = "help")]
fn push_rows(out: &mut String, rows: &[(String, String)], width: usize) {
    for (left, help) in rows {
        if help.is_empty() {
            let _ = writeln!(out, "  {left}");
            continue;
        }
        let mut paragraphs = help.split('\n');
        let first = paragraphs.next().unwrap_or_default();
        let _ = writeln!(out, "  {left:<width$}  {first}");
        for paragraph in paragraphs {
            if paragraph.is_empty() {
                out.push('\n');
            } else {
                let _ = writeln!(out, "  {:<width$}  {paragraph}", "");
            }
        }
    }
}

#[cfg(not(feature = "help"))]
pub(crate) fn usage_line(spec: &CommandSpec, _globals: &[&ArgSpec]) -> String {
    let mut out = format!("Usage: {}", spec.name);
    if spec.has_subs() {
        out.push_str(" COMMAND");
    }
    out
}

#[cfg(not(feature = "help"))]
pub(crate) fn render(spec: &CommandSpec, globals: &[&ArgSpec], _long: bool) -> String {
    usage_line(spec, globals)
}
