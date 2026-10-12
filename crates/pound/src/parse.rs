// SPDX-License-Identifier: EUPL-1.2

//! walks `argv` against a [`CommandSpec`] and produces [`Matches`]

use alloc::borrow::Cow;
use core::{
    convert::Infallible,
    iter::Peekable,
};

#[cfg(not(feature = "std"))] use crate::alloc_prelude::*;
use crate::{
    complete::Next,
    error::{
        Error,
        ErrorKind,
    },
    help,
    spec::{
        ArgSpec,
        Command,
        CommandSpec,
        Kind,
        SubSpec,
        claims_long,
        claims_short,
    },
    value::{
        FromArg,
        ValueError,
    },
};

/// what a single arg collected during a parse
#[derive(Default, Clone, Debug)]
struct Slot<'a> {
    /// count of user supplied invocations
    count:   u32,
    /// the last spelling seen was the arg's negation
    negated: bool,
    values:  Vec<&'a str>,
}

/// a successful parse
#[derive(Debug)]
pub struct Matches<'a> {
    slots:     Vec<Slot<'a>>,
    flattened: Vec<Self>,
    sub:       Option<(usize, Box<Self>)>,
    /// the deepest command this parse selected, for errors raised afterwards
    selected:  Option<Command<'static>>,
    /// what an external entry's name would have failed with
    unknown:   Option<Box<Error>>,
}

/// the command line an external subcommand took, the way `cargo foo` runs
/// `cargo-foo`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct External {
    /// the name it was called by
    pub name: String,
    /// every word after the name, flags included
    pub args: Vec<String>,
    unknown:  Error,
}

impl External {
    /// the subcommand `name` was probably meant to be, when one is close
    #[must_use]
    pub fn closest(&self) -> Option<&str> {
        self.unknown.kind.closest()
    }

    /// the error the parse would have raised with no external subcommand, for
    /// when nothing answers to `name`
    #[must_use]
    pub fn unknown(&self) -> Error {
        self.unknown.clone()
    }
}

/// an arg this command parses, and where its slot lives
struct ArgTarget {
    /// flattened indices leading from this command's matches to the owner's
    path:  Vec<usize>,
    index: usize,
    arg:   &'static ArgSpec,
}

/// the command a name selected, plus the flattened structs and spliced enums
/// between it and the command being parsed
struct SubTarget {
    path:     Vec<usize>,
    wrappers: Vec<(usize, &'static SubSpec)>,
    index:    usize,
    command:  &'static SubSpec,
}

impl SubTarget {
    fn find(spec: &CommandSpec, name: &str) -> Option<Self> {
        Self::find_by(spec, &|sub| {
            !sub.external && (sub.name == name || sub.aliases.contains(&name))
        })
    }

    fn find_external(spec: &CommandSpec) -> Option<Self> {
        Self::find_by(spec, &|sub| sub.external)
    }

    fn find_by(spec: &CommandSpec, wanted: &impl Fn(&SubSpec) -> bool) -> Option<Self> {
        fn search(
            subs: &'static [SubSpec],
            wanted: &impl Fn(&SubSpec) -> bool,
            chain: &mut Vec<(usize, &'static SubSpec)>,
        ) -> bool {
            for (i, sub) in subs.iter().enumerate() {
                chain.push((i, sub));
                let hit = if sub.flattened {
                    search(sub.spec.subs, wanted, chain)
                } else {
                    wanted(sub)
                };
                if hit {
                    return true;
                }
                chain.pop();
            }
            false
        }

        if spec.subs.is_empty() {
            return spec.flattened.iter().enumerate().find_map(|(i, inner)| {
                let mut target = Self::find_by(inner, wanted)?;
                target.path.insert(0, i);
                Some(target)
            });
        }
        let mut wrappers = Vec::new();
        if !search(spec.subs, wanted, &mut wrappers) {
            return None;
        }
        let (index, command) = wrappers.pop()?;
        Some(Self {
            path: Vec::new(),
            wrappers,
            index,
            command,
        })
    }

    /// each spliced enum reads its own level of `Matches`, so the selected
    /// command's matches get wrapped once per enum it was spliced through
    fn store<'a>(self, m: &mut Matches<'a>, selected: Matches<'a>) {
        let owner = self
            .path
            .iter()
            .fold(m, |owner, &i| &mut owner.flattened[i]);
        let mut sub = (self.index, Box::new(selected));
        for (i, wrapper) in self.wrappers.into_iter().rev() {
            let mut shell = Matches::new(wrapper.spec);
            shell.sub = Some(sub);
            sub = (i, Box::new(shell));
        }
        owner.sub = Some(sub);
    }
}

enum Halt {
    Error(Error),
    /// the words ran out where this arg wanted its value
    NeedsValue(&'static ArgSpec),
    /// the words ran out where this arg had its values but could take more,
    /// which is only news to a completion
    TakesMore(&'static ArgSpec),
}

impl From<ErrorKind> for Halt {
    fn from(kind: ErrorKind) -> Self {
        Self::Error(kind.into())
    }
}

type Words<'w, 'a> = Peekable<&'w mut dyn Iterator<Item = &'a str>>;

/// a global flag/option seen in a descendant
struct GlobalHit<'a> {
    /// position in the descendant's inherited globals, which every ancestor's
    /// own list is a prefix of
    index:   usize,
    value:   Option<&'a str>,
    negated: bool,
}

impl<'a> Matches<'a> {
    fn new(spec: &CommandSpec) -> Self {
        Self {
            slots:     vec![Slot::default(); spec.args.len()],
            flattened: spec
                .flattened
                .iter()
                .map(|inner| Self::new(inner))
                .collect(),
            sub:       None,
            selected:  None,
            unknown:   None,
        }
    }

    /// tag `err` with the usage line and help spelling of the deepest command
    /// selected, or of `root` when none was
    pub(crate) fn locate(&self, root: &CommandSpec, err: Error) -> Error {
        match &self.selected {
            Some(cmd) => cmd.locate(err),
            None => Command::root(root).locate(err),
        }
    }

    pub(crate) const fn selected(&self) -> Option<&Command<'static>> {
        self.selected.as_ref()
    }

    /// searches the deepest selected command first
    pub(crate) fn find_deepest(
        &self,
        spec: &CommandSpec,
        reaches: &impl Fn(&ArgSpec) -> bool,
    ) -> Option<(&'static ArgSpec, &Self, usize)> {
        if let Some((i, sub_m)) = &self.sub
            && let Some(found) = sub_m.find_deepest(spec.subs[*i].spec, reaches)
        {
            return Some(found);
        }
        let (selecting, idle) = spec
            .flattened
            .iter()
            .zip(&self.flattened)
            .partition::<Vec<_>, _>(|(_, inner_m)| selected_command(inner_m));
        if let Some(found) = selecting
            .into_iter()
            .chain(idle)
            .find_map(|(inner, inner_m)| inner_m.find_deepest(inner, reaches))
        {
            return Some(found);
        }

        let args: &'static [ArgSpec] = spec.args;
        let index = args.iter().position(reaches)?;
        Some((&args[index], self, index))
    }

    /// matches for the flattened struct at `index`
    #[must_use]
    pub fn flattened(&self, index: usize) -> &Self {
        &self.flattened[index]
    }

    /// was a flag (or any value) supplied at least once
    #[must_use]
    pub fn flag(&self, i: usize) -> bool {
        self.slots[i].count > 0
    }

    /// resolve a flag to its final state, honoring the negation spelling and
    /// falling back to the env var or default when neither was given
    pub fn switch(&self, spec: &CommandSpec, i: usize) -> Result<bool, Error> {
        let slot = &self.slots[i];
        if slot.negated {
            return Ok(false);
        }
        if slot.count > 0 {
            return Ok(true);
        }
        let word = |s: &str| {
            match s {
                "1" | "true" | "yes" => Ok(true),
                "" | "0" | "false" | "no" => Ok(false),
                _ => Err(ValueError::new(s, "expected 1, true, yes, 0, false or no")),
            }
        };
        parse_fallback(spec, i, &word).unwrap_or(Ok(false))
    }

    /// how many times a count flag was supplied
    #[must_use]
    pub fn count(&self, i: usize) -> u32 {
        self.slots[i].count
    }

    pub(crate) fn negated(&self, i: usize) -> bool {
        self.slots[i].negated
    }

    /// first raw command-line value, if any
    #[must_use]
    pub fn raw(&self, i: usize) -> Option<&'a str> {
        self.slots[i].values.first().copied()
    }

    /// all raw values, in order
    #[must_use]
    pub fn raws(&self, i: usize) -> &[&'a str] {
        &self.slots[i].values
    }

    /// chosen subcommand index and its matches, if one ran
    #[must_use]
    pub fn sub(&self) -> Option<(usize, &Self)> {
        self.sub.as_ref().map(|(i, m)| (*i, m.as_ref()))
    }

    /// the name and words an external subcommand took, read from its matches
    #[must_use]
    pub fn external(&self) -> External {
        let (name, args) = self.raws(0).split_first().unwrap_or((&"", &[]));
        let name = (*name).to_owned();
        let unknown = match &self.unknown {
            Some(err) => (**err).clone(),
            None => {
                ErrorKind::UnknownSubcommand {
                    name:    name.clone(),
                    closest: None,
                }
                .into()
            },
        };
        External {
            name,
            args: args.iter().map(|&s| s.to_owned()).collect(),
            unknown,
        }
    }

    /// read a required value as `T`
    pub fn required<T: FromArg>(&self, spec: &CommandSpec, i: usize) -> Result<T, Error> {
        self.required_map(spec, i, T::from_arg)
    }

    /// read an optional value as `Option<T>`
    pub fn optional<T: FromArg>(&self, spec: &CommandSpec, i: usize) -> Result<Option<T>, Error> {
        self.optional_map(spec, i, T::from_arg)
    }

    /// read every value into `Vec<T>`
    pub fn many<T: FromArg>(&self, spec: &CommandSpec, i: usize) -> Result<Vec<T>, Error> {
        self.many_map(spec, i, T::from_arg)
    }

    /// read every value into `Some(Vec<T>)`, or `None` when the arg was never
    /// given and has no fallback
    pub fn optional_many<T: FromArg>(
        &self,
        spec: &CommandSpec,
        i: usize,
    ) -> Result<Option<Vec<T>>, Error> {
        self.optional_many_map(spec, i, T::from_arg)
    }

    /// read a required value
    /// caller must supply conversion fn
    pub fn required_map<T>(
        &self,
        spec: &CommandSpec,
        i: usize,
        convert: impl Fn(&str) -> Result<T, ValueError>,
    ) -> Result<T, Error> {
        if let Some(s) = self.raw(i) {
            return parse_with(spec, i, s, None, &convert);
        }
        parse_fallback(spec, i, &convert)
            .unwrap_or_else(|| Err(ErrorKind::MissingRequired(spec.args[i].display_name()).into()))
    }

    /// read an optional value
    /// caller must supply conversion fn
    pub fn optional_map<T>(
        &self,
        spec: &CommandSpec,
        i: usize,
        convert: impl Fn(&str) -> Result<T, ValueError>,
    ) -> Result<Option<T>, Error> {
        if let Some(s) = self.raw(i) {
            return Ok(Some(parse_with(spec, i, s, None, &convert)?));
        }
        parse_fallback(spec, i, &convert).transpose()
    }

    /// read every value
    /// caller must supply conversion fn
    pub fn many_map<T>(
        &self,
        spec: &CommandSpec,
        i: usize,
        convert: impl Fn(&str) -> Result<T, ValueError>,
    ) -> Result<Vec<T>, Error> {
        let raws = self.raws(i);
        if !raws.is_empty() {
            return raws
                .iter()
                .map(|&s| parse_with(spec, i, s, None, &convert))
                .collect();
        }
        let Some(found) = fallback(spec, i) else {
            return Ok(Vec::new());
        };
        spec.args[i]
            .split(&found.value)
            .map(|s| parse_with(spec, i, s, found.env, &convert))
            .collect()
    }

    /// read every value, or `None` when the arg was never given and has no
    /// fallback
    /// caller must supply conversion fn
    pub fn optional_many_map<T>(
        &self,
        spec: &CommandSpec,
        i: usize,
        convert: impl Fn(&str) -> Result<T, ValueError>,
    ) -> Result<Option<Vec<T>>, Error> {
        if self.raws(i).is_empty() && fallback(spec, i).is_none() {
            return Ok(None);
        }
        self.many_map(spec, i, convert).map(Some)
    }
}

/// the value an arg takes when it was not given, and the env var it was read
/// from, if any
struct Fallback {
    value: Cow<'static, str>,
    env:   Option<&'static str>,
}

fn fallback(spec: &CommandSpec, i: usize) -> Option<Fallback> {
    #[cfg(feature = "std")]
    if let Some(var) = spec.args[i].env
        && let Ok(value) = std::env::var(var)
    {
        return Some(Fallback {
            value: Cow::Owned(value),
            env:   Some(var),
        });
    }
    spec.args[i].default.map(|value| {
        Fallback {
            value: Cow::Borrowed(value),
            env:   None,
        }
    })
}

fn parse_fallback<T>(
    spec: &CommandSpec,
    i: usize,
    convert: &impl Fn(&str) -> Result<T, ValueError>,
) -> Option<Result<T, Error>> {
    fallback(spec, i).map(|f| parse_with(spec, i, &f.value, f.env, convert))
}

fn parse_with<T>(
    spec: &CommandSpec,
    i: usize,
    s: &str,
    env: Option<&str>,
    convert: &impl Fn(&str) -> Result<T, ValueError>,
) -> Result<T, Error> {
    convert(s).map_err(|e| {
        let mut msg = e.msg;
        let listed = spec.args[i]
            .possible
            .unwrap_or_default()
            .iter()
            .filter(|c| !c.hidden)
            .map(|c| c.name)
            .collect::<Vec<_>>();
        if !listed.is_empty() {
            msg = format!("{msg} (possible values: {})", listed.join(", "));
        }
        ErrorKind::Value {
            arg: source_name(&spec.args[i], env),
            value: e.value,
            msg,
        }
        .into()
    })
}

fn source_name(a: &ArgSpec, env: Option<&str>) -> String {
    match env {
        Some(var) => format!("{} (from ${var})", a.display_name()),
        None => a.display_name(),
    }
}

/// entrypoint. parse `args[1..]` against `spec`
pub(crate) fn parse_spec<'a>(
    spec: &CommandSpec,
    args: impl IntoIterator<Item = &'a str>,
) -> Result<Matches<'a>, Error> {
    let mut args = args.into_iter();
    let mut it = (&mut args as &mut dyn Iterator<Item = &'a str>).peekable();
    let mut hits = Vec::new();
    parse_cmd::<Strict>(&Command::root(spec), None, &mut it, &mut hits).map(|(matches, _)| matches)
}

/// parse `args` as the start of a command line, skipping whatever does not
/// apply and checking no requirements, and say what the next word would be
pub(crate) fn probe_spec<'a>(
    spec: &CommandSpec,
    args: impl IntoIterator<Item = &'a str>,
) -> (Matches<'a>, Next) {
    let mut args = args.into_iter();
    let mut it = (&mut args as &mut dyn Iterator<Item = &'a str>).peekable();
    let mut hits = Vec::new();
    let Ok(walked) = parse_cmd::<Lenient>(&Command::root(spec), None, &mut it, &mut hits);
    walked
}

/// how a walk treats what it cannot apply
trait Mode {
    type Error;

    /// a token that could not be applied. an option still waiting on its value
    /// when the words ran out goes in `awaiting`.
    fn halt(halt: Halt, awaiting: &mut Option<&'static ArgSpec>) -> Result<(), Self::Error>;

    fn finish(cmd: &Command<'_>, m: &Matches<'_>, empty: bool) -> Result<(), Self::Error>;

    fn locate(cmd: &Command<'_>, err: Self::Error) -> Self::Error;

    fn show(cmd: &Command<'_>) -> Result<(), Self::Error>;
}

enum Strict {}

impl Mode for Strict {
    type Error = Error;

    fn halt(halt: Halt, _: &mut Option<&'static ArgSpec>) -> Result<(), Error> {
        match halt {
            Halt::Error(err) => Err(err),
            Halt::NeedsValue(a) => Err(ErrorKind::MissingValue(a.display_name()).into()),
            Halt::TakesMore(_) => Ok(()),
        }
    }

    fn finish(cmd: &Command<'_>, m: &Matches<'_>, empty: bool) -> Result<(), Error> {
        if empty && cmd.spec().help_if_empty {
            return Err(ErrorKind::NoArguments(help::render(cmd, false)).into());
        }
        Ok(finalise(cmd, m)?)
    }

    fn locate(cmd: &Command<'_>, err: Error) -> Error {
        cmd.locate(err)
    }

    fn show(cmd: &Command<'_>) -> Result<(), Error> {
        Err(ErrorKind::Help(help::render(cmd, true)).into())
    }
}

/// an unfinished line being completed, which skips what it cannot apply and
/// so cannot fail
enum Lenient {}

impl Mode for Lenient {
    type Error = Infallible;

    fn halt(halt: Halt, awaiting: &mut Option<&'static ArgSpec>) -> Result<(), Infallible> {
        if let Halt::NeedsValue(arg) | Halt::TakesMore(arg) = halt {
            *awaiting = Some(arg);
        }
        Ok(())
    }

    fn finish(_: &Command<'_>, _: &Matches<'_>, _: bool) -> Result<(), Infallible> {
        Ok(())
    }

    fn locate(_: &Command<'_>, err: Infallible) -> Infallible {
        err
    }

    fn show(_: &Command<'_>) -> Result<(), Infallible> {
        Ok(())
    }
}

/// parse one command, tagging whatever went wrong with this command's usage
/// line unless a nested command already claimed it. also says what the word
/// after the last one would be.
fn parse_cmd<'a, M: Mode>(
    cmd: &Command<'_>,
    lead: Option<&'a str>,
    it: &mut Words<'_, 'a>,
    hits: &mut Vec<GlobalHit<'a>>,
) -> Result<(Matches<'a>, Next), M::Error> {
    walk_cmd::<M>(cmd, lead, it, hits).map_err(|e| M::locate(cmd, e))
}

enum Step<'a> {
    Continue,
    Dispatch(SubTarget, &'a str),
    Help,
}

struct Walk<'w, 'a> {
    cmd:             &'w Command<'w>,
    targets:         &'w [ArgTarget],
    positionals:     Vec<&'w ArgTarget>,
    cursor:          usize,
    only_positional: bool,
    m:               Matches<'a>,
}

fn walk_cmd<'a, M: Mode>(
    cmd: &Command<'_>,
    lead: Option<&'a str>,
    it: &mut Words<'_, 'a>,
    hits: &mut Vec<GlobalHit<'a>>,
) -> Result<(Matches<'a>, Next), M::Error> {
    let mut targets = Vec::new();
    cmd.spec().visit(&mut Vec::new(), &mut |path, index, arg| {
        targets.push(ArgTarget {
            path: path.to_vec(),
            index,
            arg,
        });
    });

    let mut walk = Walk {
        cmd,
        targets: &targets,
        positionals: targets.iter().filter(|t| t.arg.is_positional()).collect(),
        cursor: 0,
        only_positional: false,
        m: Matches::new(cmd.spec()),
    };
    let mut awaiting = None;
    let mut empty = lead.is_none();
    if let Some(tok) = lead
        && let Err(err) = walk.positional(tok)
    {
        M::halt(err.into(), &mut awaiting)?;
    }

    let next = loop {
        let Some(tok) = it.next() else {
            break walk.next(awaiting);
        };
        empty = false;
        match walk.step(tok, it, hits) {
            Ok(Step::Continue) => {},
            Ok(Step::Dispatch(target, name)) => {
                break walk.dispatch::<M>(target, name, it, hits)?;
            },
            Ok(Step::Help) => break walk.help::<M>(it)?,
            Err(halt) => M::halt(halt, &mut awaiting)?,
        }
    };

    // must run before finalise so owned globals count toward required/group checks
    apply_global_hits(&targets, cmd.globals().len(), &mut walk.m, hits);
    M::finish(cmd, &walk.m, empty)?;
    Ok((walk.m, next))
}

impl<'a> Walk<'_, 'a> {
    fn step(
        &mut self,
        tok: &'a str,
        it: &mut Words<'_, 'a>,
        hits: &mut Vec<GlobalHit<'a>>,
    ) -> Result<Step<'a>, Halt> {
        if self.only_positional {
            self.positional(tok)?;
        } else if tok == "--" {
            self.only_positional = true;
        } else if let Some(long) = tok.strip_prefix("--") {
            self.long(tok, long, it, hits)?;
        } else if let Some(rest) = tok.strip_prefix('-').filter(|r| !r.is_empty()) {
            self.short(tok, rest, it, hits)?;
        } else if tok == "help"
            && self.cmd.help_command().is_some()
            && sub_open(self.cmd.spec(), &self.positionals, self.cursor)
        {
            return Ok(Step::Help);
        } else if let Some(target) =
            sub_dispatch(self.cmd.spec(), &self.positionals, self.cursor, tok)?
        {
            return Ok(Step::Dispatch(target, tok));
        } else {
            self.positional(tok)?;
        }
        Ok(Step::Continue)
    }

    fn long(
        &mut self,
        tok: &'a str,
        long: &'a str,
        it: &mut Words<'_, 'a>,
        hits: &mut Vec<GlobalHit<'a>>,
    ) -> Result<(), Halt> {
        let (name, inline) = match long.split_once('=') {
            Some((n, v)) => (n, Some(v)),
            None => (long, None),
        };
        if let Some(sig) = builtin_long(self.cmd, name) {
            return Err(sig.into());
        }

        if let Some(target) = self
            .targets
            .iter()
            .find(|t| t.arg.answers_long(name) && t.arg.negate != Some(name))
        {
            apply_named(&mut self.m, target, inline, it)?;
        } else if let Some(target) = self.targets.iter().find(|t| t.arg.negate == Some(name)) {
            apply_negation(target_slot_mut(&mut self.m, target), name, inline)?;
        } else if let Some((index, g)) = find_global_long(self.cmd.globals(), name) {
            if g.negate != Some(name) {
                record_global(index, g, inline, it, hits)?;
            } else if inline.is_some() {
                return Err(ErrorKind::UnexpectedValue(format!("--{name}")).into());
            } else {
                hits.push(GlobalHit {
                    index,
                    value: None,
                    negated: true,
                });
            }
        } else if self.takes_hyphen_value() {
            self.positional(tok)?;
        } else {
            return Err(ErrorKind::Unknown {
                arg:     format!("--{name}"),
                closest: closest_long(self.cmd, name),
            }
            .into());
        }
        Ok(())
    }

    fn short(
        &mut self,
        tok: &'a str,
        rest: &'a str,
        it: &mut Words<'_, 'a>,
        hits: &mut Vec<GlobalHit<'a>>,
    ) -> Result<(), Halt> {
        let (cmd, targets) = (self.cmd, self.targets);
        let builtin = |ch| builtin_short(cmd, ch);
        let first = rest.chars().next().unwrap_or('-');
        let arg_for = |ch| {
            targets
                .iter()
                .find(|t| t.arg.answers_short(ch))
                .map(|t| t.arg)
                .or_else(|| find_global_short(cmd.globals(), ch).map(|(_, g)| g))
        };
        let known = |ch| arg_for(ch).is_some() || builtin(ch).is_some();
        let cluster_known = || {
            for ch in rest.chars() {
                match arg_for(ch) {
                    Some(arg) if arg.kind == Kind::Opt => return true,
                    _ if known(ch) => {},
                    _ => return false,
                }
            }
            true
        };
        let hyphen_value = self.takes_hyphen_value();
        let options = if hyphen_value {
            cluster_known()
        } else {
            known(first)
        };
        if options {
            shorts(targets, &mut self.m, rest, it, cmd.globals(), hits, builtin)
        } else if first.is_ascii_digit() || first == '.' || hyphen_value {
            // a negative number is a value, not a flag
            Ok(self.positional(tok)?)
        } else {
            Err(unknown(format!("-{first}")).into())
        }
    }

    fn dispatch<M: Mode>(
        &mut self,
        target: SubTarget,
        name: &'a str,
        it: &mut Words<'_, 'a>,
        hits: &mut Vec<GlobalHit<'a>>,
    ) -> Result<Next, M::Error> {
        let child = self.cmd.child(target.command);
        let lead = target.command.external.then_some(name);
        let (mut sub_m, next) = parse_cmd::<M>(&child, lead, it, hits)?;
        if target.command.external {
            let err = unknown_subcommand(self.cmd.spec(), name).into();
            sub_m.unknown = Some(Box::new(self.cmd.locate(err)));
        }
        self.m.selected = sub_m.selected.take().or(Some(child));
        target.store(&mut self.m, sub_m);
        Ok(next)
    }

    /// a further `help` names the help command itself, so it is skipped
    fn help<M: Mode>(&mut self, it: &mut Words<'_, 'a>) -> Result<Next, M::Error> {
        let mut named = None::<Command<'static>>;
        for word in it.by_ref() {
            let at = named.as_ref().unwrap_or(self.cmd);
            if word == "help" && at.help_command().is_some() {
                continue;
            }
            let Some(target) = SubTarget::find(at.spec(), word) else {
                M::halt(unknown_subcommand(at.spec(), word).into(), &mut None)?;
                break;
            };
            named = Some(at.child(target.command));
        }

        let shown = named.as_ref().unwrap_or(self.cmd);
        M::show(shown)?;
        let next = Next::Word {
            positional:  None,
            subcommands: shown.spec().has_subs(),
            options:     false,
        };
        self.m.selected = named;
        Ok(next)
    }

    fn takes_hyphen_value(&self) -> bool {
        next_positional(&self.positionals, self.cursor).is_some_and(|t| t.arg.hyphen_values)
    }

    fn positional(&mut self, tok: &'a str) -> Result<(), ErrorKind> {
        if positional(&mut self.m, &self.positionals, &mut self.cursor, tok)? {
            self.only_positional = true;
        }
        Ok(())
    }

    fn next(&self, awaiting: Option<&'static ArgSpec>) -> Next {
        if let Some(arg) = awaiting {
            return Next::Value(arg);
        }
        Next::Word {
            positional:  next_positional(&self.positionals, self.cursor).map(|t| t.arg),
            subcommands: !self.only_positional
                && sub_open(self.cmd.spec(), &self.positionals, self.cursor),
            options:     !self.only_positional,
        }
    }
}

/// a bare token names a subcommand only once every required positional is
/// filled, and never past a variadic one, which stays greedy so its tail can
/// fill
fn sub_dispatch(
    spec: &CommandSpec,
    positionals: &[&ArgTarget],
    cursor: usize,
    tok: &str,
) -> Result<Option<SubTarget>, ErrorKind> {
    if !sub_open(spec, positionals, cursor) {
        return Ok(None);
    }
    match SubTarget::find(spec, tok) {
        Some(target) => Ok(Some(target)),
        None if cursor < positionals.len() => Ok(None), // an open optional slot can still take it
        None => {
            SubTarget::find_external(spec)
                .map(Some)
                .ok_or_else(|| unknown_subcommand(spec, tok))
        },
    }
}

fn unknown_subcommand(spec: &CommandSpec, tok: &str) -> ErrorKind {
    ErrorKind::UnknownSubcommand {
        name:    tok.to_owned(),
        closest: closest(
            spec.subcommands().filter(|s| !s.hidden).flat_map(sub_names),
            tok,
        ),
    }
}

fn sub_open(spec: &CommandSpec, positionals: &[&ArgTarget], cursor: usize) -> bool {
    let next = positionals.get(cursor).map(|t| t.arg);
    spec.has_subs()
        && !next.is_some_and(|a| a.multi || a.kind == Kind::Trailing)
        && !positionals.iter().skip(cursor).any(|t| t.arg.required)
}

/// the positional a bare word at `cursor` fills, which past the last one is
/// the variadic tail, if there is one
fn next_positional<'t>(positionals: &[&'t ArgTarget], cursor: usize) -> Option<&'t ArgTarget> {
    match positionals.get(cursor) {
        Some(&target) => Some(target),
        None => {
            positionals
                .last()
                .copied()
                .filter(|t| t.arg.multi || t.arg.kind == Kind::Trailing)
        },
    }
}

/// apply a long option
fn apply_named<'a>(
    m: &mut Matches<'a>,
    target: &ArgTarget,
    inline: Option<&'a str>,
    it: &mut Words<'_, 'a>,
) -> Result<(), Halt> {
    let a = target.arg;
    let slot = target_slot_mut(m, target);
    match a.kind {
        Kind::Flag => {
            if inline.is_some() {
                return Err(ErrorKind::UnexpectedValue(a.display_name()).into());
            }
            slot.count = 1;
            slot.negated = false;
        },
        Kind::Count => {
            if inline.is_some() {
                return Err(ErrorKind::UnexpectedValue(a.display_name()).into());
            }
            slot.count += 1;
        },
        Kind::Opt => {
            let value = match inline {
                Some(v) => v,
                None => detached_value(a, it)?,
            };
            push_value(a, slot, value);
            more_values(a, it, |more| push_value(a, slot, more))?;
        },
        Kind::Positional | Kind::Trailing => return Err(unknown(a.display_name()).into()),
    }
    Ok(())
}

/// apply a cluster of short args
fn shorts<'a>(
    targets: &[ArgTarget],
    m: &mut Matches<'a>,
    cluster: &'a str,
    it: &mut Words<'_, 'a>,
    globals: &[&'static ArgSpec],
    hits: &mut Vec<GlobalHit<'a>>,
    builtin: impl Fn(char) -> Option<ErrorKind>,
) -> Result<(), Halt> {
    for (off, ch) in cluster.char_indices() {
        if let Some(sig) = builtin(ch) {
            return Err(sig.into());
        }
        if let Some(target) = targets.iter().find(|t| t.arg.answers_short(ch)) {
            let a = target.arg;
            let slot = target_slot_mut(m, target);
            match a.kind {
                Kind::Flag => {
                    slot.count = 1;
                    slot.negated = false;
                },
                Kind::Count => slot.count += 1,
                Kind::Opt => {
                    let value = cluster_value(cluster, off, ch, it, a)?;
                    push_value(a, slot, value);
                    more_values(a, it, |more| push_value(a, slot, more))?;
                    return Ok(()); // option swallowed the cluster tail
                },
                Kind::Positional | Kind::Trailing => {
                    return Err(unknown(format!("-{ch}")).into());
                },
            }
        } else if let Some((index, g)) = find_global_short(globals, ch) {
            match g.kind {
                Kind::Flag | Kind::Count => {
                    hits.push(GlobalHit {
                        index,
                        value: None,
                        negated: false,
                    });
                },
                Kind::Opt => {
                    let value = cluster_value(cluster, off, ch, it, g)?;
                    record_values(index, g, value, it, hits)?;
                    return Ok(());
                },
                Kind::Positional | Kind::Trailing => {
                    return Err(unknown(format!("-{ch}")).into());
                },
            }
        } else {
            return Err(unknown(format!("-{ch}")).into());
        }
    }
    Ok(())
}

/// a short option's value
fn cluster_value<'a>(
    cluster: &'a str,
    off: usize,
    ch: char,
    it: &mut Words<'_, 'a>,
    a: &'static ArgSpec,
) -> Result<&'a str, Halt> {
    let rest = &cluster[off + ch.len_utf8()..];
    if rest.is_empty() {
        detached_value(a, it)
    } else {
        Ok(rest)
    }
}

/// the value for an option written without an attached one. an option that
/// declares `default_missing` takes that instead of eating the next token.
fn detached_value<'a>(a: &'static ArgSpec, it: &mut Words<'_, 'a>) -> Result<&'a str, Halt> {
    if let Some(missing) = a.default_missing {
        return Ok(missing);
    }
    it.next().ok_or(Halt::NeedsValue(a))
}

fn more_values<'a>(
    a: &'static ArgSpec,
    it: &mut Words<'_, 'a>,
    mut each: impl FnMut(&'a str),
) -> Result<(), Halt> {
    let Some(arity) = a.words_per_use() else {
        return Ok(());
    };

    let mut taken = 1;
    while arity.max().is_none_or(|max| taken < max) {
        let Some(word) =
            it.next_if(|word| *word != "--" && (a.hyphen_values || !looks_like_option(word)))
        else {
            break;
        };
        each(word);
        taken += 1;
    }

    let min = arity.min();
    let open = arity.max().is_none_or(|max| taken < max);
    match it.peek() {
        None if taken >= min && open => Err(Halt::TakesMore(a)),
        _ if taken >= min => Ok(()),
        None => Err(Halt::NeedsValue(a)),
        Some(_) => {
            Err(ErrorKind::TooFewValues {
                arg: a.display_name(),
                min,
                got: taken,
            }
            .into())
        },
    }
}

fn looks_like_option(word: &str) -> bool {
    word.strip_prefix('-').is_some_and(|rest| {
        !rest.is_empty() && !rest.starts_with(|ch: char| ch.is_ascii_digit() || ch == '.')
    })
}

/// record a global option's value, and those one use of it takes after it
fn record_values<'a>(
    index: usize,
    g: &'static ArgSpec,
    value: &'a str,
    it: &mut Words<'_, 'a>,
    hits: &mut Vec<GlobalHit<'a>>,
) -> Result<(), Halt> {
    let mut record = |word| {
        hits.push(GlobalHit {
            index,
            value: Some(word),
            negated: false,
        });
    };
    record(value);
    more_values(g, it, record)
}

fn push_value<'a>(a: &ArgSpec, slot: &mut Slot<'a>, value: &'a str) {
    if !a.multi {
        slot.values.clear(); // last wins for single-valued opts
    }
    slot.values.extend(a.split(value));
    slot.count += 1;
}

/// the nearest ancestor's global wins when several answer to the same spelling
fn find_global_long(globals: &[&'static ArgSpec], name: &str) -> Option<(usize, &'static ArgSpec)> {
    globals
        .iter()
        .copied()
        .enumerate()
        .rev()
        .find(|(_, a)| a.answers_long(name))
}

/// switch a flag back off, so the last spelling on the line wins
fn apply_negation(slot: &mut Slot<'_>, name: &str, inline: Option<&str>) -> Result<(), ErrorKind> {
    if inline.is_some() {
        return Err(ErrorKind::UnexpectedValue(format!("--{name}")));
    }
    slot.count = 0;
    slot.negated = true;
    Ok(())
}

fn find_global_short(globals: &[&'static ArgSpec], ch: char) -> Option<(usize, &'static ArgSpec)> {
    globals
        .iter()
        .copied()
        .enumerate()
        .rev()
        .find(|(_, a)| a.answers_short(ch))
}

fn record_global<'a>(
    index: usize,
    g: &'static ArgSpec,
    inline: Option<&'a str>,
    it: &mut Words<'_, 'a>,
    hits: &mut Vec<GlobalHit<'a>>,
) -> Result<(), Halt> {
    match g.kind {
        Kind::Flag | Kind::Count => {
            if inline.is_some() {
                return Err(ErrorKind::UnexpectedValue(g.display_name()).into());
            }
            hits.push(GlobalHit {
                index,
                value: None,
                negated: false,
            });
        },
        Kind::Opt => {
            let value = match inline {
                Some(v) => v,
                None => detached_value(g, it)?,
            };
            record_values(index, g, value, it, hits)?;
        },
        Kind::Positional | Kind::Trailing => return Err(unknown(g.display_name()).into()),
    }
    Ok(())
}

/// apply the hits this `spec` owns into its slots, leaving the rest to bubble
/// up
fn apply_global_hits<'a>(
    targets: &[ArgTarget],
    inherited: usize,
    m: &mut Matches<'a>,
    hits: &mut Vec<GlobalHit<'a>>,
) {
    hits.retain(|h| {
        let Some(target) = h
            .index
            .checked_sub(inherited)
            .and_then(|own| targets.iter().filter(|t| t.arg.global).nth(own))
        else {
            return true;
        };
        let a = target.arg;
        let slot = target_slot_mut(m, target);
        match a.kind {
            Kind::Flag => {
                slot.count = u32::from(!h.negated);
                slot.negated = h.negated;
            },
            Kind::Count => slot.count += 1,
            Kind::Opt => {
                if let Some(v) = h.value {
                    push_value(a, slot, v);
                }
            },
            Kind::Positional | Kind::Trailing => {},
        }
        false
    });
}

/// assign a bare token to the next positional, or a trailing/variadic sink,
/// returning whether a trailing arg took it and so owns the rest of the line
fn positional<'a>(
    m: &mut Matches<'a>,
    positionals: &[&ArgTarget],
    cursor: &mut usize,
    tok: &'a str,
) -> Result<bool, ErrorKind> {
    let Some(target) = next_positional(positionals, *cursor) else {
        return Err(ErrorKind::UnexpectedPositional(tok.to_owned()));
    };

    let a = target.arg;
    let slot = target_slot_mut(m, target);
    slot.values.extend(a.split(tok));
    slot.count += 1;
    // single positional advances the cursor, a variadic one keeps eating
    if !(a.multi || a.kind == Kind::Trailing) {
        *cursor += 1;
    }
    Ok(a.kind == Kind::Trailing)
}

/// enforce a list arg's arity. an absent arg with a fallback is held only to
/// `max_values`, by the values its delimiter splits the fallback into.
fn count_values(a: &ArgSpec, got: usize, fallback: Option<Fallback>) -> Result<(), ErrorKind> {
    if got == 0
        && let Some(found) = fallback
    {
        let got = a.split(&found.value).count();
        return match a.max_values {
            Some(max) if got > max => {
                Err(ErrorKind::TooManyValues {
                    arg: source_name(a, found.env),
                    max,
                    got,
                })
            },
            _ => Ok(()),
        };
    }
    if let Some(min) = a.min_values
        && got < min
    {
        return Err(ErrorKind::TooFewValues {
            arg: a.display_name(),
            min,
            got,
        });
    }
    if let Some(max) = a.max_values
        && got > max
    {
        return Err(ErrorKind::TooManyValues {
            arg: a.display_name(),
            max,
            got,
        });
    }
    Ok(())
}

/// whether an arg will have a value by the time it is read, counting an env
/// var only when it is actually set
fn supplied(spec: &CommandSpec, m: &Matches, i: usize) -> bool {
    m.slots[i].count > 0 || !m.slots[i].values.is_empty() || fallback(spec, i).is_some()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Demands {
    Checked,
    Waived,
}

/// enforce `required` and group constraints. defaults resolve when a value is
/// read, so a defaulted arg never counts as missing here.
fn finalise(cmd: &Command<'_>, m: &Matches) -> Result<(), ErrorKind> {
    let spec = cmd.spec();
    let owed = if exclusive_alone(spec, m)? {
        Demands::Waived
    } else {
        Demands::Checked
    };
    finalise_args(spec, m, owed)?;
    finalise_groups(spec, m, owed)?;
    let missing = spec.has_subs() && !selected_command(m) && !spec.subcommand_optional();
    if missing && owed == Demands::Checked {
        return Err(ErrorKind::MissingSubcommand {
            help: Some(help::render(cmd, false)),
        });
    }
    Ok(())
}

/// whether an exclusive arg was given, alone. given with another arg, it is
/// an error.
fn exclusive_alone(spec: &CommandSpec, m: &Matches) -> Result<bool, ErrorKind> {
    let mut given = Vec::new();
    spec.visit(&mut Vec::new(), &mut |path, index, arg| {
        let owner = path.iter().fold(m, |owner, &i| &owner.flattened[i]);
        if owner.slots[index].count > 0 || owner.slots[index].negated {
            given.push(arg);
        }
    });

    let Some(exclusive) = given.iter().find(|a| a.exclusive) else {
        return Ok(false);
    };
    match given.iter().find(|a| !core::ptr::eq(**a, *exclusive)) {
        Some(other) => {
            Err(ErrorKind::Exclusive {
                arg:   exclusive.display_name(),
                other: other.display_name(),
            })
        },
        None => Ok(true),
    }
}

/// whether a subcommand was chosen here or inside a flattened struct
fn selected_command(m: &Matches) -> bool {
    m.sub.is_some() || m.flattened.iter().any(selected_command)
}

fn finalise_args(spec: &CommandSpec, m: &Matches, demands: Demands) -> Result<(), ErrorKind> {
    for (i, a) in spec.args.iter().enumerate() {
        if !supplied(spec, m, i) && a.required && demands == Demands::Checked {
            return Err(ErrorKind::MissingRequired(a.display_name()));
        }
        count_values(a, m.slots[i].values.len(), fallback(spec, i))?;
    }

    for &(a, b) in spec.requires {
        if m.slots[a].count > 0 && !supplied(spec, m, b) {
            return Err(ErrorKind::Requires {
                arg:   spec.args[a].display_name(),
                needs: spec.args[b].display_name(),
            });
        }
    }

    for &(a, b) in spec.conflicts {
        if m.slots[a].count > 0 && m.slots[b].count > 0 {
            return Err(ErrorKind::Conflict {
                group:  String::new(),
                first:  spec.args[a].display_name(),
                second: spec.args[b].display_name(),
            });
        }
    }

    for (inner, inner_matches) in spec.flattened.iter().zip(&m.flattened) {
        finalise_args(inner, inner_matches, demands)?;
    }

    Ok(())
}

/// a group spans every flattened struct at a command level, so each name is
/// checked once against all its members, required if any declaration says so
fn finalise_groups(spec: &CommandSpec, m: &Matches, demands: Demands) -> Result<(), ErrorKind> {
    for (name, required) in command_groups(spec) {
        let members = group_members(spec, m, name);
        let set: Vec<String> = members
            .iter()
            .filter(|(_, slot)| slot.count > 0)
            .map(|(a, _)| a.display_name())
            .collect();
        if set.len() > 1 {
            return Err(ErrorKind::Conflict {
                group:  name.to_owned(),
                first:  set[0].clone(),
                second: set[1].clone(),
            });
        }
        if set.is_empty() && required && demands == Demands::Checked {
            let options = members
                .iter()
                .map(|(a, _)| a.display_name())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ErrorKind::MissingGroup {
                group: name.to_owned(),
                options,
            });
        }
    }
    Ok(())
}

fn command_groups(spec: &CommandSpec) -> Vec<(&'static str, bool)> {
    fn collect(spec: &CommandSpec, out: &mut Vec<(&'static str, bool)>) {
        for group in spec.groups {
            match out.iter_mut().find(|(name, _)| *name == group.name) {
                Some((_, required)) => *required |= group.required,
                None => out.push((group.name, group.required)),
            }
        }
        for inner in spec.flattened {
            collect(inner, out);
        }
    }

    let mut out = Vec::new();
    collect(spec, &mut out);
    out
}

/// the args in `group` across `spec` and its flattened structs, with their
/// slots
fn group_members<'m, 'a>(
    spec: &CommandSpec,
    m: &'m Matches<'a>,
    group: &str,
) -> Vec<(&'static ArgSpec, &'m Slot<'a>)> {
    let mut members = Vec::new();
    spec.visit(&mut Vec::new(), &mut |path, index, arg| {
        if arg.group == Some(group) {
            let owner = path.iter().fold(m, |owner, &i| &owner.flattened[i]);
            members.push((arg, &owner.slots[index]));
        }
    });
    members
}

fn target_slot_mut<'m, 'a>(m: &'m mut Matches<'a>, target: &ArgTarget) -> &'m mut Slot<'a> {
    let owner = target
        .path
        .iter()
        .fold(m, |owner, &i| &mut owner.flattened[i]);
    &mut owner.slots[target.index]
}

/// the help or version a long spelling asks for, unless an arg claims it
fn builtin_long(cmd: &Command<'_>, name: &str) -> Option<ErrorKind> {
    let own = cmd.spec().arguments().collect::<Vec<_>>();
    if claims_long(&own, cmd.globals(), name) {
        return None;
    }
    match name {
        "help" => Some(ErrorKind::Help(help::render(cmd, true))),
        "version" if cmd.spec().has_version_info() => {
            Some(ErrorKind::Version(help::version_line(cmd.spec(), true)))
        },
        _ => None,
    }
}

fn builtin_short(cmd: &Command<'_>, ch: char) -> Option<ErrorKind> {
    let own = cmd.spec().arguments().collect::<Vec<_>>();
    if claims_short(&own, cmd.globals(), ch) {
        return None;
    }
    match ch {
        'h' => Some(ErrorKind::Help(help::render(cmd, false))),
        'V' if cmd.spec().has_version_info() => {
            Some(ErrorKind::Version(help::version_line(cmd.spec(), false)))
        },
        _ => None,
    }
}

/// an unrecognized spelling with no suggestion worth offering
const fn unknown(arg: String) -> ErrorKind {
    ErrorKind::Unknown { arg, closest: None }
}

/// every long spelling this command answers to, including the implicit ones
fn long_names<'s>(cmd: &'s Command<'_>) -> impl Iterator<Item = &'s str> {
    let (spec, globals) = (cmd.spec(), cmd.globals());
    let own: Vec<&'static ArgSpec> = spec.arguments().collect();
    let help = !claims_long(&own, globals, "help");
    let version = spec.has_version_info() && !claims_long(&own, globals, "version");
    own.into_iter()
        .chain(globals.iter().copied())
        .filter(|a| !a.hidden)
        .flat_map(|a| {
            a.long
                .into_iter()
                .chain(a.negate)
                .chain(a.aliases.iter().copied())
        })
        .chain(help.then_some("help"))
        .chain(version.then_some("version"))
}

fn sub_names(s: &SubSpec) -> impl Iterator<Item = &str> {
    core::iter::once(s.name).chain(s.aliases.iter().copied())
}

fn closest_long(cmd: &Command<'_>, name: &str) -> Option<String> {
    closest(long_names(cmd), name).map(|found| format!("--{found}"))
}

/// the nearest candidate, if one is close enough that a typo is the likely
/// explanation. short names get a tighter budget, or `ls` would suggest `rm`.
fn closest<'s>(candidates: impl Iterator<Item = &'s str>, target: &str) -> Option<String> {
    let budget = if target.chars().count() <= 4 { 1 } else { 2 };
    let mut best: Option<(usize, &'s str)> = None;
    for candidate in candidates {
        let distance = edit_distance(candidate, target);
        if distance <= budget && best.is_none_or(|(seen, _)| distance < seen) {
            best = Some((distance, candidate));
        }
    }
    best.map(|(_, candidate)| candidate.to_owned())
}

fn edit_distance(a: &str, b: &str) -> usize {
    let width = b.chars().count();
    let mut prev: Vec<usize> = (0..=width).collect();
    let mut cur = vec![0; width + 1];
    for (i, ca) in a.chars().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.chars().enumerate() {
            let substitute = prev[j] + usize::from(ca != cb);
            cur[j + 1] = substitute.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        core::mem::swap(&mut prev, &mut cur);
    }
    prev[width]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{
        GroupSpec,
        SubSpec,
    };

    fn argv<'a>(a: &[&'a str]) -> Vec<&'a str> {
        a.to_vec()
    }

    // flat command: flags, an option (long+short), a count, a required
    // positional and a variadic tail
    const FLAT_ARGS: &[ArgSpec] = &[
        ArgSpec::new(Kind::Flag).long("force").short('f'), // 0
        ArgSpec::new(Kind::Opt)
            .long("dir")
            .short('d')
            .value_name("dir"), // 1
        ArgSpec::new(Kind::Count).long("verbose").short('v'), // 2
        ArgSpec::new(Kind::Positional).value_name("name").required(), // 3
        ArgSpec::new(Kind::Positional).value_name("rest").multi(), // 4
    ];
    const FLAT: CommandSpec = CommandSpec {
        name:           "flat",
        invoked_name:   false,
        version:        "0.1.0",
        long_version:   "",
        hash:           None,
        long_about:     "",
        before_help:    "",
        after_help:     "",
        about:          "a flat command",
        args:           FLAT_ARGS,
        flattened:      &[],
        argument_order: &[],
        groups:         &[],
        conflicts:      &[],
        requires:       &[],
        subs:           &[],
        sub_optional:   false,
        help_if_empty:  false,
    };

    fn parse<'a>(spec: &CommandSpec, a: &[&'a str]) -> Result<Matches<'a>, ErrorKind> {
        parse_spec(spec, argv(a)).map_err(|e| e.kind)
    }

    #[test]
    fn longs_shorts_counts_positionals() {
        let m = parse(&FLAT, &[
            "--force", "--dir", "/x", "-vv", "alpha", "beta", "gamma",
        ])
        .unwrap();
        assert!(m.flag(0));
        assert_eq!(m.raw(1), Some("/x"));
        assert_eq!(m.count(2), 2);
        assert_eq!(m.raw(3), Some("alpha"));
        assert_eq!(m.raws(4), ["beta", "gamma"]);
    }

    #[test]
    fn short_cluster_and_attached_value() {
        let m = parse(&FLAT, &["-vvf", "name"]).unwrap();
        assert_eq!(m.count(2), 2);
        assert!(m.flag(0));

        let m = parse(&FLAT, &["-d/x", "name"]).unwrap();
        assert_eq!(m.raw(1), Some("/x"));

        let m = parse(&FLAT, &["--dir=/y", "name"]).unwrap();
        assert_eq!(m.raw(1), Some("/y"));
    }

    #[test]
    fn last_wins_for_single_option() {
        let m = parse(&FLAT, &["--dir", "/a", "--dir", "/b", "name"]).unwrap();
        assert_eq!(m.raw(1), Some("/b"));
    }

    #[test]
    fn double_dash_forces_positionals() {
        let m = parse(&FLAT, &["--", "--weird", "-x"]).unwrap();
        assert_eq!(m.raw(3), Some("--weird"));
        assert_eq!(m.raws(4), ["-x"]);
    }

    #[test]
    fn trailing_takes_the_rest_after_its_first_value() {
        const ARGS: &[ArgSpec] = &[
            ArgSpec::new(Kind::Flag).long("force"),
            ArgSpec::new(Kind::Trailing).value_name("cmd"),
        ];
        let wrap = CommandSpec { args: ARGS, ..FLAT };
        let m = parse(&wrap, &["--force", "ls", "-la", "--force"]).unwrap();
        assert!(m.flag(0));
        assert_eq!(m.raws(1), ["ls", "-la", "--force"]);
    }

    #[test]
    fn an_exclusive_arg_stands_alone_and_lifts_requirements() {
        const ARGS: &[ArgSpec] = &[
            ArgSpec::new(Kind::Flag).long("list").exclusive(),
            ArgSpec::new(Kind::Flag).long("force"),
            ArgSpec::new(Kind::Positional).value_name("name").required(),
        ];
        let tool = CommandSpec { args: ARGS, ..FLAT };
        assert!(parse(&tool, &["--list"]).is_ok());
        assert!(matches!(
            parse(&tool, &["--list", "--force"]),
            Err(ErrorKind::Exclusive { .. })
        ));
    }

    #[test]
    fn positionals_are_named_as_usage_names_them() {
        let Err(ErrorKind::MissingRequired(name)) = parse(&FLAT, &[]) else {
            panic!("expected a missing positional");
        };
        assert_eq!(name, "NAME");
    }

    #[test]
    fn errors() {
        assert!(matches!(
            parse(&FLAT, &["--nope"]),
            Err(ErrorKind::Unknown { .. })
        ));
        assert!(matches!(
            parse(&FLAT, &["-q"]),
            Err(ErrorKind::Unknown { .. })
        ));
        assert_eq!(parse(&FLAT, &["-5"]).unwrap().raw(3), Some("-5"));
        assert!(matches!(
            parse(&FLAT, &["name", "--dir"]),
            Err(ErrorKind::MissingValue(_))
        ));
        assert!(matches!(
            parse(&FLAT, &[]),
            Err(ErrorKind::MissingRequired(_))
        ));
    }

    #[test]
    fn a_typo_suggests_the_real_spelling() {
        match parse(&FLAT, &["--forse"]) {
            Err(ErrorKind::Unknown { closest, .. }) => {
                assert_eq!(closest.as_deref(), Some("--force"));
            },
            other => panic!("expected an unknown arg, got {other:?}"),
        }
        // too far off to be a typo
        match parse(&FLAT, &["--wibble"]) {
            Err(ErrorKind::Unknown { closest, .. }) => assert_eq!(closest, None),
            other => panic!("expected an unknown arg, got {other:?}"),
        }
    }

    #[test]
    fn help_and_version_signals() {
        const HASHED: CommandSpec = CommandSpec::new("flat").version("0.1.0").hash("abc123");
        const HASH_ONLY: CommandSpec = CommandSpec::new("flat").hash("abc123");

        assert!(matches!(parse(&FLAT, &["--help"]), Err(ErrorKind::Help(_))));
        assert!(matches!(parse(&FLAT, &["-h"]), Err(ErrorKind::Help(_))));
        match parse(&FLAT, &["--version"]) {
            Err(ErrorKind::Version(v)) => assert_eq!(v, "flat 0.1.0"),
            other => panic!("expected version, got {other:?}"),
        }

        match parse(&HASHED, &["--version"]) {
            Err(ErrorKind::Version(v)) => assert_eq!(v, "flat 0.1.0 (abc123)"),
            other => panic!("expected version, got {other:?}"),
        }

        match parse(&HASH_ONLY, &["-V"]) {
            Err(ErrorKind::Version(v)) => assert_eq!(v, "flat (abc123)"),
            other => panic!("expected version, got {other:?}"),
        }
    }

    #[test]
    #[cfg(feature = "std")]
    fn an_unset_env_var_supplies_nothing() {
        const ARGS: &[ArgSpec] = &[ArgSpec::new(Kind::Opt)
            .long("token")
            .env("POUND_TEST_UNSET_TOKEN")
            .required()];
        const SPEC: CommandSpec = CommandSpec::new("e").args(ARGS);
        assert_eq!(
            parse(&SPEC, &[]).unwrap_err(),
            ErrorKind::MissingRequired("--token".to_owned())
        );
    }

    #[test]
    #[cfg(feature = "std")]
    fn a_bad_env_value_names_its_variable() {
        const ARGS: &[ArgSpec] = &[ArgSpec::new(Kind::Opt).long("path").env("PATH")];
        const SPEC: CommandSpec = CommandSpec::new("e").args(ARGS);
        let reject = |s: &str| Err::<u8, _>(ValueError::new(s, "bad"));
        let arg_of = |args: &[&str]| {
            match parse(&SPEC, args).unwrap().required_map(&SPEC, 0, reject) {
                Err(Error {
                    kind: ErrorKind::Value { arg, .. },
                    ..
                }) => arg,
                _ => panic!("expected a value error"),
            }
        };
        assert_eq!(arg_of(&[]), "--path (from $PATH)");
        assert_eq!(arg_of(&["--path", "x"]), "--path");
    }

    #[test]
    fn defaults_resolve_in_readers() {
        const ARGS: &[ArgSpec] = &[ArgSpec::new(Kind::Opt).long("level").default("info")];
        const SPEC: CommandSpec = CommandSpec {
            name:           "d",
            invoked_name:   false,
            version:        "",
            long_version:   "",
            hash:           None,
            long_about:     "",
            before_help:    "",
            after_help:     "",
            about:          "",
            args:           ARGS,
            flattened:      &[],
            argument_order: &[],
            groups:         &[],
            conflicts:      &[],
            requires:       &[],
            subs:           &[],
            sub_optional:   false,
            help_if_empty:  false,
        };
        // unset: the reader falls back to the default
        let m = parse(&SPEC, &[]).unwrap();
        assert_eq!(
            m.optional::<String>(&SPEC, 0).unwrap().as_deref(),
            Some("info")
        );
        // a user value overrides the default
        let m = parse(&SPEC, &["--level", "debug"]).unwrap();
        assert_eq!(
            m.optional::<String>(&SPEC, 0).unwrap().as_deref(),
            Some("debug")
        );
    }

    #[test]
    fn negation_switches_a_flag_off() {
        const ARGS: &[ArgSpec] = &[ArgSpec::new(Kind::Flag)
            .long("color")
            .negate("no-color")
            .default("true")];
        const SPEC: CommandSpec = CommandSpec::new("n").args(ARGS);

        assert!(parse(&SPEC, &[]).unwrap().switch(&SPEC, 0).unwrap());
        assert!(
            !parse(&SPEC, &["--no-color"])
                .unwrap()
                .switch(&SPEC, 0)
                .unwrap()
        );
        // the last spelling on the line wins
        assert!(
            parse(&SPEC, &["--no-color", "--color"])
                .unwrap()
                .switch(&SPEC, 0)
                .unwrap()
        );
        assert!(matches!(
            parse(&SPEC, &["--no-color=1"]),
            Err(ErrorKind::UnexpectedValue(_))
        ));
    }

    #[test]
    fn optional_value_leaves_the_next_token_alone() {
        const ARGS: &[ArgSpec] = &[
            ArgSpec::new(Kind::Opt)
                .long("color")
                .default_missing("always"),
            ArgSpec::new(Kind::Positional).value_name("file"),
        ];
        const SPEC: CommandSpec = CommandSpec::new("o").args(ARGS);

        let m = parse(&SPEC, &["--color", "x.txt"]).unwrap();
        assert_eq!(m.raw(0), Some("always"));
        assert_eq!(m.raw(1), Some("x.txt"));

        assert_eq!(
            parse(&SPEC, &["--color=never"]).unwrap().raw(0),
            Some("never")
        );
    }

    #[test]
    fn groups_conflict_and_require() {
        const ARGS: &[ArgSpec] = &[
            ArgSpec::new(Kind::Flag).long("flake").group("mode"),
            ArgSpec::new(Kind::Flag).long("fetch").group("mode"),
        ];
        const OPT: CommandSpec = CommandSpec {
            name:           "g",
            invoked_name:   false,
            version:        "",
            long_version:   "",
            hash:           None,
            long_about:     "",
            before_help:    "",
            after_help:     "",
            about:          "",
            args:           ARGS,
            flattened:      &[],
            argument_order: &[],
            groups:         &[GroupSpec::new("mode")],
            conflicts:      &[],
            requires:       &[],
            subs:           &[],
            sub_optional:   false,
            help_if_empty:  false,
        };
        const REQ: CommandSpec = CommandSpec {
            groups: &[GroupSpec::new("mode").required()],
            ..OPT
        };
        assert!(matches!(
            parse(&OPT, &["--flake", "--fetch"]),
            Err(ErrorKind::Conflict { .. })
        ));
        assert!(parse(&OPT, &["--flake"]).is_ok());
        assert!(parse(&OPT, &[]).is_ok()); // not required, zero is fine
        assert!(matches!(
            parse(&REQ, &[]),
            Err(ErrorKind::MissingGroup { .. })
        ));
    }

    #[test]
    fn value_counts_are_bounded() {
        const ARGS: &[ArgSpec] = &[ArgSpec::new(Kind::Positional)
            .value_name("file")
            .multi()
            .min_values(1)
            .max_values(2)];
        const SPEC: CommandSpec = CommandSpec::new("c").args(ARGS);

        assert!(parse(&SPEC, &["a"]).is_ok());
        assert!(parse(&SPEC, &["a", "b"]).is_ok());
        assert!(matches!(
            parse(&SPEC, &[]),
            Err(ErrorKind::TooFewValues { .. })
        ));
        assert!(matches!(
            parse(&SPEC, &["a", "b", "c"]),
            Err(ErrorKind::TooManyValues { .. })
        ));
    }

    #[test]
    fn requires_obliges_the_other_arg() {
        const ARGS: &[ArgSpec] = &[
            ArgSpec::new(Kind::Opt).long("output"),
            ArgSpec::new(Kind::Opt).long("format"),
        ];
        const SPEC: CommandSpec = CommandSpec::new("r").args(ARGS).requires(&[(0, 1)]);

        assert!(parse(&SPEC, &[]).is_ok());
        assert!(parse(&SPEC, &["--format", "json"]).is_ok());
        assert!(parse(&SPEC, &["--output", "f", "--format", "json"]).is_ok());
        assert!(matches!(
            parse(&SPEC, &["--output", "f"]),
            Err(ErrorKind::Requires { .. })
        ));
    }

    #[test]
    fn conflict_pairs() {
        const ARGS: &[ArgSpec] = &[
            ArgSpec::new(Kind::Flag).long("a"),
            ArgSpec::new(Kind::Flag).long("b"),
        ];
        const SPEC: CommandSpec = CommandSpec {
            name:           "c",
            invoked_name:   false,
            version:        "",
            long_version:   "",
            hash:           None,
            long_about:     "",
            before_help:    "",
            after_help:     "",
            about:          "",
            args:           ARGS,
            flattened:      &[],
            argument_order: &[],
            groups:         &[],
            conflicts:      &[(0, 1)],
            requires:       &[],
            subs:           &[],
            sub_optional:   false,
            help_if_empty:  false,
        };
        assert!(parse(&SPEC, &["--a"]).is_ok());
        assert!(matches!(
            parse(&SPEC, &["--a", "--b"]),
            Err(ErrorKind::Conflict { .. })
        ));
    }

    // a subcommand tree: `prog add <name> <url> [--force]`
    const ADD_ARGS: &[ArgSpec] = &[
        ArgSpec::new(Kind::Positional).value_name("name").required(),
        ArgSpec::new(Kind::Positional).value_name("url").required(),
        ArgSpec::new(Kind::Flag).long("force").short('f'),
    ];
    const ADD: CommandSpec = CommandSpec {
        name:           "add",
        invoked_name:   false,
        version:        "",
        long_version:   "",
        hash:           None,
        long_about:     "",
        before_help:    "",
        after_help:     "",
        about:          "add a pin",
        args:           ADD_ARGS,
        flattened:      &[],
        argument_order: &[],
        groups:         &[],
        conflicts:      &[],
        requires:       &[],
        subs:           &[],
        sub_optional:   false,
        help_if_empty:  false,
    };
    const ROOT_SUBS: &[SubSpec] = &[SubSpec {
        name:          "add",
        aliases:       &[],
        about:         "add a pin",
        spec:          &ADD,
        hidden:        false,
        flattened:     false,
        external:      false,
        display_order: None,
    }];
    const ROOT: CommandSpec = CommandSpec {
        name:           "prog",
        invoked_name:   false,
        version:        "1.0.0",
        long_version:   "",
        hash:           None,
        long_about:     "",
        before_help:    "",
        after_help:     "",
        about:          "demo",
        args:           &[],
        flattened:      &[],
        argument_order: &[],
        groups:         &[],
        conflicts:      &[],
        requires:       &[],
        subs:           ROOT_SUBS,
        sub_optional:   false,
        help_if_empty:  false,
    };

    #[test]
    fn subcommands() {
        let m = parse(&ROOT, &["add", "serde", "https://x", "--force"]).unwrap();
        let (idx, sub) = m.sub().expect("a subcommand");
        assert_eq!(idx, 0);
        assert_eq!(sub.raw(0), Some("serde"));
        assert_eq!(sub.raw(1), Some("https://x"));
        assert!(sub.flag(2));

        assert!(matches!(
            parse(&ROOT, &["nope"]),
            Err(ErrorKind::UnknownSubcommand { .. })
        ));
        assert!(matches!(
            parse(&ROOT, &[]),
            Err(ErrorKind::MissingSubcommand { help: Some(_) })
        ));
    }

    // positionals and subs on one command (`prog <process> <access> COMMAND`)
    const MIXED_ARGS: &[ArgSpec] = &[
        ArgSpec::new(Kind::Positional)
            .value_name("process")
            .required(),
        ArgSpec::new(Kind::Positional)
            .value_name("access")
            .required(),
    ];
    const MIXED: CommandSpec = CommandSpec {
        args: MIXED_ARGS,
        ..ROOT
    };

    #[test]
    fn subcommand_after_parent_positionals() {
        let m = parse(&MIXED, &["p1", "a1", "add", "serde", "https://x"]).unwrap();
        assert_eq!(m.raw(0), Some("p1"));
        assert_eq!(m.raw(1), Some("a1"));
        let (idx, sub) = m.sub().expect("a subcommand");
        assert_eq!(idx, 0);
        assert_eq!(sub.raw(0), Some("serde"));

        // an unfilled required positional still wins the token
        let m = parse(&MIXED, &["p1", "add", "add", "serde", "https://x"]).unwrap();
        assert_eq!(m.raw(1), Some("add"));
        assert!(m.sub().is_some());

        assert!(matches!(
            parse(&MIXED, &["p1", "a1", "nope"]),
            Err(ErrorKind::UnknownSubcommand { .. })
        ));
    }

    fn expected_add_usage() -> &'static str {
        if cfg!(feature = "help") {
            "Usage: prog add [OPTION]... NAME URL"
        } else {
            "Usage: prog add"
        }
    }

    #[test]
    fn an_unknown_subcommand_goes_external_with_the_rest_of_the_line() {
        const SUBS: &[SubSpec] = &[ROOT_SUBS[0], SubSpec::external()];
        let root = CommandSpec { subs: SUBS, ..ROOT };

        let m = parse(&root, &["frob", "--force", "x"]).unwrap();
        let (index, ext) = m.sub().unwrap();
        assert_eq!(index, 1);
        assert_eq!(ext.raws(0), ["frob", "--force", "x"]);
        assert_eq!(
            parse(&root, &["add", "a", "b"])
                .unwrap()
                .sub()
                .map(|(i, _)| i),
            Some(0)
        );
    }

    #[test]
    fn subcommand_usage_carries_the_program_path() {
        let err = parse_spec(&ROOT, argv(&["add", "only"])).unwrap_err();
        let usage = err.usage.map(|line| line.to_string());
        assert_eq!(usage.as_deref(), Some(expected_add_usage()));

        let ErrorKind::Help(help) = parse(&ROOT, &["add", "--help"]).unwrap_err() else {
            panic!("expected help");
        };
        let text = help.to_string();
        assert!(text.contains(expected_add_usage()), "{text}");

        let err = parse_spec(&ROOT, argv(&["--nope"])).unwrap_err();
        assert!(err.usage.unwrap().to_string().starts_with("Usage: prog"));
    }
}
