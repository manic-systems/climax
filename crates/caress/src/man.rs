// SPDX-License-Identifier: EUPL-1.2

//! mdoc manual pages, one per command
//!
//! the root command gets `<name>.1` and every visible subcommand gets
//! `<name>-<sub>.1`, the layout git and cargo use, so `man grab-fetch` works.

use core::fmt::{
    self,
    Write as _,
};
use std::collections::VecDeque;

use pound::spec::{
    ArgSpec,
    Command,
    CommandSpec,
    Kind,
};

/// one rendered manual page
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Page {
    /// the page name, `grab-fetch` for `grab fetch`
    pub name:    String,
    /// the mdoc source
    pub content: String,
}

impl Page {
    /// the file the page installs as, under `man1`
    #[must_use]
    pub fn file_name(&self) -> String {
        format!("{}.1", self.name)
    }
}

/// a page for `spec` and every visible subcommand below it. `date` goes on
/// each page as written, e.g. `October 11, 2026`, and is a parameter so a
/// reproducible build can pin it.
///
/// # Panics
///
/// when two commands would install the same page, such as `a b-c` and
/// `a b c`, which both name `a-b-c`
#[must_use]
pub fn pages(spec: &CommandSpec, date: &str) -> Vec<Page> {
    let mut out = Vec::new();
    let mut pending = VecDeque::from([Command::root(spec)]);
    while let Some(cmd) = pending.pop_front() {
        pending.extend(cmd.subcommands().filter(|sub| !sub.hidden()));
        let manual = Manual::new(cmd);
        let name = manual.name();
        assert!(
            out.iter().all(|page: &Page| page.name != name),
            "two commands would both install the man page {name}"
        );
        out.push(Page {
            content: manual.render(date),
            name,
        });
    }
    out
}

struct Manual<'m> {
    cmd:         Command<'m>,
    options:     Vec<ArgSpec>,
    positionals: Vec<&'static ArgSpec>,
}

impl<'m> Manual<'m> {
    fn new(cmd: Command<'m>) -> Self {
        Self {
            options: cmd.options(),
            positionals: cmd.positionals().collect(),
            cmd,
        }
    }

    fn words(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.cmd
            .path()
            .iter()
            .copied()
            .chain([self.cmd.spec().name])
    }

    fn name(&self) -> String {
        self.words().collect::<Vec<_>>().join("-")
    }

    fn render(&self, date: &str) -> String {
        let name = self.name();
        let mut page = Mdoc::default();

        page.line(format_args!("Dd {date}"));
        page.line(format_args!("Dt {} 1", name.to_uppercase()));
        page.line("Os");
        page.line("Sh NAME");
        page.line(format_args!("Nm {}", Word(&name)));
        let spec = self.cmd.spec();
        let about = [spec.about, spec.long_about]
            .into_iter()
            .find_map(|text| text.lines().find(|line| !line.trim().is_empty()))
            .unwrap_or_default()
            .trim_end_matches('.');
        page.line(format_args!("Nd {}", Word(about)));

        self.synopsis(&mut page);
        self.description(&mut page);
        self.commands(&mut page);
        self.environment(&mut page);
        self.see_also(&mut page, &name);
        page.out
    }

    fn synopsis(&self, page: &mut Mdoc) {
        page.line("Sh SYNOPSIS");
        let mut words = self.words();
        page.line(format_args!(
            "Nm {}",
            Word(words.next().unwrap_or_default())
        ));
        for word in words {
            page.line(format_args!("Cm {}", Word(word)));
        }

        let switches = self
            .options
            .iter()
            .filter(|a| matches!(a.kind, Kind::Flag | Kind::Count))
            .collect::<Vec<_>>();
        let bundle = switches.iter().filter_map(|a| a.short).collect::<String>();
        if !bundle.is_empty() {
            page.line(format_args!("Op Fl {}", Word(&bundle)));
        }
        for a in switches.iter().filter(|a| a.short.is_none()) {
            page.line(format_args!("Op {}", Flag(a)));
        }

        for a in self.options.iter().filter(|a| a.kind == Kind::Opt) {
            let op = if a.required { "" } else { "Op " };
            page.line(format_args!("{op}{} {}", Flag(a), Values(a)));
        }
        for a in &self.positionals {
            let dots = if a.multi || a.kind == Kind::Trailing {
                " ..."
            } else {
                ""
            };
            let op = if a.required { "" } else { "Op " };
            page.line(format_args!("{op}Ar {}{dots}", Meta(a)));
        }
        if self.cmd.spec().has_subs() {
            let op = if self.cmd.spec().subcommand_optional() {
                "Op "
            } else {
                ""
            };
            page.line(format_args!("{op}Ar command"));
        }
    }

    fn description(&self, page: &mut Mdoc) {
        page.line("Sh DESCRIPTION");
        let spec = self.cmd.spec();
        let about = if spec.long_about.is_empty() {
            spec.about
        } else {
            spec.long_about
        };
        page.block(spec.before_help);
        page.block(about);

        if !self.positionals.is_empty() {
            page.paragraph();
            page.text("The arguments are as follows:");
            page.list(&self.positionals, |page, a| {
                page.line(format_args!("It Ar {}", Meta(a)));
                page.describe(a);
            });
        }

        if !self.options.is_empty() {
            page.paragraph();
            page.text("The options are as follows:");
            page.list(&self.options, |page, a| {
                page.line(format_args!("It {}", Item(a)));
                page.describe(a);
            });
        }

        page.block(spec.after_help);
    }

    fn commands(&self, page: &mut Mdoc) {
        let mut subs = self
            .cmd
            .subcommands()
            .filter(|sub| !sub.hidden())
            .map(|sub| (sub.spec().name, "", sub.spec().about))
            .collect::<Vec<_>>();
        if subs.is_empty() {
            return;
        }
        if let Some(help) = self.cmd.help_command() {
            subs.push((help.name, " Op Ar command ...", help.about));
        }

        page.line("Sh COMMANDS");
        page.list(subs, |page, (name, operands, about)| {
            page.line(format_args!("It Cm {}{operands}", Word(name)));
            page.text(about);
        });
    }

    fn see_also(&self, page: &mut Mdoc, name: &str) {
        let parent = self.cmd.path().join("-");
        let children = self
            .cmd
            .subcommands()
            .filter(|sub| !sub.hidden())
            .map(|sub| format!("{name}-{}", sub.spec().name));
        let mut pages = (!parent.is_empty())
            .then_some(parent)
            .into_iter()
            .chain(children)
            .collect::<Vec<_>>();
        pages.sort_by_cached_key(|name| name.to_lowercase());
        if pages.is_empty() {
            return;
        }

        page.line("Sh SEE ALSO");
        let last = pages.len() - 1;
        for (i, name) in pages.iter().enumerate() {
            let comma = if i == last { "" } else { " ," };
            page.line(format_args!("Xr {} 1{comma}", Word(name)));
        }
    }

    fn environment(&self, page: &mut Mdoc) {
        let env = self
            .options
            .iter()
            .chain(self.positionals.iter().copied())
            .filter_map(|a| Some((a.env?, a)))
            .collect::<Vec<_>>();
        if env.is_empty() {
            return;
        }

        page.line("Sh ENVIRONMENT");
        page.list(env, |page, (var, a)| {
            page.line(format_args!("It Ev {}", Word(var)));
            page.text("Default for");
            match (a.long, a.short) {
                _ if a.is_positional() => page.line(format_args!("Ar {} .", Meta(a))),
                (Some(long), _) => page.line(format_args!("Fl \\-{} .", Word(long))),
                (None, Some(short)) => {
                    page.line(format_args!(
                        "Fl {} .",
                        Word(short.encode_utf8(&mut [0; 4]))
                    ));
                },
                (None, None) => page.line(format_args!("Ar {} .", Meta(a))),
            }
        });
    }
}

#[derive(Default)]
struct Mdoc {
    out: String,
}

impl Mdoc {
    /// its words must already be escaped
    fn line(&mut self, line: impl fmt::Display) {
        let _ = writeln!(self.out, ".{line}");
    }

    /// mandoc warns on a `Pp` straight after `Sh`
    fn paragraph(&mut self) {
        let fresh = self
            .out
            .lines()
            .last()
            .is_none_or(|last| last.starts_with(".Sh "));
        if !fresh {
            self.line("Pp");
        }
    }

    fn block(&mut self, text: &str) {
        if !text.trim().is_empty() {
            self.paragraph();
            self.text(text);
        }
    }

    /// guards a line starting with `.` or `'`, which mdoc reads as a request
    fn text(&mut self, text: &str) {
        let indent = |line: &str| line.len() - line.trim_start_matches([' ', '\t']).len();
        let flat = text
            .lines()
            .map(|line| if line.trim().is_empty() { "" } else { line })
            .collect::<Vec<_>>()
            .join("\n");
        let paragraphs = flat
            .split("\n\n")
            .map(|p| p.trim_matches('\n'))
            .filter(|p| !p.is_empty());
        for (nth, paragraph) in paragraphs.enumerate() {
            let lines = paragraph.lines().collect::<Vec<_>>();
            let mut margin = None;
            for (at, line) in lines.iter().enumerate() {
                match (margin, indent(line) > 0) {
                    (None, true) => {
                        self.line("Bd -literal -offset indent");
                        let run = lines[at..].iter().take_while(|next| indent(next) > 0);
                        margin = run.map(|next| indent(next)).min();
                    },
                    (Some(_), false) => {
                        self.line("Ed");
                        margin = None;
                    },
                    (None, false) if at == 0 && nth > 0 => self.line("Pp"),
                    _ => {},
                }

                let shown = match margin {
                    Some(width) => line[width..].trim_end(),
                    None => line.trim(),
                };
                if shown.starts_with(['.', '\'']) {
                    self.out.push_str("\\&");
                }
                self.out.push_str(&shown.replace('\\', "\\e"));
                self.out.push('\n');
            }
            if margin.is_some() {
                self.line("Ed");
            }
        }
    }

    fn list<T>(&mut self, items: impl IntoIterator<Item = T>, mut entry: impl FnMut(&mut Self, T)) {
        self.line("Bl -tag -width Ds");
        for item in items {
            entry(self, item);
        }
        self.line("El");
    }

    fn describe(&mut self, a: &ArgSpec) {
        self.text(a.long_help.unwrap_or(a.help));
        let choices = a
            .possible
            .unwrap_or_default()
            .iter()
            .filter(|c| !c.hidden)
            .collect::<Vec<_>>();
        if choices.iter().any(|c| !c.help.is_empty()) {
            self.line("Pp");
            self.text("One of");
            self.list(&choices, |page, choice| {
                page.line(format_args!("It Cm {}", Word(choice.name)));
                page.text(choice.help);
            });
        } else if let Some(last) = choices.len().checked_sub(1) {
            self.line("Pp");
            self.text("One of");
            for (i, choice) in choices.iter().enumerate() {
                let punct = if i == last { " ." } else { " ," };
                self.line(format_args!("Cm {}{punct}", Word(choice.name)));
            }
        }
        if let Some(default) = a.default.filter(|_| a.kind != Kind::Flag) {
            self.line("Pp");
            self.text("Defaults to");
            self.line(format_args!("Ql {} .", Word(default)));
        }
    }
}

/// one macro argument. quoting keeps spaces in one argument, and the `\&`
/// keeps a word that spells a macro name, like `Ar`, from being read as one.
struct Word<'w>(&'w str);

impl fmt::Display for Word<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let word = self.0;
        let quoted = word.contains(' ');
        let delimiter = matches!(
            word,
            "" | "(" | "[" | "." | "," | ":" | ";" | ")" | "]" | "?" | "!" | "|"
        );
        if quoted {
            f.write_char('"')?;
        } else if delimiter || word.len() <= 3 && word.starts_with(char::is_uppercase) {
            f.write_str("\\&")?;
        }
        for c in word.chars() {
            match c {
                '\\' => f.write_str("\\e")?,
                '"' => f.write_str("\\(dq")?,
                c => f.write_char(c)?,
            }
        }
        if quoted {
            f.write_char('"')?;
        }
        Ok(())
    }
}

struct Meta<'a>(&'a ArgSpec);

impl fmt::Display for Meta<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Word(&self.0.metavar().to_lowercase()).fmt(f)
    }
}

struct Values<'a>(&'a ArgSpec);

impl fmt::Display for Values<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some(arity) = self.0.words_per_use() else {
            return write!(f, "Ar {}", Meta(self.0));
        };

        for nth in 0..arity.min() {
            let gap = if nth > 0 { " " } else { "" };
            write!(f, "{gap}Ar {}", Meta(self.0))?;
        }
        if arity.max() != Some(arity.min()) {
            f.write_str(" No ...")?;
        }
        Ok(())
    }
}

struct Flag<'a>(&'a ArgSpec);

impl fmt::Display for Flag<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let a = self.0;
        match (a.short, a.long) {
            (Some(c), _) => write!(f, "Fl {}", Word(c.encode_utf8(&mut [0; 4]))),
            (None, Some(long)) => write!(f, "Fl \\-{}", Word(long)),
            (None, None) => write!(f, "Ar {}", Meta(a)),
        }
    }
}

struct Item<'a>(&'a ArgSpec);

impl fmt::Display for Item<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Spelling<'s> {
            Short(char),
            Long(&'s str),
        }

        let a = self.0;
        let shorts = a.short.map(Spelling::Short);
        let longs = a.long.into_iter().chain(a.negate).map(Spelling::Long);
        for (i, spelling) in shorts.into_iter().chain(longs).enumerate() {
            if i > 0 {
                f.write_str(" , ")?;
            }
            match spelling {
                Spelling::Short(c) => write!(f, "Fl {}", Word(c.encode_utf8(&mut [0; 4])))?,
                Spelling::Long(long) => write!(f, "Fl \\-{}", Word(long))?,
            }
            match (a.kind, spelling) {
                (Kind::Opt, Spelling::Long(_)) if a.words_per_use().is_none() => {
                    write!(f, " Ns = Ns Ar {}", Meta(a))?;
                },
                (Kind::Opt, _) => write!(f, " {}", Values(a))?,
                _ => {},
            }
        }
        Ok(())
    }
}
