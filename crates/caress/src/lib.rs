// SPDX-License-Identifier: EUPL-1.2

//! shell completions for [`pound`] commands
//!
//! call [`handle`] first thing in `main`. a shell's completion script runs the
//! program with `COMPLETE` set, and [`handle`] answers and exits before the
//! program does anything else. completion runs pound's own parser over the
//! line, so it agrees with what the program will accept, and an arg's
//! `#[pound(complete = ...)]` completer sees every other arg already typed.
//!
//! ```no_run
//! use pound::Parse;
//!
//! #[derive(Parse)]
//! struct Grab {
//!     url: Vec<String>,
//! }
//!
//! caress::handle(Grab::SPEC);
//! let grab = Grab::parse();
//! ```
//!
//! users register completion by sourcing the script the program prints when
//! `COMPLETE` names a shell and no words follow.
//!
//! | shell      | setup                                                                                                  |
//! |------------|--------------------------------------------------------------------------------------------------------|
//! | bash       | `source <(COMPLETE=bash grab)`                                                                         |
//! | zsh        | `COMPLETE=zsh grab > ~/.zfunc/_grab`, with it in `fpath`                                               |
//! | fish       | `COMPLETE=fish grab \| source`                                                                         |
//! | nu         | `COMPLETE=nu grab \| save grab.nu`, then `source` it                                                   |
//! | elvish     | `eval (COMPLETE=elvish grab \| slurp)`                                                                 |
//! | powershell | `$env:COMPLETE='powershell'; grab \| Out-String \| Invoke-Expression`, then `Remove-Item Env:COMPLETE` |
//!
//! packagers can write the same scripts at build time with [`Shell::script`]
//! and install them with the other completion files.
//!
//! # startup
//!
//! every TAB runs the program, so its startup is the completion latency.
//! answering takes caress about 10 µs. the rest is the process itself,
//! measured on x86-64 Linux at about 180 µs for exec and the Rust runtime,
//! plus about 350 µs more when the binary is dynamically linked, for the
//! loader to resolve glibc. two things keep it short.
//!
//! - call [`handle`] before anything else in `main`. config loading, logging
//!   setup, or a lock taken earlier lands on every TAB.
//! - link the binary statically when completion latency matters, with
//!   `RUSTFLAGS='-C target-feature=+crt-static'` or a musl target, or
//!   `pkgsStatic` in nix. that took a full bash TAB from 0.9 ms to 0.55 ms.

mod engine;
mod shell;

use std::{
    ffi::OsStr,
    io::Write as _,
    path::Path,
};

pub use engine::{
    Completions,
    Fallback,
    Line,
};
use pound::spec::CommandSpec;
pub use shell::{
    Shell,
    UnknownShell,
};

/// answer a completion request and exit, when `COMPLETE` names a shell
///
/// returns without doing anything when `COMPLETE` is unset, empty, or `0`,
/// see [`Request::from_env`].
pub fn handle(spec: &CommandSpec) {
    let Some(request) = Request::from_env(spec) else {
        return;
    };

    let code = match request {
        Ok(request) => {
            let answer = request.answer(spec);
            i32::from(std::io::stdout().write_all(answer.as_bytes()).is_err())
        },
        Err(err) => {
            let _ = writeln!(std::io::stderr(), "error: {err}");
            2
        },
    };
    std::process::exit(code);
}

/// what a shell asked the program for
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Request {
    /// the shell's registration script, for the name the program ran as
    Script { shell: Shell, program: String },
    /// candidates for the word under the cursor
    Complete { shell: Shell, line: Line },
}

impl Request {
    /// the request `COMPLETE` and the process arguments make, or `None` when
    /// `COMPLETE` is unset, empty, or `0`. a program run without a name is
    /// named after `spec`.
    #[must_use]
    pub fn from_env(spec: &CommandSpec) -> Option<Result<Self, UnknownShell>> {
        let shell = std::env::var_os("COMPLETE").filter(|v| !v.is_empty() && v != "0")?;
        let shell = match shell.to_string_lossy().parse::<Shell>() {
            Ok(shell) => shell,
            Err(err) => return Some(Err(err)),
        };

        let mut args = std::env::args_os();
        let program = args
            .next()
            .and_then(|arg0| program_name(&arg0))
            .unwrap_or_else(|| spec.name.to_owned());
        let rest = args.map(|arg| arg.to_string_lossy().into_owned());
        let cursor = std::env::var("COMPLETE_INDEX")
            .ok()
            .and_then(|i| i.parse().ok());
        Some(Ok(Self::from_args(shell, program, rest, cursor)))
    }

    /// the request in the arguments after the program name, a line after
    /// `--` and the script otherwise. the word at `cursor`, or else the last,
    /// is under the cursor.
    #[must_use]
    pub fn from_args(
        shell: Shell,
        program: String,
        args: impl IntoIterator<Item = String>,
        cursor: Option<usize>,
    ) -> Self {
        let mut args = args.into_iter();
        if args.next().as_deref() != Some("--") {
            return Self::Script { shell, program };
        }

        let mut words = args.collect::<Vec<_>>();
        let typed = match cursor {
            Some(at) => {
                let typed = words.get(at).cloned().unwrap_or_default();
                words.truncate(at);
                typed
            },
            None => words.pop().unwrap_or_default(),
        };
        Self::Complete {
            shell,
            line: Line::new(words, typed),
        }
    }

    /// what to print for the shell, completing against `spec`
    #[must_use]
    pub fn answer(&self, spec: &CommandSpec) -> String {
        match self {
            Self::Script { shell, program } => shell.script(program),
            Self::Complete { shell, line } => shell.render(&line.complete(spec)),
        }
    }
}

fn program_name(arg0: &OsStr) -> Option<String> {
    let name = Path::new(arg0).file_name()?.to_string_lossy();
    (!name.is_empty()).then(|| name.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_after_dashes_are_a_line_with_the_cursor_last() {
        let args = ["--", "open", "db"].map(String::from);
        let request = Request::from_args(Shell::Fish, "vault".to_owned(), args, None);
        let line = Line::new(vec!["open".to_owned()], "db".to_owned());
        assert_eq!(request, Request::Complete {
            shell: Shell::Fish,
            line,
        });
        let script = Request::from_args(Shell::Fish, "vault".to_owned(), [], None);
        assert!(matches!(script, Request::Script { .. }));
    }
}
