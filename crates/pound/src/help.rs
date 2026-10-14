// SPDX-License-Identifier: EUPL-1.2

//! help and version rendering.

#[cfg(feature = "help")] use core::fmt::Write as _;
use core::{
    fmt,
    iter,
};

#[cfg(not(feature = "std"))] use crate::alloc_prelude::*;
#[cfg(feature = "help")]
use crate::spec::{
    ArgSpec,
    Kind,
};
use crate::{
    spec::{
        Command,
        CommandSpec,
    },
    term::{
        Paint,
        Span,
        Term,
    },
};

/// a command's help page, laid out for whichever terminal prints it
///
/// `Display` gives the plain, unwrapped page. [`Help::render`] wraps it to a
/// [`Term`]'s width and paints it when the terminal takes color.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Help {
    blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(feature = "help"),
    expect(dead_code, reason = "a usage line is all it renders")
)]
enum Block {
    Text(String),
    Closing(String),
    Usage(Usage),
    Section {
        heading: String,
        rows:    Vec<Row>,
        /// aligns to its own column, not the one the other sections share
        own:     bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Row {
    left: Vec<Span>,
    help: String,
}

impl Row {
    fn width(&self) -> usize {
        self.left.iter().map(Span::width).sum()
    }
}

const NEXT_LINE_INDENT: usize = 10;
/// the narrowest help column worth keeping beside the left one
const NARROWEST: usize = 20;

impl Help {
    /// the page laid out for `term`
    #[must_use]
    pub fn render(&self, term: Term) -> String {
        let shared = self
            .blocks
            .iter()
            .filter_map(|block| {
                match block {
                    Block::Section {
                        rows, own: false, ..
                    } => Some(rows),
                    Block::Text(_)
                    | Block::Closing(_)
                    | Block::Usage(_)
                    | Block::Section { .. } => None,
                }
            })
            .flatten()
            .map(Row::width)
            .max()
            .unwrap_or(0);

        let mut out = String::new();
        for block in &self.blocks {
            match block {
                Block::Text(text) => {
                    push_wrapped(&mut out, text, 0, term.width());
                    out.push('\n');
                },
                Block::Closing(text) => {
                    out.push('\n');
                    push_wrapped(&mut out, text, 0, term.width());
                },
                Block::Usage(usage) => {
                    out.push_str(&usage.render(term));
                    out.push('\n');
                },
                Block::Section { heading, rows, own } => {
                    out.push('\n');
                    Span::new(format!("{heading}:"), Paint::Heading).write(&mut out, term);
                    out.push('\n');
                    let column = if *own {
                        rows.iter().map(Row::width).max().unwrap_or(0)
                    } else {
                        shared
                    };
                    for row in rows {
                        push_row(&mut out, row, column, term);
                    }
                },
            }
        }

        out.truncate(out.trim_end().len());
        out
    }
}

impl fmt::Display for Help {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render(Term::plain()))
    }
}

fn push_row(out: &mut String, row: &Row, column: usize, term: Term) {
    out.push_str("  ");
    for span in &row.left {
        span.write(out, term);
    }
    if row.help.is_empty() {
        out.push('\n');
        return;
    }

    let beside = 2 + column + 2;
    let indent = match term.width() {
        Some(width) if width.saturating_sub(beside) < NARROWEST => {
            out.push('\n');
            pad(out, NEXT_LINE_INDENT);
            NEXT_LINE_INDENT
        },
        _ => {
            pad(out, column - row.width() + 2);
            beside
        },
    };

    let mut first = true;
    for paragraph in row.help.split('\n') {
        if paragraph.is_empty() {
            out.push('\n');
            continue;
        }
        if !first {
            pad(out, indent);
        }
        first = false;
        push_wrapped(out, paragraph, indent, term.width());
    }
}

/// the caller writes the first line's indent, later lines get it here
fn push_wrapped(out: &mut String, text: &str, indent: usize, width: Option<usize>) {
    let room = width.map_or(usize::MAX, |w| w.saturating_sub(indent).max(1));
    for (i, line) in text.lines().enumerate() {
        if i > 0 {
            pad(out, indent);
        }

        let body = line.trim_start_matches(' ');
        let hang = line.len() - body.len();
        let mut filled = hang;
        pad(out, hang);
        let mut gap = 0;
        for (nth, word) in body.split(' ').enumerate() {
            gap += usize::from(nth > 0);
            if word.is_empty() {
                continue;
            }

            let len = Term::columns(word);
            if filled > hang && filled + gap + len > room {
                out.push('\n');
                pad(out, indent + hang);
                filled = hang;
            } else {
                pad(out, gap);
                filled += gap;
            }
            gap = 0;
            out.push_str(word);
            filled += len;
        }
        out.push('\n');
    }
}

fn pad(out: &mut String, columns: usize) {
    out.extend(iter::repeat_n(' ', columns));
}

/// a command's usage line, `Usage: grab [OPTION]... URL`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Usage {
    /// `grab fetch [OPTION]... URL`, without the `Usage:` lead
    line:    Box<str>,
    /// where the program, `grab fetch`, ends in `line`
    program: usize,
}

impl Usage {
    /// the line painted for `term`
    #[must_use]
    pub fn render(&self, term: Term) -> String {
        let mut out = String::new();
        Span::new("Usage:", Paint::Heading).write(&mut out, term);
        out.push(' ');
        let (program, args) = self.line.split_at(self.program);
        Span::literal(program).write(&mut out, term);
        out.push_str(args);
        out
    }
}

impl fmt::Display for Usage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render(Term::plain()))
    }
}

/// the full invocation of `cmd`, led by the names of the commands above it
fn program(cmd: &Command<'_>) -> String {
    let names = cmd.path().iter().copied().chain([cmd.spec().name]);
    names
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

const fn command_placeholder(spec: &CommandSpec) -> &'static str {
    if spec.subcommand_optional() {
        " [COMMAND]"
    } else {
        " COMMAND"
    }
}

pub(crate) fn version_line(spec: &CommandSpec, long: bool) -> String {
    let text = if (long || spec.version.is_empty()) && !spec.long_version.is_empty() {
        spec.long_version
    } else {
        spec.version
    };
    let (first, tail) = text
        .split_once('\n')
        .map_or((text, None), |(head, remainder)| (head, Some(remainder)));

    let mut out = spec.name.to_owned();
    if !first.is_empty() {
        out.push(' ');
        out.push_str(first);
    }
    if let Some(hash) = spec.hash {
        out.push_str(" (");
        out.push_str(hash);
        out.push(')');
    }
    if long && let Some(extra) = tail {
        out.push('\n');
        out.push_str(extra);
    }
    out
}

#[cfg(feature = "help")]
fn usage_positional(arg: &ArgSpec) -> String {
    let meta = arg.metavar();
    let dots = if arg.multi || arg.kind == Kind::Trailing {
        "..."
    } else {
        ""
    };
    if arg.required {
        format!("{meta}{dots}")
    } else {
        format!("[{meta}]{dots}")
    }
}

/// the `=VALUE` tail of a long option, bracketed when the value may be omitted
#[cfg(feature = "help")]
fn value_suffix(arg: &ArgSpec) -> String {
    let meta = arg.metavar();
    match several(arg) {
        Some(words) => format!(" {words}"),
        None if arg.value_may_be_left_out() => format!("[={meta}]"),
        None => format!("={meta}"),
    }
}

#[cfg(feature = "help")]
fn several(arg: &ArgSpec) -> Option<String> {
    let arity = arg.words_per_use()?;
    let meta = arg.metavar();
    let mut words = vec![meta.as_str(); arity.min()].join(" ");
    if arity.max() != Some(arity.min()) {
        words.push_str("...");
    }
    Some(words)
}

/// the long spelling, folding a `no-` negation into the `--[no-]name` form and
/// listing any other negation as a second spelling
#[cfg(feature = "help")]
fn long_form(arg: &ArgSpec, long: &str) -> Vec<Span> {
    match arg.negate {
        Some(negate) if negate.strip_prefix("no-") == Some(long) => {
            vec![Span::literal(format!("--[no-]{long}"))]
        },
        Some(negate) => {
            vec![
                Span::literal(format!("--{long}")),
                Span::plain(", "),
                Span::literal(format!("--{negate}")),
            ]
        },
        None => vec![Span::literal(format!("--{long}"))],
    }
}

#[cfg(feature = "help")]
fn invocation(arg: &ArgSpec) -> Vec<Span> {
    let takes_value = arg.kind == Kind::Opt;
    let mut spans = Vec::new();
    match (arg.short, arg.long) {
        (Some(short), Some(long)) => {
            spans.push(Span::literal(format!("-{short}")));
            spans.push(Span::plain(", "));
            spans.extend(long_form(arg, long));
            if takes_value {
                spans.push(Span::plain(value_suffix(arg)));
            }
        },
        (Some(short), None) => {
            spans.push(Span::literal(format!("-{short}")));
            if let Some(words) = several(arg) {
                spans.push(Span::plain(format!(" {words}")));
            } else if takes_value && arg.value_may_be_left_out() {
                spans.push(Span::plain(format!(" [{}]", arg.metavar())));
            } else if takes_value {
                spans.push(Span::plain(format!(" {}", arg.metavar())));
            }
        },
        (None, Some(long)) => {
            spans.push(Span::plain("    "));
            spans.extend(long_form(arg, long));
            if takes_value {
                spans.push(Span::plain(value_suffix(arg)));
            }
        },
        (None, None) => spans.push(Span::plain(arg.metavar())),
    }
    spans
}

#[cfg(feature = "help")]
fn help_text(arg: &ArgSpec, long: bool) -> String {
    let text = if long {
        arg.long_help.unwrap_or(arg.help)
    } else {
        arg.help
    };
    let choices = arg
        .possible
        .filter(|_| !arg.hide_possible)
        .unwrap_or_default()
        .iter()
        .filter(|choice| !choice.hidden)
        .collect::<Vec<_>>();
    let listed = long && choices.iter().any(|choice| !choice.help.is_empty());

    let mut rendered = text.to_owned();
    if !choices.is_empty() && !listed {
        let names = choices.iter().map(|choice| choice.name).collect::<Vec<_>>();
        push_tag(
            &mut rendered,
            format_args!("possible values: {}", names.join(", ")),
        );
    }
    let takes_value = matches!(arg.kind, Kind::Opt | Kind::Positional | Kind::Trailing);
    if takes_value && let Some(default) = arg.default.filter(|_| !arg.hide_default) {
        push_tag(&mut rendered, format_args!("default: {default}"));
    }
    #[cfg(feature = "std")]
    if let Some(env) = arg.env.filter(|_| !arg.hide_env) {
        push_tag(&mut rendered, format_args!("env: {env}"));
    }

    if listed {
        if !rendered.is_empty() {
            rendered.push_str("\n\n");
        }
        rendered.push_str("Possible values:");
        for choice in choices {
            let _reason = write!(rendered, "\n- {}", choice.name);
            if !choice.help.is_empty() {
                let _description_result = write!(rendered, ": {}", choice.help);
            }
        }
    }
    rendered
}

#[cfg(feature = "help")]
fn push_tag(out: &mut String, tag: fmt::Arguments<'_>) {
    if !out.is_empty() {
        out.push(' ');
    }
    let _reason = write!(out, "[{tag}]");
}

#[cfg(feature = "help")]
pub(crate) fn usage_line(cmd: &Command<'_>) -> Usage {
    let visible_args: Vec<&ArgSpec> = cmd.spec().arguments().filter(|arg| !arg.hidden).collect();
    usage(cmd, &visible_args, !cmd.inherited().is_empty())
}

#[cfg(feature = "help")]
fn usage(cmd: &Command<'_>, visible_args: &[&ArgSpec], has_globals: bool) -> Usage {
    let spec = cmd.spec();
    let mut args = String::new();
    if has_globals || visible_args.iter().any(|arg| !arg.is_positional()) {
        args.push_str(" [OPTION]...");
    }
    for arg in visible_args.iter().filter(|arg| arg.is_positional()) {
        args.push(' ');
        args.push_str(&usage_positional(arg));
    }
    if spec.subcommands().any(|sub| !sub.hidden || sub.external) {
        args.push_str(command_placeholder(spec));
    }
    let mut line = program(cmd);
    let program_end = line.len();
    line.push_str(&args);
    Usage {
        line:    line.into(),
        program: program_end,
    }
}

#[cfg(feature = "help")]
pub(crate) fn render(cmd: &Command<'_>, long: bool) -> Help {
    let spec = cmd.spec();
    let mut blocks = Vec::new();

    if !spec.before_help.is_empty() {
        blocks.push(Block::Text(spec.before_help.to_owned()));
    }

    let about = if long && !spec.long_about.is_empty() {
        spec.long_about
    } else {
        spec.about
    };
    if !about.is_empty() {
        blocks.push(Block::Text(about.to_owned()));
    }

    let visible_args: Vec<&ArgSpec> = spec.arguments().filter(|arg| !arg.hidden).collect();
    let globals = cmd
        .inherited()
        .iter()
        .map(|arg| {
            Row {
                left: invocation(arg),
                help: help_text(arg, long),
            }
        })
        .collect::<Vec<_>>();
    blocks.push(Block::Usage(usage(cmd, &visible_args, !globals.is_empty())));

    let mut commands = spec
        .subcommands()
        .filter(|sub| !sub.hidden)
        .map(|sub| {
            Row {
                left: vec![Span::literal(sub.name)],
                help: sub.about.to_owned(),
            }
        })
        .collect::<Vec<_>>();
    if let Some(sub) = cmd.help_command().filter(|_| !commands.is_empty()) {
        commands.push(Row {
            left: vec![Span::literal(sub.name)],
            help: sub.about.to_owned(),
        });
    }
    if !commands.is_empty() {
        blocks.push(Block::Section {
            heading: "Commands".to_owned(),
            rows:    commands,
            own:     true,
        });
    }

    let positionals = visible_args
        .iter()
        .filter(|arg| arg.is_positional())
        .map(|&arg| {
            Row {
                left: vec![Span::plain(usage_positional(arg))],
                help: help_text(arg, long),
            }
        })
        .collect::<Vec<_>>();
    if !positionals.is_empty() {
        blocks.push(Block::Section {
            heading: "Arguments".to_owned(),
            rows:    positionals,
            own:     false,
        });
    }

    let mut sections = vec![("Options", vec![])];
    for arg in cmd.own_options() {
        let heading = arg.heading.unwrap_or("Options");
        let row = Row {
            left: invocation(arg),
            help: help_text(arg, long),
        };
        match sections.iter_mut().find(|(name, _)| *name == heading) {
            Some((_, rows)) => rows.push(row),
            None => sections.push((heading, vec![row])),
        }
    }
    sections[0].1.extend(cmd.builtins().iter().map(|arg| {
        Row {
            left: invocation(arg),
            help: arg.help.to_owned(),
        }
    }));
    sections.push(("Global options", globals));

    blocks.extend(
        sections
            .into_iter()
            .filter(|(_, rows)| !rows.is_empty())
            .map(|(heading, rows)| {
                Block::Section {
                    heading: heading.to_owned(),
                    rows,
                    own: false,
                }
            }),
    );

    if !spec.after_help.is_empty() {
        blocks.push(Block::Closing(spec.after_help.to_owned()));
    }
    Help { blocks }
}

#[cfg(not(feature = "help"))]
pub(crate) fn usage_line(cmd: &Command<'_>) -> Usage {
    let args = if cmd.spec().has_subs() {
        command_placeholder(cmd.spec())
    } else {
        ""
    };
    let mut line = program(cmd);
    let program_end = line.len();
    line.push_str(args);
    Usage {
        line:    line.into(),
        program: program_end,
    }
}

#[cfg(not(feature = "help"))]
pub(crate) fn render(cmd: &Command<'_>, _long: bool) -> Help {
    Help {
        blocks: vec![Block::Usage(usage_line(cmd))],
    }
}

#[cfg(all(test, feature = "help", feature = "std"))]
mod tests {
    use super::*;
    use crate::Choice;

    #[test]
    fn defaults_and_env_follow_the_possible_values() {
        const MODES: &[Choice] = &[Choice::new("fast"), Choice::new("slow")];
        let mode = ArgSpec::new(Kind::Opt)
            .long("mode")
            .help("how to run")
            .possible(MODES)
            .default("fast")
            .env("RUN_MODE");
        let loud = ArgSpec::new(Kind::Flag).long("loud").env("LOUD");
        assert_eq!(
            help_text(&mode, false),
            "how to run [possible values: fast, slow] [default: fast] [env: RUN_MODE]"
        );
        assert_eq!(help_text(&loud, false), "[env: LOUD]");
    }

    #[test]
    fn help_wraps_beside_its_column() {
        let help = Help {
            blocks: vec![Block::Section {
                heading: "Options".to_owned(),
                rows:    vec![Row {
                    left: vec![Span::literal("--jobs")],
                    help: "how many downloads run at once".to_owned(),
                }],
                own:     false,
            }],
        };
        assert_eq!(
            help.render(Term::new(Some(30), false)),
            "\nOptions:\n  --jobs  how many downloads\n          run at once"
        );
    }
}
