// SPDX-License-Identifier: EUPL-1.2

//! what to offer for the word under the cursor

use pound::{
    complete::{
        Candidate,
        Next,
        Partial,
    },
    spec::{
        ArgSpec,
        Command,
        CommandSpec,
        Hint,
        Kind,
    },
};

/// a command line cut at the cursor
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    words: Vec<String>,
    typed: String,
}

impl Line {
    /// `words` before the cursor without the program name, and the text
    /// `typed` so far for the word under it
    #[must_use]
    pub const fn new(words: Vec<String>, typed: String) -> Self {
        Self { words, typed }
    }

    /// what to offer for the word under the cursor, parsing the words before
    /// it against `spec`
    #[must_use]
    pub fn complete(&self, spec: &CommandSpec) -> Completions {
        let partial = Partial::parse(spec, self.words.iter().map(String::as_str));
        let mut out = Completions::default();

        match Cursor::new(&partial, &self.typed) {
            Cursor::Value { arg, lead, prefix } => out.values(&partial, arg, lead, prefix),
            Cursor::Option { prefix } => out.options(partial.command(), prefix),
            Cursor::Bare {
                positional,
                subcommands,
                options,
                prefix,
            } => {
                if subcommands {
                    out.subcommands(partial.command(), prefix);
                }
                if let Some(arg) = positional {
                    out.values(&partial, arg, "", prefix);
                }
                if options && !subcommands && positional.is_none() {
                    out.options(partial.command(), prefix);
                }
            },
        }

        out
    }
}

enum Cursor<'t> {
    /// an option's value, typed as its own word or after `--name=`, which is
    /// the `lead`
    Value {
        arg:    &'static ArgSpec,
        lead:   &'t str,
        prefix: &'t str,
    },
    Option {
        prefix: &'t str,
    },
    Bare {
        positional:  Option<&'static ArgSpec>,
        subcommands: bool,
        /// whether options can still follow, false after `--`
        options:     bool,
        prefix:      &'t str,
    },
}

impl<'t> Cursor<'t> {
    fn new(partial: &Partial<'_, '_>, typed: &'t str) -> Self {
        let (positional, subcommands, options) = match partial.next() {
            Next::Value(arg) => {
                return Self::Value {
                    arg,
                    lead: "",
                    prefix: typed,
                };
            },
            Next::Word {
                positional,
                subcommands,
                options,
                ..
            } => (positional, subcommands, options),
        };

        if options && let Some(long) = typed.strip_prefix("--") {
            if let Some((name, value)) = long.split_once('=')
                && let Some(arg) = partial.command().long(name).filter(|a| a.kind == Kind::Opt)
            {
                let lead = &typed[..typed.len() - value.len()];
                return Self::Value {
                    arg,
                    lead,
                    prefix: value,
                };
            }
            return Self::Option { prefix: typed };
        }
        if options && let Some(cluster) = typed.strip_prefix('-') {
            for (at, c) in cluster.char_indices() {
                match partial.command().short(c) {
                    Some(arg) if arg.kind == Kind::Opt => {
                        let (lead, prefix) = typed.split_at(1 + at + c.len_utf8());
                        return Self::Value { arg, lead, prefix };
                    },
                    Some(_) => {},
                    None => break,
                }
            }
            return Self::Option { prefix: typed };
        }

        Self::Bare {
            positional,
            subcommands,
            options,
            prefix: typed,
        }
    }
}

/// the answer to one completion request
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Completions {
    /// the words to offer, each starting with the text already typed
    pub candidates: Vec<Candidate>,
    pub fallback:   Fallback,
}

/// what the shell offers on top of the candidates
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Fallback {
    #[default]
    Nothing,
    /// file names, for a value nothing more is known about
    Files,
}

impl Completions {
    fn offer(&mut self, candidate: Candidate, prefix: &str) {
        if candidate.value.starts_with(prefix) && !candidate.value.contains(['\n', '\t']) {
            self.candidates.push(candidate);
        }
    }

    fn values(&mut self, partial: &Partial<'_, '_>, arg: &ArgSpec, before: &str, word: &str) {
        let cut = arg
            .delimiter
            .filter(|_| arg.multi)
            .and_then(|delim| word.rfind(delim).map(|at| at + delim.len_utf8()));
        let (done, prefix) = word.split_at(cut.unwrap_or(0));
        let lead = format!("{before}{done}");

        let found = match (arg.complete, arg.possible) {
            (Some(complete), _) => complete(partial, prefix),
            (None, Some(possible)) => {
                possible
                    .iter()
                    .filter(|c| !c.hidden)
                    .map(|c| Candidate::new(c.name).help(c.help))
                    .collect()
            },
            (None, None) if arg.hint == Hint::Path && lead.is_empty() => {
                self.fallback = Fallback::Files;
                return;
            },
            (None, None) if arg.hint == Hint::Path => {
                self.files(&lead, prefix);
                return;
            },
            (None, None) => return,
        };

        for candidate in found {
            let candidate =
                Candidate::new(format!("{lead}{}", candidate.value)).help(candidate.help);
            self.offer(candidate, &format!("{lead}{prefix}"));
        }
    }

    /// a shell cannot list these itself after `--name=`
    fn files(&mut self, lead: &str, prefix: &str) {
        let start = prefix.rsplit_once('/').map_or(prefix, |(_, start)| start);
        let (read, shown_dir) = match prefix.rsplit_once('/') {
            Some(("", _)) => ("/", "/".to_owned()),
            Some((dir, _)) => (dir, format!("{dir}/")),
            None => (".", String::new()),
        };
        let Ok(entries) = std::fs::read_dir(read) else {
            return;
        };

        let mut names = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let wanted =
                    name.starts_with(start) && (start.starts_with('.') || !name.starts_with('.'));
                let slash = if entry.path().is_dir() { "/" } else { "" };
                wanted.then(|| format!("{lead}{shown_dir}{name}{slash}"))
            })
            .collect::<Vec<_>>();
        names.sort();
        let typed = format!("{lead}{prefix}");
        for name in names {
            self.offer(Candidate::new(name), &typed);
        }
    }

    fn options(&mut self, command: &Command<'_>, prefix: &str) {
        for arg in command.options() {
            let longs = arg.long.into_iter().chain(arg.negate);
            for spelling in longs
                .map(|long| format!("--{long}"))
                .chain(arg.short.map(|c| format!("-{c}")))
            {
                self.offer(Candidate::new(spelling).help(arg.help), prefix);
            }
        }
    }

    fn subcommands(&mut self, command: &Command<'_>, prefix: &str) {
        for sub in command.subcommands().filter(|s| !s.hidden()) {
            let spec = sub.spec();
            self.offer(Candidate::new(spec.name).help(spec.about), prefix);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use pound::{
        Parse,
        ValueEnum,
    };

    use super::*;

    #[derive(ValueEnum)]
    enum Format {
        Raw,
        Json,
    }

    #[derive(Parse)]
    #[allow(dead_code, reason = "only the spec is read")]
    struct Cp {
        #[pound(long)]
        jobs:   Option<u32>,
        #[pound(short = 'F', long)]
        format: Option<Format>,
        from:   PathBuf,
    }

    fn complete(words: &[&str], typed: &str) -> Completions {
        let words = words.iter().map(|w| (*w).to_owned()).collect();
        Line::new(words, typed.to_owned()).complete(Cp::SPEC)
    }

    #[test]
    fn only_a_path_falls_back_to_files() {
        assert_eq!(complete(&["--jobs"], "").fallback, Fallback::Nothing);
        assert_eq!(complete(&[], "").fallback, Fallback::Files);
    }

    #[test]
    fn an_inline_value_keeps_its_option() {
        let values = complete(&[], "--format=j").candidates;
        assert_eq!(values, [Candidate::new("--format=json")]);
        let values = complete(&[], "-Fj").candidates;
        assert_eq!(values, [Candidate::new("-Fjson")]);
    }
}
