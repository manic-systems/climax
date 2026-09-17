// SPDX-License-Identifier: EUPL-1.2

//! walk a program's `CommandSpec`
//! this demonstrates how a manpage/completions/etc generator might work

use std::fmt::Write as _;

use pound::{
    Parse,
    spec::{
        ArgSpec,
        CommandSpec,
    },
};

/// fetch urls to disk
#[derive(Parse)]
#[pound(name = "grab", version = "0.1.0")]
#[allow(dead_code, reason = "not a runnable example")]
struct Grab {
    /// download directory
    #[pound(short, long)]
    output: Option<String>,
    /// no output
    #[pound(long, global)]
    quiet:  bool,
    #[pound(subcommand)]
    cmd:    Option<Cmd>,
}

/// cache maintenance
#[derive(Parse)]
#[allow(dead_code, reason = "not a runnable example")]
enum Cmd {
    Fetch {
        /// urls to fetch
        url: Vec<String>,
    },
    /// list cached files
    List {
        /// output format
        #[pound(long)]
        format: Option<String>,
    },
    /// remove stale files
    Clean {
        /// remove all files
        #[pound(short, long)]
        all: bool,
    },
}

fn main() {
    print!("{}", walk(Grab::SPEC, 0, &[]));
}

/// print one command then recurse
/// all walkers must pass down their globals
fn walk(spec: &CommandSpec, depth: usize, inherited: &[&ArgSpec]) -> String {
    let pad = "  ".repeat(depth);
    let version = if spec.version.is_empty() {
        String::new()
    } else {
        format!(" {}", spec.version)
    };
    let mut output = format!("{pad}{}{version}  {}\n", spec.name, spec.about);

    for arg in spec.arguments().filter(|a| !a.hidden) {
        writeln!(output, "{pad}  {}", row(arg)).unwrap();
    }
    for arg in inherited.iter().filter(|arg| !arg.hidden) {
        writeln!(output, "{pad}  {}  [inherited global]", row(arg)).unwrap();
    }
    // help/version are accepted without living in `args`, and each spelling is
    // dropped on its own when the command claims it, so check them separately.
    for spelling in implicit(spec, inherited, 'h', "help") {
        writeln!(output, "{pad}  {spelling}  [implicit]").unwrap();
    }
    if spec.has_version_info() {
        for spelling in implicit(spec, inherited, 'V', "version") {
            writeln!(output, "{pad}  {spelling}  [implicit]").unwrap();
        }
    }

    // globals accumulate down the tree
    let mut globals = inherited.to_vec();
    globals.extend(spec.arguments().filter(|a| a.global));
    for sub in spec.subcommands().filter(|s| !s.hidden) {
        output.push_str(&walk(sub.spec, depth + 1, &globals));
    }
    output
}

/// whichever spellings of a builtin this command has not claimed for itself
fn implicit(spec: &CommandSpec, inherited: &[&ArgSpec], short: char, long: &str) -> Vec<String> {
    let mut out = Vec::new();
    if !spec
        .arguments()
        .chain(inherited.iter().copied())
        .any(|arg| arg.short == Some(short))
    {
        out.push(format!("-{short}"));
    }
    if !spec
        .arguments()
        .chain(inherited.iter().copied())
        .any(|arg| {
            arg.long == Some(long) || arg.aliases.contains(&long) || arg.negate == Some(long)
        })
    {
        out.push(format!("--{long}"));
    }
    out
}

/// a single arg's specification
/// should capture the switches and values it may take, etc
fn row(a: &ArgSpec) -> String {
    if a.is_positional() {
        let dots = if a.multi { "..." } else { "" };
        return format!("{}{dots}  {}", metavar(a), a.help);
    }
    let value = if a.takes_value() {
        format!(" <{}>", metavar(a))
    } else {
        String::new()
    };
    let switch = match (a.short, a.long) {
        (Some(s), Some(l)) => format!("-{s}, --{l}{value}"),
        (Some(s), None) => format!("-{s}{value}"),
        (None, Some(l)) => format!("--{l}{value}"),
        (None, None) => metavar(a),
    };
    format!("{switch}  {}", a.help)
}

/// placeholder for an arg's value
fn metavar(a: &ArgSpec) -> String {
    let name = if a.value_name.is_empty() {
        a.long.unwrap_or("arg")
    } else {
        a.value_name
    };
    name.to_uppercase()
}

#[cfg(test)]
mod tests {
    use pound::{
        Kind,
        SubSpec,
    };

    use super::*;

    #[test]
    fn commands_parse_without_urls_swallowing_their_names() {
        let grab = Grab::try_parse_from([
            "fetch",
            "https://example.com/a",
            "https://example.com/b",
            "--quiet",
        ])
        .unwrap();
        assert!(grab.quiet);
        let Some(Cmd::Fetch { url }) = grab.cmd else {
            panic!("expected fetch");
        };
        assert_eq!(url, ["https://example.com/a", "https://example.com/b"]);

        let grab = Grab::try_parse_from(["list", "--format=json"]).unwrap();
        let Some(Cmd::List { format }) = grab.cmd else {
            panic!("expected list");
        };
        assert_eq!(format.as_deref(), Some("json"));

        let grab = Grab::try_parse_from(["clean", "--all"]).unwrap();
        assert!(matches!(grab.cmd, Some(Cmd::Clean { all: true })));
        assert_eq!(
            Grab::SPEC
                .subcommands()
                .map(|sub| sub.name)
                .collect::<Vec<_>>(),
            ["fetch", "list", "clean"],
        );
    }

    #[test]
    fn hidden_globals_stay_hidden_in_child_output() {
        const CHILD: CommandSpec = CommandSpec::new("child");
        const ROOT: CommandSpec = CommandSpec::new("root")
            .args(&[
                ArgSpec::new(Kind::Flag).long("secret").global().hidden(),
                ArgSpec::new(Kind::Flag).long("visible").global(),
            ])
            .subs(&[SubSpec::new("child", &CHILD)]);
        let output = walk(&ROOT, 0, &[]);
        assert!(!output.contains("--secret"));
        assert!(output.contains("--visible    [inherited global]"));
    }

    #[test]
    fn inherited_aliases_and_negations_shadow_builtin_spellings() {
        const CHILD: CommandSpec = CommandSpec::new("child");
        const ALIAS: ArgSpec = ArgSpec::new(Kind::Flag)
            .long("assist")
            .aliases(&["help"])
            .global();
        const NEGATED: ArgSpec = ArgSpec::new(Kind::Flag)
            .long("normal")
            .negate("version")
            .global();
        const SHORT: ArgSpec = ArgSpec::new(Kind::Flag).short('h').global();
        assert_eq!(implicit(&CHILD, &[&ALIAS], 'h', "help"), ["-h"]);
        assert_eq!(implicit(&CHILD, &[&NEGATED], 'V', "version"), ["-V"]);
        assert_eq!(implicit(&CHILD, &[&SHORT], 'h', "help"), ["--help"]);
        assert!(implicit(&CHILD, &[&ALIAS, &SHORT], 'h', "help").is_empty());
    }
}
