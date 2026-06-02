// SPDX-License-Identifier: EUPL-1.2

//! a non-trivial demo: a secrets-manager CLI that exercises the whole pound
//! surface. value enums, nested subcommands, groups, conflicts, trailing args,
//! a custom `FromArg`, and the override attributes all show up here.
//!
//! run `cargo run --example vault -- --help`

#![allow(dead_code, reason = "the demo just parses argv and prints the result")]

use std::time::Duration;

use pound::{
    FromArg,
    Parse,
    ValueEnum,
    ValueError,
};

// ── a custom value type ───────────────────────────────────────────────────────

/// a time-to-live like `30s`, `15m`, `2h`, `7d`, parsed into a `Duration`.
/// hand-implementing `FromArg` is all it takes to use a bespoke type as a field.
#[derive(Debug)]
struct Ttl(Duration);

impl FromArg for Ttl {
    fn from_arg(s: &str) -> Result<Self, ValueError> {
        let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
        let (num, unit) = s.split_at(split);
        let n: u64 = num
            .parse()
            .map_err(|_| ValueError::new(s, "expected <number><s|m|h|d>"))?;
        let secs = match unit {
            "" | "s" => n,
            "m" => n * 60,
            "h" => n * 3_600,
            "d" => n * 86_400,
            other => {
                return Err(ValueError::new(s, format!("unknown unit '{other}', use s/m/h/d")));
            },
        };
        Ok(Self(Duration::from_secs(secs)))
    }
}

// ── value enums ──────────────────────────────────────────────────────────────

/// output format
#[derive(ValueEnum, Debug)]
enum Format {
    Text,
    Json,
    Toml,
    Env,
}

/// secret type
#[derive(ValueEnum, Debug)]
enum Kind {
    Password,
    Token,
    Key,
    Certificate,
}

/// how to resolve conflicts when importing
#[derive(ValueEnum, Debug)]
enum OnConflict {
    Skip,
    Overwrite,
    Fail,
}

// ── nested subcommand tree ────────────────────────────────────────────────────

/// namespace management
#[derive(Parse, Debug)]
enum NsCmd {
    /// create a namespace
    Create {
        name: String,
        /// brief description
        #[pound(long)]
        desc: Option<String>,
    },
    /// list namespaces (exposed as `ls` via a variant name override)
    #[pound(name = "ls")]
    List {
        #[pound(long)]
        format: Option<Format>,
    },
    /// remove a namespace and all its secrets
    Rm {
        name: String,
        #[pound(short, long)]
        force: bool,
    },
    /// rename a namespace
    Rename { from: String, to: String },
}

// ── top-level subcommands ─────────────────────────────────────────────────────

/// the top-level subcommand
#[derive(Parse, Debug)]
enum Cmd {
    /// store or update a secret
    Set {
        key:   String,
        value: String,
        /// secret type hint (short overridden to -K so -k stays free)
        #[pound(short = 'K', long)]
        kind:  Option<Kind>,
        /// tag for grouping (repeatable)
        #[pound(short, long)]
        tag:   Vec<String>,
        /// expire after this long, e.g. 30m, 2h, 7d (custom `FromArg`)
        #[pound(long)]
        ttl:   Option<Ttl>,
        /// mark as read-only
        #[pound(long)]
        lock:  bool,
    },
    /// retrieve a secret
    Get {
        key: String,
        /// print in this format
        #[pound(short, long)]
        format: Option<Format>,
        /// copy to clipboard instead of printing (conflicts with --format)
        #[pound(short, long, conflicts_with = "format")]
        clip:   bool,
    },
    /// list secrets in the active namespace
    List {
        /// filter by tag
        #[pound(short, long)]
        tag: Option<String>,
        /// filter by kind
        #[pound(short, long)]
        kind: Option<Kind>,
        #[pound(short, long)]
        format: Option<Format>,
        /// show values (hidden by default)
        #[pound(long)]
        show: bool,
    },
    /// delete a secret (also reachable as `delete`)
    #[pound(alias = "delete")]
    Rm {
        key: String,
        #[pound(short, long)]
        force: bool,
    },
    /// import secrets from a file
    Import {
        /// file to read (explicit positional, shown as `<PATH>` via `value_name`)
        #[pound(positional, value_name = "PATH")]
        file: String,
        #[pound(short, long)]
        format: Option<Format>,
        /// how to handle existing keys
        #[pound(long, default = "skip")]
        on_conflict: OnConflict,
    },
    /// export secrets to a file or stdout (pick at most one destination)
    Export {
        #[pound(short, long)]
        format: Option<Format>,
        /// write to this file
        #[pound(short, long, group = "dest")]
        output: Option<String>,
        /// write to stdout
        #[pound(long, group = "dest")]
        stdout: bool,
        /// filter by tag
        #[pound(short, long)]
        tag: Option<String>,
        /// include locked secrets
        #[pound(long)]
        include_locked: bool,
    },
    /// run a command with the namespace's secrets in its environment
    Exec {
        /// the command and its args, everything after `--`
        #[pound(trailing)]
        command: Vec<String>,
    },
    /// manage namespaces
    Namespace {
        #[pound(subcommand)]
        cmd: NsCmd,
    },
    /// show the current auth identity (exposed as `whoami`)
    #[pound(name = "whoami")]
    WhoAmI,
    /// internal diagnostics, omitted from help
    #[pound(hidden)]
    Doctor,
}

// ── root ──────────────────────────────────────────────────────────────────────

/// a simple secrets manager
#[derive(Parse, Debug)]
#[pound(name = "vault", version = "0.1.0", required_group = "auth")]
struct Cli {
    /// unlock with this token
    #[pound(long, group = "auth")]
    token: Option<String>,

    /// unlock with this key file (exactly one auth method is required)
    #[pound(long, group = "auth")]
    key_file: Option<String>,

    /// active namespace (also accepts --ns)
    #[pound(short, long, alias = "ns", default = "default")]
    namespace: String,

    /// this doc line is replaced by the `help =` override in --help output
    #[pound(short = 'D', long = "database", env = "VAULT_DB", help = "path to the vault database file")]
    db: Option<String>,

    /// increase verbosity
    #[pound(short, long, count)]
    verbose: u8,

    /// do nothing, show what would happen
    #[pound(long)]
    dry_run: bool,

    /// dump internal state (unstable, hidden from help)
    #[pound(long, hidden)]
    debug_internals: bool,

    #[pound(subcommand)]
    cmd: Cmd,
}

fn main() {
    let cli = Cli::parse();
    if cli.verbose > 1 {
        eprintln!("{cli:?}");
    }
}
