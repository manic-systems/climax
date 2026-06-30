// SPDX-License-Identifier: EUPL-1.2

//! walk a program's `CommandSpec`
//! this demonstrates how a manpage/completions/etc generator might work

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
    /// urls to fetch
    url:    Vec<String>,
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
    walk(Grab::SPEC, 0, &[]);
}

/// print one command then recurse
/// all walkers must pass down their globals
fn walk(spec: &CommandSpec, depth: usize, inherited: &[&ArgSpec]) {
    let pad = "  ".repeat(depth);
    let version = if spec.version.is_empty() {
        String::new()
    } else {
        format!(" {}", spec.version)
    };
    println!("{pad}{}{version}  {}", spec.name, spec.about);

    for arg in spec.args.iter().filter(|a| !a.hidden) {
        println!("{pad}  {}", row(arg));
    }
    for arg in inherited {
        println!("{pad}  {}  [inherited global]", row(arg));
    }
    // help/version are accepted without living in `args`, unless overridden.
    if spec.find_long("help").is_none() && spec.find_short('h').is_none() {
        println!("{pad}  -h, --help  [implicit]");
    }
    if !spec.version.is_empty()
        && spec.find_long("version").is_none()
        && spec.find_short('V').is_none()
    {
        println!("{pad}  -V, --version  [implicit]");
    }

    // globals accumulate down the tree
    let mut globals = inherited.to_vec();
    globals.extend(spec.args.iter().filter(|a| a.global));
    for sub in spec.subs.iter().filter(|s| !s.hidden) {
        walk(sub.spec, depth + 1, &globals);
    }
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
