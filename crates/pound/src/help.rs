// SPDX-License-Identifier: EUPL-1.2

//! help and version rendering.

#[cfg(feature = "help")]
use core::fmt::Write as _;

#[cfg(not(feature = "std"))] use crate::alloc_prelude::*;
use crate::spec::{
    ArgSpec,
    CommandSpec,
};
#[cfg(feature = "help")]
use crate::spec::{
    Kind,
    claims_long,
    claims_short,
};

/// the full invocation of `spec`, led by the names of the commands above it
fn program(spec: &CommandSpec, path: &[&str]) -> String {
    let mut out = String::new();
    for name in path {
        out.push_str(name);
        out.push(' ');
    }
    out.push_str(spec.name);
    out
}

const fn command_placeholder(spec: &CommandSpec) -> &'static str {
    if spec.subcommand_optional() {
        " [COMMAND]"
    } else {
        " COMMAND"
    }
}

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
fn usage_positional(a: &ArgSpec) -> String {
    let meta = a.metavar();
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
    let meta = a.metavar();
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
                    let _ = write!(s, "[{}]", a.metavar());
                } else {
                    s.push_str(&a.metavar());
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
        (None, None) => s.push_str(&a.metavar()),
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
    let takes_value = matches!(a.kind, Kind::Opt | Kind::Positional | Kind::Trailing);
    if takes_value && let Some(default) = a.default {
        push_tag(&mut s, format_args!("default: {default}"));
    }
    #[cfg(feature = "std")]
    if takes_value && let Some(env) = a.env {
        push_tag(&mut s, format_args!("env: {env}"));
    }
    s
}

#[cfg(feature = "help")]
fn push_tag(s: &mut String, tag: core::fmt::Arguments<'_>) {
    if !s.is_empty() {
        s.push(' ');
    }
    let _ = write!(s, "[{tag}]");
}

/// the listing for `-h`/`--help` or `-V`/`--version`. the parser answers each
/// spelling independently, so a command that takes `-h` for its own flag still
/// gets `--help`, and the row has to say so.
#[cfg(feature = "help")]
fn builtin_row(
    own: &[&ArgSpec],
    globals: &[&ArgSpec],
    short: char,
    long: &'static str,
    help: &'static str,
) -> Option<(String, String)> {
    let free_short = !claims_short(own, globals, short);
    let free_long = !claims_long(own, globals, long);
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

/// the inherited globals as this command shows them, each stripped of the
/// spellings that a nearer arg answers to instead
#[cfg(feature = "help")]
fn visible_globals(own: &[&ArgSpec], globals: &[&ArgSpec]) -> Vec<ArgSpec> {
    globals
        .iter()
        .enumerate()
        .filter(|(_, a)| !a.hidden)
        .filter_map(|(i, &&global)| {
            let nearer = &globals[i + 1..];
            let mut shown = global;
            shown.short = shown.short.filter(|&c| !claims_short(own, nearer, c));
            shown.long = shown.long.filter(|l| !claims_long(own, nearer, l));
            shown.negate = shown.negate.filter(|n| !claims_long(own, nearer, n));
            if shown.long.is_none() {
                shown.long = shown.negate.take();
            }
            (shown.short.is_some() || shown.long.is_some()).then_some(shown)
        })
        .collect()
}

#[cfg(feature = "help")]
pub(crate) fn usage_line(spec: &CommandSpec, path: &[&str], globals: &[&ArgSpec]) -> String {
    let own: Vec<&ArgSpec> = spec.arguments().collect();
    let visible_args: Vec<&ArgSpec> = own.iter().copied().filter(|a| !a.hidden).collect();
    usage(
        spec,
        path,
        &visible_args,
        !visible_globals(&own, globals).is_empty(),
    )
}

#[cfg(feature = "help")]
fn usage(
    spec: &CommandSpec,
    path: &[&str],
    visible_args: &[&ArgSpec],
    has_globals: bool,
) -> String {
    let mut out = String::from("Usage: ");
    out.push_str(&program(spec, path));
    if has_globals || visible_args.iter().any(|a| !a.is_positional()) {
        out.push_str(" [OPTION]...");
    }
    for a in visible_args.iter().filter(|a| a.is_positional()) {
        out.push(' ');
        out.push_str(&usage_positional(a));
    }
    if spec.subcommands().any(|s| !s.hidden) {
        out.push_str(command_placeholder(spec));
    }
    out
}

#[cfg(feature = "help")]
pub(crate) fn render(
    spec: &CommandSpec,
    path: &[&str],
    globals: &[&ArgSpec],
    long: bool,
) -> String {
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

    let own: Vec<&ArgSpec> = spec.arguments().collect();
    let visible_args: Vec<&ArgSpec> = own.iter().copied().filter(|a| !a.hidden).collect();
    let visible_subs = spec.subcommands().filter(|s| !s.hidden).collect::<Vec<_>>();
    let grows: Vec<(String, String)> = visible_globals(&own, globals)
        .iter()
        .map(|a| (invocation(a), help_text(a, long)))
        .collect();

    out.push_str(&usage(spec, path, &visible_args, !grows.is_empty()));
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
    if let Some(row) = builtin_row(&own, globals, 'h', "help", "Print help") {
        builtins.push(row);
    }
    if spec.has_version_info()
        && let Some(row) = builtin_row(
            &own,
            globals,
            'V',
            "version",
            "Print version",
        )
    {
        builtins.push(row);
    }

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
pub(crate) fn usage_line(spec: &CommandSpec, path: &[&str], _globals: &[&ArgSpec]) -> String {
    let mut out = format!("Usage: {}", program(spec, path));
    if spec.has_subs() {
        out.push_str(command_placeholder(spec));
    }
    out
}

#[cfg(not(feature = "help"))]
pub(crate) fn render(
    spec: &CommandSpec,
    path: &[&str],
    globals: &[&ArgSpec],
    _long: bool,
) -> String {
    usage_line(spec, path, globals)
}

#[cfg(all(test, feature = "help"))]
mod tests {
    use super::*;

    const ARGS: &[ArgSpec] = &[
        ArgSpec::new(Kind::Opt)
            .long("mode")
            .help("how to run")
            .possible(&["fast", "slow"])
            .default("fast")
            .env("RUN_MODE"),
        ArgSpec::new(Kind::Flag).long("loud").default("x").env("LOUD"),
        ArgSpec::new(Kind::Positional).value_name("path").default("."),
    ];
    const SPEC: CommandSpec = CommandSpec::new("run").args(ARGS);

    #[test]
    fn defaults_and_env_follow_the_possible_values() {
        let env = if cfg!(feature = "std") {
            " [env: RUN_MODE]"
        } else {
            ""
        };
        assert_eq!(
            help_text(&ARGS[0], false),
            format!("how to run [possible values: fast, slow] [default: fast]{env}")
        );
        assert_eq!(help_text(&ARGS[2], false), "[default: .]");
    }

    #[test]
    fn builtin_rows_use_the_clap_wording() {
        const VERSIONED: CommandSpec = CommandSpec::new("run").version("1.0");
        let text = render(&VERSIONED, &[], &[], false);
        assert!(text.contains("-h, --help") && text.contains("  Print help"), "{text}");
        assert!(text.contains("  Print version"), "{text}");
    }

    #[test]
    fn a_flag_shows_neither() {
        assert_eq!(help_text(&ARGS[1], false), "");
        assert!(!render(&SPEC, &[], &[], false).contains("LOUD"));
    }
}
