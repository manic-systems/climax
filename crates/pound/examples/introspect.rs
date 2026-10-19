// SPDX-License-Identifier: EUPL-1.2

//! walk a program's `CommandSpec`
//! this demonstrates how a manpage/completions/etc generator might work

#![expect(
    clippy::print_stdout,
    reason = "the example prints each command specification"
)]

use pound::{
    Parse,
    spec::{
        ArgSpec,
        Command,
    },
};

/// fetch urls to disk
#[derive(Parse)]
#[pound(name = "grab", version = "0.1.0")]
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
enum Cmd {
    /// download urls
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
    walk(&Command::root(Grab::SPEC), 0);
}

/// print one command then recurse
/// `Command` carries the inherited globals down
fn walk(cmd: &Command<'_>, depth: usize) {
    let spec = cmd.spec();
    let pad = "  ".repeat(depth);
    let version = if spec.version.is_empty() {
        String::new()
    } else {
        format!(" {}", spec.version)
    };
    println!("{pad}{}{version}  {}", spec.name, spec.about);

    for arg in spec.arguments().filter(|arg| !arg.hidden) {
        println!("{pad}  {}", row(arg));
    }
    for arg in cmd.inherited() {
        println!("{pad}  {}  [inherited global]", row(&arg));
    }
    for arg in cmd.builtins() {
        println!("{pad}  {}  [implicit]", row(&arg));
    }

    for sub in cmd.subcommands().filter(|child| !child.hidden()) {
        walk(&sub, depth + 1);
    }
}

/// a single arg's specification
/// should capture the switches and values it may take, etc
fn row(arg: &ArgSpec) -> String {
    if arg.is_positional() {
        let dots = if arg.multi { "..." } else { "" };
        return format!("{}{dots}  {}", metavar(arg), arg.help);
    }
    let value = if arg.takes_value() {
        format!(" <{}>", metavar(arg))
    } else {
        String::new()
    };
    let switch = match (arg.short, arg.long) {
        (Some(short), Some(long)) => format!("-{short}, --{long}{value}"),
        (Some(short), None) => format!("-{short}{value}"),
        (None, Some(long)) => format!("--{long}{value}"),
        (None, None) => metavar(arg),
    };
    format!("{switch}  {}", arg.help)
}

/// placeholder for an arg's value
fn metavar(arg: &ArgSpec) -> String {
    let name = if arg.value_name.is_empty() {
        arg.long.unwrap_or("arg")
    } else {
        arg.value_name
    };
    name.to_uppercase()
}
