// SPDX-License-Identifier: EUPL-1.2

//! walks `argv` against a [`CommandSpec`] and produces [`Matches`]

use alloc::borrow::Cow;
use core::{
    convert::Infallible,
    iter::{
        self,
        Peekable,
    },
    mem,
    ptr,
};
#[cfg(feature = "std")] use std::env;

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
        Arity,
        Command,
        CommandSpec,
        Kind,
        SubSpec,
        claims_long,
        claims_short,
    },
    value::{
        FromArg,
        Source,
        Sourced,
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
#[expect(
    clippy::partial_pub_fields,
    reason = "the parse error stays private behind the unknown accessor"
)]
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
    fn store<'a>(self, matches: &mut Matches<'a>, selected: Matches<'a>) {
        let owner = self
            .path
            .iter()
            .fold(matches, |owner, &i| &mut owner.flattened[i]);
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
    /// the words ran out where this arg wanted a value, after `got` of them
    NeedsValue {
        arg: &'static ArgSpec,
        got: usize,
    },
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

    /// whether arg `i` was written on the line, its negation included
    #[must_use]
    pub fn given(&self, i: usize) -> bool {
        self.slots[i].count > 0 || self.slots[i].negated
    }

    /// resolve a flag to its final state, honoring the negation spelling and
    /// falling back to the env var or default when neither was given
    ///
    /// # Errors
    ///
    /// fails with [`ErrorKind::Value`] when the flag falls back to an env var
    /// or default that is not one of 1, true, yes, 0, false or no.
    pub fn switch(&self, spec: &CommandSpec, i: usize) -> Result<bool, Error> {
        let slot = &self.slots[i];
        if slot.negated {
            return Ok(false);
        }
        if slot.count > 0 {
            return Ok(true);
        }
        let word = |raw: &str| {
            match raw {
                "1" | "true" | "yes" => Ok(true),
                "" | "0" | "false" | "no" => Ok(false),
                _ => {
                    Err(ValueError::new(
                        raw,
                        "expected 1, true, yes, 0, false or no",
                    ))
                },
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
        self.sub.as_ref().map(|(i, sub)| (*i, sub.as_ref()))
    }

    /// check the subcommand selected among `spec`'s as a fresh parse would,
    /// for an update that switches to it
    ///
    /// # Errors
    ///
    /// fails when the selected subcommand misses a required arg, group or
    /// subcommand, or breaks a conflict, requires or arity rule.
    pub fn demand_sub(&self, spec: &'static CommandSpec) -> Result<(), Error> {
        Ok(demand_selected(spec, self)?)
    }

    /// the name and words an external subcommand took, read from its matches
    #[must_use]
    pub fn external(&self) -> External {
        let (raw_name, args) = self.raws(0).split_first().unwrap_or((&"", &[]));
        let name = (*raw_name).to_owned();
        let unknown = self.unknown.as_ref().map_or_else(
            || {
                ErrorKind::UnknownSubcommand {
                    name:    name.clone(),
                    closest: None,
                }
                .into()
            },
            |err| (**err).clone(),
        );
        External {
            name,
            args: args.iter().map(|&arg| arg.to_owned()).collect(),
            unknown,
        }
    }

    /// read a required value as `T`
    ///
    /// # Errors
    ///
    /// fails with [`ErrorKind::Value`] when the value, or the env var or
    /// default it falls back to, does not convert, and
    /// [`ErrorKind::MissingRequired`] when it has none of them.
    pub fn required<T: FromArg>(&self, spec: &CommandSpec, i: usize) -> Result<T, Error> {
        self.required_map(spec, i, T::from_arg)
    }

    /// read an optional value as `Option<T>`
    ///
    /// # Errors
    ///
    /// fails with [`ErrorKind::Value`] when a value, or the env var or default
    /// it falls back to, does not convert.
    pub fn optional<T: FromArg>(&self, spec: &CommandSpec, i: usize) -> Result<Option<T>, Error> {
        self.optional_map(spec, i, T::from_arg)
    }

    /// read every value into `Vec<T>`
    ///
    /// # Errors
    ///
    /// fails with [`ErrorKind::Value`] when a value, or the env var or default
    /// it falls back to, does not convert.
    pub fn many<T: FromArg>(&self, spec: &CommandSpec, i: usize) -> Result<Vec<T>, Error> {
        self.many_map(spec, i, T::from_arg)
    }

    /// read every value into `Some(Vec<T>)`, or `None` when the arg was never
    /// given and has no fallback
    ///
    /// # Errors
    ///
    /// fails with [`ErrorKind::Value`] when a value, or the env var or default
    /// it falls back to, does not convert.
    pub fn optional_many<T: FromArg>(
        &self,
        spec: &CommandSpec,
        i: usize,
    ) -> Result<Option<Vec<T>>, Error> {
        self.optional_many_map(spec, i, T::from_arg)
    }

    /// read a required value
    /// caller must supply conversion fn
    ///
    /// # Errors
    ///
    /// fails with [`ErrorKind::Value`] when `convert` rejects the value or its
    /// fallback, and [`ErrorKind::MissingRequired`] when it has neither.
    pub fn required_map<T>(
        &self,
        spec: &CommandSpec,
        i: usize,
        convert: impl Fn(&str) -> Result<T, ValueError>,
    ) -> Result<T, Error> {
        if let Some(raw) = self.raw(i) {
            return parse_with(spec, i, raw, None, &convert);
        }
        parse_fallback(spec, i, &convert)
            .unwrap_or_else(|| Err(ErrorKind::MissingRequired(spec.args[i].display_name()).into()))
    }

    /// read an optional value
    /// caller must supply conversion fn
    ///
    /// # Errors
    ///
    /// fails with [`ErrorKind::Value`] when `convert` rejects the value or its
    /// fallback.
    pub fn optional_map<T>(
        &self,
        spec: &CommandSpec,
        i: usize,
        convert: impl Fn(&str) -> Result<T, ValueError>,
    ) -> Result<Option<T>, Error> {
        if let Some(raw) = self.raw(i) {
            return Ok(Some(parse_with(spec, i, raw, None, &convert)?));
        }
        parse_fallback(spec, i, &convert).transpose()
    }

    /// read an option whose value may be left out, as `None` when it was
    /// not given, `Some(None)` when given with no value, and `Some(Some(_))`
    /// when given one or filled by its fallback
    /// caller must supply conversion fn
    ///
    /// # Errors
    ///
    /// fails with [`ErrorKind::Value`] when `convert` rejects the value or its
    /// fallback.
    pub fn optional_value_map<T>(
        &self,
        spec: &CommandSpec,
        i: usize,
        convert: impl Fn(&str) -> Result<T, ValueError>,
    ) -> Result<Option<Option<T>>, Error> {
        if let Some(raw) = self.raw(i) {
            return Ok(Some(Some(parse_with(spec, i, raw, None, &convert)?)));
        }
        if self.slots[i].count > 0 {
            return Ok(Some(None));
        }
        Ok(parse_fallback(spec, i, &convert).transpose()?.map(Some))
    }

    /// where the value of arg `i` came from, `None` when it has none
    #[must_use]
    pub fn source(&self, spec: &CommandSpec, i: usize) -> Option<Source> {
        if self.given(i) {
            return Some(Source::CommandLine);
        }
        fallback(spec, i).map(|found| found.source())
    }

    /// resolve a flag like [`Self::switch`], with where its state came from
    ///
    /// # Errors
    ///
    /// fails like [`Self::switch`].
    pub fn switch_sourced(&self, spec: &CommandSpec, i: usize) -> Result<Sourced<bool>, Error> {
        Ok(Sourced {
            value:  self.switch(spec, i)?,
            source: self.source(spec, i).unwrap_or(Source::Default),
        })
    }

    /// read a required value with where it came from
    /// caller must supply conversion fn
    ///
    /// # Errors
    ///
    /// fails with [`ErrorKind::Value`] when `convert` rejects the value or its
    /// fallback, and [`ErrorKind::MissingRequired`] when it has neither.
    pub fn required_sourced_map<T>(
        &self,
        spec: &CommandSpec,
        i: usize,
        convert: impl Fn(&str) -> Result<T, ValueError>,
    ) -> Result<Sourced<T>, Error> {
        self.optional_sourced_map(spec, i, convert)?
            .ok_or_else(|| ErrorKind::MissingRequired(spec.args[i].display_name()).into())
    }

    /// read an optional value with where it came from
    /// caller must supply conversion fn
    ///
    /// # Errors
    ///
    /// fails with [`ErrorKind::Value`] when `convert` rejects the value or its
    /// fallback.
    pub fn optional_sourced_map<T>(
        &self,
        spec: &CommandSpec,
        i: usize,
        convert: impl Fn(&str) -> Result<T, ValueError>,
    ) -> Result<Option<Sourced<T>>, Error> {
        if let Some(raw) = self.raw(i) {
            return Ok(Some(Sourced {
                value:  parse_with(spec, i, raw, None, &convert)?,
                source: Source::CommandLine,
            }));
        }
        let Some(found) = fallback(spec, i) else {
            return Ok(None);
        };
        Ok(Some(Sourced {
            value:  parse_with(spec, i, &found.value, found.env, &convert)?,
            source: found.source(),
        }))
    }

    /// read every value
    /// caller must supply conversion fn
    ///
    /// # Errors
    ///
    /// fails with [`ErrorKind::Value`] when `convert` rejects any value, or any
    /// piece of the split fallback.
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
                .map(|&raw| parse_with(spec, i, raw, None, &convert))
                .collect();
        }
        let Some(found) = fallback(spec, i) else {
            return Ok(Vec::new());
        };
        spec.args[i]
            .split(&found.value)
            .map(|raw| parse_with(spec, i, raw, found.env, &convert))
            .collect()
    }

    /// read every value, or `None` when the arg was never given and has no
    /// fallback
    /// caller must supply conversion fn
    ///
    /// # Errors
    ///
    /// fails with [`ErrorKind::Value`] when `convert` rejects any value, or any
    /// piece of the split fallback.
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

impl Fallback {
    fn source(&self) -> Source {
        self.env.map_or(Source::Default, Source::Env)
    }
}

fn fallback(spec: &CommandSpec, i: usize) -> Option<Fallback> {
    #[cfg(feature = "std")]
    if let Some(var) = spec.args[i].env
        && let Ok(value) = env::var(var)
    {
        return Some(Fallback {
            value: Cow::Owned(value),
            env:   Some(var),
        });
    }
    spec.args[i].default.map(|default| {
        Fallback {
            value: default.text(),
            env:   None,
        }
    })
}

fn parse_fallback<T>(
    spec: &CommandSpec,
    i: usize,
    convert: &impl Fn(&str) -> Result<T, ValueError>,
) -> Option<Result<T, Error>> {
    fallback(spec, i).map(|found| parse_with(spec, i, &found.value, found.env, convert))
}

fn parse_with<T>(
    spec: &CommandSpec,
    i: usize,
    raw: &str,
    env: Option<&str>,
    convert: &impl Fn(&str) -> Result<T, ValueError>,
) -> Result<T, Error> {
    convert(raw).map_err(|error| {
        let mut msg = error.msg;
        let listed = spec.args[i]
            .possible
            .unwrap_or_default()
            .iter()
            .filter(|choice| !choice.hidden)
            .map(|choice| choice.name)
            .collect::<Vec<_>>();
        if !listed.is_empty() {
            msg = format!("{msg} (possible values: {})", listed.join(", "));
        }
        ErrorKind::Value {
            arg: source_name(&spec.args[i], env),
            value: error.value,
            msg,
        }
        .into()
    })
}

/// an arg left off the line reads its env var, which [`fallback`] would skip
/// in silence when it holds no UTF-8
#[cfg(feature = "std")]
fn env_readable(arg: &ArgSpec) -> Result<(), ErrorKind> {
    let Some(var) = arg.env else {
        return Ok(());
    };
    match env::var(var) {
        Err(env::VarError::NotUnicode(raw)) => {
            Err(ErrorKind::Value {
                arg:   source_name(arg, Some(var)),
                value: raw.to_string_lossy().into_owned(),
                msg:   "not valid UTF-8".to_owned(),
            })
        },
        _ => Ok(()),
    }
}

fn source_name(arg: &ArgSpec, env: Option<&str>) -> String {
    env.map_or_else(
        || arg.display_name(),
        |var| format!("{} (from ${var})", arg.display_name()),
    )
}

/// entrypoint. parse `args[1..]` against `spec`
pub(crate) fn parse_spec<'a>(
    spec: &CommandSpec,
    args: impl IntoIterator<Item = &'a str>,
) -> Result<Matches<'a>, Error> {
    let mut input = args.into_iter();
    let iterator: &mut dyn Iterator<Item = &'a str> = &mut input;
    let mut it = iterator.peekable();
    let mut hits = Vec::new();
    parse_cmd::<Strict>(&Command::root(spec), None, &mut it, &mut hits).map(|(matches, _)| matches)
}

pub(crate) fn update_spec<'a>(
    spec: &CommandSpec,
    args: impl IntoIterator<Item = &'a str>,
) -> Result<Matches<'a>, Error> {
    let mut input = args.into_iter();
    let iterator: &mut dyn Iterator<Item = &'a str> = &mut input;
    let mut it = iterator.peekable();
    let mut hits = Vec::new();
    parse_cmd::<Update>(&Command::root(spec), None, &mut it, &mut hits).map(|(matches, _)| matches)
}

/// parse `args` as the start of a command line, skipping whatever does not
/// apply and checking no requirements, and say what the next word would be
pub(crate) fn probe_spec<'a>(
    spec: &CommandSpec,
    args: impl IntoIterator<Item = &'a str>,
) -> (Matches<'a>, Next) {
    let mut input = args.into_iter();
    let iterator: &mut dyn Iterator<Item = &'a str> = &mut input;
    let mut it = iterator.peekable();
    let mut hits = Vec::new();
    let Ok(walked) = parse_cmd::<Lenient>(&Command::root(spec), None, &mut it, &mut hits);
    walked
}

/// how a walk treats what it cannot apply
trait Mode {
    type Error;

    const DEMANDS: Demands;

    /// a token that could not be applied. an option still waiting on its value
    /// when the words ran out goes in `awaiting`.
    fn halt(halt: Halt, awaiting: &mut Option<&'static ArgSpec>) -> Result<(), Self::Error>;

    fn finish(cmd: &Command<'_>, matches: &Matches<'_>, empty: bool) -> Result<(), Self::Error>;

    fn locate(cmd: &Command<'_>, err: Self::Error) -> Self::Error;

    fn show(cmd: &Command<'_>) -> Result<(), Self::Error>;
}

enum Strict {}

impl Mode for Strict {
    type Error = Error;

    const DEMANDS: Demands = Demands::Checked;

    fn halt(halt: Halt, _: &mut Option<&'static ArgSpec>) -> Result<(), Error> {
        match halt {
            Halt::Error(err) => Err(err),
            Halt::NeedsValue { arg, got } => {
                let min = arg.words_per_use().map_or(1, Arity::min);
                if min > 1 {
                    Err(ErrorKind::TooFewValues {
                        arg: arg.display_name(),
                        min,
                        got,
                    }
                    .into())
                } else {
                    Err(ErrorKind::MissingValue(arg.display_name()).into())
                }
            },
            Halt::TakesMore(_) => Ok(()),
        }
    }

    fn finish(cmd: &Command<'_>, matches: &Matches<'_>, empty: bool) -> Result<(), Error> {
        if empty && cmd.spec().help_if_empty {
            return Err(ErrorKind::NoArguments(help::render(cmd, false)).into());
        }
        Ok(finalise(cmd, matches, Self::DEMANDS)?)
    }

    fn locate(cmd: &Command<'_>, err: Error) -> Error {
        cmd.locate(err)
    }

    fn show(cmd: &Command<'_>) -> Result<(), Error> {
        Err(ErrorKind::Help(help::render(cmd, true)).into())
    }
}

enum Update {}

impl Mode for Update {
    type Error = Error;

    const DEMANDS: Demands = Demands::Waived;

    fn halt(halt: Halt, awaiting: &mut Option<&'static ArgSpec>) -> Result<(), Error> {
        Strict::halt(halt, awaiting)
    }

    fn finish(cmd: &Command<'_>, matches: &Matches<'_>, _: bool) -> Result<(), Error> {
        Ok(finalise(cmd, matches, Self::DEMANDS)?)
    }

    fn locate(cmd: &Command<'_>, err: Error) -> Error {
        Strict::locate(cmd, err)
    }

    fn show(cmd: &Command<'_>) -> Result<(), Error> {
        Strict::show(cmd)
    }
}

/// an unfinished line being completed, which skips what it cannot apply and
/// so cannot fail
enum Lenient {}

impl Mode for Lenient {
    type Error = Infallible;

    const DEMANDS: Demands = Demands::Checked;

    fn halt(halt: Halt, awaiting: &mut Option<&'static ArgSpec>) -> Result<(), Infallible> {
        if let Halt::NeedsValue { arg, .. } | Halt::TakesMore(arg) = halt {
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
    walk_cmd::<M>(cmd, lead, it, hits).map_err(|error| M::locate(cmd, error))
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
    last:            Option<&'w ArgTarget>,
    cursor:          usize,
    only_positional: bool,
    separated:       bool,
    demands:         Demands,
    matches:         Matches<'a>,
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
        positionals: targets
            .iter()
            .filter(|target| target.arg.is_positional() && target.arg.kind != Kind::Last)
            .collect(),
        last: targets.iter().find(|target| target.arg.kind == Kind::Last),
        cursor: 0,
        only_positional: false,
        separated: false,
        demands: M::DEMANDS,
        matches: Matches::new(cmd.spec()),
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

    // must run before finalise so owned globals count toward required/group
    // checks
    apply_global_hits(&targets, cmd.globals().len(), &mut walk.matches, hits);
    M::finish(cmd, &walk.matches, empty)?;
    Ok((walk.matches, next))
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
            self.separated = true;
        } else if let Some(long) = tok.strip_prefix("--") {
            self.long(tok, long, it, hits)?;
        } else if let Some(rest) = tok.strip_prefix('-').filter(|tail| !tail.is_empty()) {
            self.short(tok, rest, it, hits)?;
        } else if tok == "help" && self.cmd.help_command().is_some() && self.sub_open() {
            return Ok(Step::Help);
        } else if let Some(target) = self.sub_dispatch(tok)? {
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
            Some((key, value)) => (key, Some(value)),
            None => (long, None),
        };
        if let Some(sig) = builtin_long(self.cmd, name) {
            return Err(sig.into());
        }

        if let Some(target) = self.targets.iter().find(|candidate| {
            candidate.arg.answers_long(name) && candidate.arg.negate != Some(name)
        }) {
            apply_named(&mut self.matches, target, inline, it)?;
        } else if let Some(target) = self
            .targets
            .iter()
            .find(|candidate| candidate.arg.negate == Some(name))
        {
            apply_negation(target_slot_mut(&mut self.matches, target), name, inline)?;
        } else if let Some((index, global)) = find_global_long(self.cmd.globals(), name) {
            if global.negate != Some(name) {
                record_global(index, global, inline, it, hits)?;
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
                .find(|target| target.arg.answers_short(ch))
                .map(|target| target.arg)
                .or_else(|| find_global_short(cmd.globals(), ch).map(|(_, global)| global))
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
            shorts(
                targets,
                &mut self.matches,
                rest,
                it,
                cmd.globals(),
                hits,
                builtin,
            )
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
        self.matches.selected = sub_m.selected.take().or(Some(child));
        target.store(&mut self.matches, sub_m);
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
        self.matches.selected = named;
        Ok(next)
    }

    fn sub_dispatch(&self, tok: &str) -> Result<Option<SubTarget>, ErrorKind> {
        let spec = self.cmd.spec();
        if !self.sub_open() {
            return Ok(None);
        }
        match SubTarget::find(spec, tok) {
            Some(target) => Ok(Some(target)),
            // an open optional slot can still take it
            None if self.cursor < self.positionals.len() => Ok(None),
            None => {
                SubTarget::find_external(spec)
                    .map(Some)
                    .ok_or_else(|| unknown_subcommand(spec, tok))
            },
        }
    }

    /// a bare token names a subcommand only once every required positional is
    /// filled, unless requirements are waived, and never once a variadic one
    /// has started, which then stays greedy so its tail can fill
    fn sub_open(&self) -> bool {
        let variadic_started = self.positionals.get(self.cursor).is_some_and(|target| {
            (target.arg.multi || target.arg.kind == Kind::Trailing)
                && target_slot(&self.matches, target).count > 0
        });
        let pending = self
            .positionals
            .iter()
            .skip(self.cursor)
            .any(|target| target.arg.required);
        self.cmd.spec().has_subs()
            && !variadic_started
            && (self.demands == Demands::Waived || !pending)
    }

    fn takes_hyphen_value(&self) -> bool {
        next_positional(&self.positionals, self.cursor)
            .is_some_and(|target| target.arg.hyphen_values)
    }

    fn positional(&mut self, tok: &'a str) -> Result<(), ErrorKind> {
        if self.separated
            && let Some(last) = self.last
        {
            let slot = target_slot_mut(&mut self.matches, last);
            slot.values.push(tok);
            slot.count += 1;
            return Ok(());
        }
        if positional(&mut self.matches, &self.positionals, &mut self.cursor, tok)? {
            self.only_positional = true;
        }
        Ok(())
    }

    fn next(&self, awaiting: Option<&'static ArgSpec>) -> Next {
        if let Some(arg) = awaiting {
            return Next::Value(arg);
        }
        Next::Word {
            positional:  self
                .last
                .filter(|_| self.separated)
                .or_else(|| next_positional(&self.positionals, self.cursor))
                .map(|target| target.arg),
            subcommands: !self.only_positional && self.sub_open(),
            options:     !self.only_positional,
        }
    }
}

fn unknown_subcommand(spec: &CommandSpec, tok: &str) -> ErrorKind {
    ErrorKind::UnknownSubcommand {
        name:    tok.to_owned(),
        closest: closest(
            spec.subcommands()
                .filter(|sub| !sub.hidden)
                .flat_map(sub_names),
            tok,
        ),
    }
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
                .filter(|target| target.arg.multi || target.arg.kind == Kind::Trailing)
        },
    }
}

/// apply a long option
fn apply_named<'a>(
    matches: &mut Matches<'a>,
    target: &ArgTarget,
    inline: Option<&'a str>,
    it: &mut Words<'_, 'a>,
) -> Result<(), Halt> {
    let arg = target.arg;
    let slot = target_slot_mut(matches, target);
    match arg.kind {
        Kind::Flag => {
            if inline.is_some() {
                return Err(ErrorKind::UnexpectedValue(arg.display_name()).into());
            }
            slot.count = 1;
            slot.negated = false;
        },
        Kind::Count => {
            if inline.is_some() {
                return Err(ErrorKind::UnexpectedValue(arg.display_name()).into());
            }
            slot.count += 1;
        },
        Kind::Opt => {
            let value = match inline {
                Some(attached) => Some(attached),
                None => detached_value(arg, it)?,
            };
            use_option(arg, slot, value, it)?;
        },
        Kind::Positional | Kind::Trailing | Kind::Last => {
            return Err(unknown(arg.display_name()).into());
        },
    }
    Ok(())
}

fn use_option<'a>(
    arg: &'static ArgSpec,
    slot: &mut Slot<'a>,
    value: Option<&'a str>,
    it: &mut Words<'_, 'a>,
) -> Result<(), Halt> {
    let Some(first) = value else {
        push_bare(arg, slot);
        return Ok(());
    };
    push_value(arg, slot, first);
    more_values(arg, it, |more| push_value(arg, slot, more))
}

/// apply a cluster of short args
fn shorts<'a>(
    targets: &[ArgTarget],
    matches: &mut Matches<'a>,
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
        if let Some(target) = targets
            .iter()
            .find(|candidate| candidate.arg.answers_short(ch))
        {
            let arg = target.arg;
            let slot = target_slot_mut(matches, target);
            match arg.kind {
                Kind::Flag => {
                    slot.count = 1;
                    slot.negated = false;
                },
                Kind::Count => slot.count += 1,
                Kind::Opt => {
                    let value = cluster_value(cluster, off, ch, it, arg)?;
                    use_option(arg, slot, value, it)?;
                    return Ok(()); // option swallowed the cluster tail
                },
                Kind::Positional | Kind::Trailing | Kind::Last => {
                    return Err(unknown(format!("-{ch}")).into());
                },
            }
        } else if let Some((index, global)) = find_global_short(globals, ch) {
            match global.kind {
                Kind::Flag | Kind::Count => {
                    hits.push(GlobalHit {
                        index,
                        value: None,
                        negated: false,
                    });
                },
                Kind::Opt => {
                    let value = cluster_value(cluster, off, ch, it, global)?;
                    record_values(index, global, value, it, hits)?;
                    return Ok(());
                },
                Kind::Positional | Kind::Trailing | Kind::Last => {
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
    arg: &'static ArgSpec,
) -> Result<Option<&'a str>, Halt> {
    let rest = &cluster[off + ch.len_utf8()..];
    if rest.is_empty() {
        detached_value(arg, it)
    } else {
        Ok(Some(rest))
    }
}

/// the value for an option written without an attached one. an option that
/// declares `default_missing` takes that instead of eating the next token.
fn detached_value<'a>(
    arg: &'static ArgSpec,
    it: &mut Words<'_, 'a>,
) -> Result<Option<&'a str>, Halt> {
    if let Some(missing) = arg.default_missing {
        return Ok(Some(missing));
    }
    if arg.optional_value {
        return Ok(None);
    }
    it.next_if(|word| arg.hyphen_values || !looks_like_option(word))
        .map(Some)
        .ok_or(Halt::NeedsValue { arg, got: 0 })
}

fn more_values<'a>(
    arg: &'static ArgSpec,
    it: &mut Words<'_, 'a>,
    mut each: impl FnMut(&'a str),
) -> Result<(), Halt> {
    let Some(arity) = arg.words_per_use() else {
        return Ok(());
    };

    let mut taken = 1;
    while arity.max().is_none_or(|max| taken < max) {
        let Some(word) =
            it.next_if(|word| *word != "--" && (arg.hyphen_values || !looks_like_option(word)))
        else {
            break;
        };
        each(word);
        taken += 1;
    }

    let min = arity.min();
    let open = arity.max().is_none_or(|max| taken < max);
    match it.peek() {
        None if taken >= min && open => Err(Halt::TakesMore(arg)),
        _ if taken >= min => Ok(()),
        None => Err(Halt::NeedsValue { arg, got: taken }),
        Some(_) => {
            Err(ErrorKind::TooFewValues {
                arg: arg.display_name(),
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

fn record_values<'a>(
    index: usize,
    global: &'static ArgSpec,
    value: Option<&'a str>,
    it: &mut Words<'_, 'a>,
    hits: &mut Vec<GlobalHit<'a>>,
) -> Result<(), Halt> {
    let mut record = |word| {
        hits.push(GlobalHit {
            index,
            value: word,
            negated: false,
        });
    };
    record(value);
    if value.is_none() {
        return Ok(());
    }
    more_values(global, it, |word| record(Some(word)))
}

fn push_bare(arg: &ArgSpec, slot: &mut Slot<'_>) {
    if !arg.multi {
        slot.values.clear();
    }
    slot.count += 1;
}

fn push_value<'a>(arg: &ArgSpec, slot: &mut Slot<'a>, value: &'a str) {
    if !arg.multi {
        slot.values.clear(); // last wins for single-valued opts
    }
    slot.values.extend(arg.split(value));
    slot.count += 1;
}

/// the nearest ancestor's global wins when several answer to the same spelling
fn find_global_long(globals: &[&'static ArgSpec], name: &str) -> Option<(usize, &'static ArgSpec)> {
    globals
        .iter()
        .copied()
        .enumerate()
        .rev()
        .find(|(_, arg)| arg.answers_long(name))
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
        .find(|(_, arg)| arg.answers_short(ch))
}

fn record_global<'a>(
    index: usize,
    global: &'static ArgSpec,
    inline: Option<&'a str>,
    it: &mut Words<'_, 'a>,
    hits: &mut Vec<GlobalHit<'a>>,
) -> Result<(), Halt> {
    match global.kind {
        Kind::Flag | Kind::Count => {
            if inline.is_some() {
                return Err(ErrorKind::UnexpectedValue(global.display_name()).into());
            }
            hits.push(GlobalHit {
                index,
                value: None,
                negated: false,
            });
        },
        Kind::Opt => {
            let value = match inline {
                Some(attached) => Some(attached),
                None => detached_value(global, it)?,
            };
            record_values(index, global, value, it, hits)?;
        },
        Kind::Positional | Kind::Trailing | Kind::Last => {
            return Err(unknown(global.display_name()).into());
        },
    }
    Ok(())
}

/// apply the hits this `spec` owns into its slots, leaving the rest to bubble
/// up
fn apply_global_hits<'a>(
    targets: &[ArgTarget],
    inherited: usize,
    matches: &mut Matches<'a>,
    hits: &mut Vec<GlobalHit<'a>>,
) {
    hits.retain(|hit| {
        let Some(target) = hit.index.checked_sub(inherited).and_then(|own| {
            targets
                .iter()
                .filter(|candidate| candidate.arg.global)
                .nth(own)
        }) else {
            return true;
        };
        let arg = target.arg;
        let slot = target_slot_mut(matches, target);
        match arg.kind {
            Kind::Flag => {
                slot.count = u32::from(!hit.negated);
                slot.negated = hit.negated;
            },
            Kind::Count => slot.count += 1,
            Kind::Opt => {
                match hit.value {
                    Some(value) => push_value(arg, slot, value),
                    None => push_bare(arg, slot),
                }
            },
            Kind::Positional | Kind::Trailing | Kind::Last => {},
        }
        false
    });
}

/// assign a bare token to the next positional, or a trailing/variadic sink,
/// returning whether a trailing arg took it and so owns the rest of the line
fn positional<'a>(
    matches: &mut Matches<'a>,
    positionals: &[&ArgTarget],
    cursor: &mut usize,
    tok: &'a str,
) -> Result<bool, ErrorKind> {
    let Some(target) = next_positional(positionals, *cursor) else {
        return Err(ErrorKind::UnexpectedPositional(tok.to_owned()));
    };

    let arg = target.arg;
    let slot = target_slot_mut(matches, target);
    slot.values.extend(arg.split(tok));
    slot.count += 1;
    // single positional advances the cursor, a variadic one keeps eating
    if !(arg.multi || arg.kind == Kind::Trailing) {
        *cursor += 1;
    }
    Ok(arg.kind == Kind::Trailing)
}

/// enforce a list arg's arity. an absent arg with a fallback is held only to
/// `max_values`, by the values its delimiter splits the fallback into.
fn count_values(arg: &ArgSpec, got: usize, fallback: Option<Fallback>) -> Result<(), ErrorKind> {
    if got == 0
        && let Some(found) = fallback
    {
        let fallback_count = arg.split(&found.value).count();
        return match arg.max_values {
            Some(max) if fallback_count > max => {
                Err(ErrorKind::TooManyValues {
                    arg: source_name(arg, found.env),
                    max,
                    got: fallback_count,
                })
            },
            _ => Ok(()),
        };
    }
    if let Some(min) = arg.min_values
        && got < min
    {
        return Err(ErrorKind::TooFewValues {
            arg: arg.display_name(),
            min,
            got,
        });
    }
    if let Some(max) = arg.max_values
        && got > max
    {
        return Err(ErrorKind::TooManyValues {
            arg: arg.display_name(),
            max,
            got,
        });
    }
    Ok(())
}

/// whether an arg will have a value by the time it is read, counting an env
/// var only when it is actually set
fn supplied(spec: &CommandSpec, matches: &Matches, i: usize) -> bool {
    matches.slots[i].count > 0 || !matches.slots[i].values.is_empty() || fallback(spec, i).is_some()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Demands {
    Checked,
    Waived,
}

/// enforce `required` and group constraints. defaults resolve when a value is
/// read, so a defaulted arg never counts as missing here.
fn finalise(cmd: &Command<'_>, matches: &Matches, demands: Demands) -> Result<(), ErrorKind> {
    let spec = cmd.spec();
    let owed = if exclusive_alone(spec, matches)? {
        Demands::Waived
    } else {
        demands
    };
    finalise_args(spec, matches, owed)?;
    finalise_groups(spec, matches, owed)?;
    let missing = spec.has_subs() && !selected_command(matches) && !spec.subcommand_optional();
    if missing && owed == Demands::Checked {
        return Err(ErrorKind::MissingSubcommand {
            help: Some(help::render(cmd, false)),
        });
    }
    Ok(())
}

fn demand_selected(spec: &'static CommandSpec, matches: &Matches) -> Result<(), ErrorKind> {
    let Some((index, chosen)) = matches.sub() else {
        return Ok(());
    };
    let sub = &spec.subs[index];
    if sub.flattened {
        return demand_selected(sub.spec, chosen);
    }
    if sub.external {
        return Ok(());
    }
    demand(sub.spec, chosen)
}

/// [`finalise`] with every demand checked, down through the subcommands this
/// line selected, for a command that had not been parsed before
fn demand(spec: &'static CommandSpec, matches: &Matches) -> Result<(), ErrorKind> {
    let owed = if exclusive_alone(spec, matches)? {
        Demands::Waived
    } else {
        Demands::Checked
    };
    finalise_args(spec, matches, owed)?;
    finalise_groups(spec, matches, owed)?;
    let Some((selector, selected)) = selector_matches(spec, matches) else {
        return Ok(());
    };
    match selected.sub() {
        Some(_) => demand_selected(selector, selected),
        None if spec.subcommand_optional() || owed == Demands::Waived => Ok(()),
        None => Err(ErrorKind::MissingSubcommand { help: None }),
    }
}

/// the spec declaring this command's subcommands and the matches read against
/// it, which may sit inside a flattened struct
fn selector_matches<'m, 'a>(
    spec: &'static CommandSpec,
    matches: &'m Matches<'a>,
) -> Option<(&'static CommandSpec, &'m Matches<'a>)> {
    if !spec.subs.is_empty() {
        return Some((spec, matches));
    }
    spec.flattened
        .iter()
        .zip(&matches.flattened)
        .find_map(|(inner, inner_m)| selector_matches(inner, inner_m))
}

/// whether an exclusive arg was given, alone. given with another arg, it is
/// an error.
fn exclusive_alone(spec: &CommandSpec, matches: &Matches) -> Result<bool, ErrorKind> {
    let mut given = Vec::new();
    spec.visit(&mut Vec::new(), &mut |path, index, arg| {
        let owner = path.iter().fold(matches, |owner, &i| &owner.flattened[i]);
        if owner.slots[index].count > 0 || owner.slots[index].negated {
            given.push(arg);
        }
    });

    let Some(exclusive) = given.iter().find(|arg| arg.exclusive) else {
        return Ok(false);
    };
    given
        .iter()
        .find(|arg| !ptr::eq(**arg, *exclusive))
        .map_or(Ok(true), |other| {
            Err(ErrorKind::Exclusive {
                arg:   exclusive.display_name(),
                other: other.display_name(),
            })
        })
}

/// whether a subcommand was chosen here or inside a flattened struct
fn selected_command(matches: &Matches) -> bool {
    matches.sub.is_some() || matches.flattened.iter().any(selected_command)
}

fn finalise_args(spec: &CommandSpec, matches: &Matches, demands: Demands) -> Result<(), ErrorKind> {
    for (i, arg) in spec.args.iter().enumerate() {
        #[cfg(feature = "std")]
        if !matches.given(i) && matches.slots[i].values.is_empty() {
            env_readable(arg)?;
        }
        if !supplied(spec, matches, i) && arg.required && demands == Demands::Checked {
            return Err(ErrorKind::MissingRequired(arg.display_name()));
        }
        count_values(arg, matches.slots[i].values.len(), fallback(spec, i))?;
    }

    let unmet = |&&(arg, needs): &&(usize, usize)| {
        matches.slots[arg].count > 0 && !supplied(spec, matches, needs)
    };

    if let Some(&(arg, _)) = spec.requires.iter().find(unmet) {
        let needs = spec
            .requires
            .iter()
            .filter(|pair| pair.0 == arg && unmet(pair))
            .map(|&(_, needs)| spec.args[needs].display_name())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(ErrorKind::Requires {
            arg: spec.args[arg].display_name(),
            needs,
        });
    }

    for &(first, second) in spec.conflicts {
        if matches.slots[first].count > 0 && matches.slots[second].count > 0 {
            return Err(ErrorKind::Conflict {
                group:  String::new(),
                first:  spec.args[first].display_name(),
                second: spec.args[second].display_name(),
            });
        }
    }

    for (inner, inner_matches) in spec.flattened.iter().zip(&matches.flattened) {
        finalise_args(inner, inner_matches, demands)?;
    }

    Ok(())
}

/// a group spans every flattened struct at a command level, so each name is
/// checked once against all its members, required if any declaration says so
fn finalise_groups(
    spec: &CommandSpec,
    matches: &Matches,
    demands: Demands,
) -> Result<(), ErrorKind> {
    for (name, required) in command_groups(spec) {
        let members = group_members(spec, matches, name);
        let set: Vec<String> = members
            .iter()
            .filter(|(_, slot)| slot.count > 0)
            .map(|(arg, _)| arg.display_name())
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
                .map(|(arg, _)| arg.display_name())
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
    matches: &'m Matches<'a>,
    group: &str,
) -> Vec<(&'static ArgSpec, &'m Slot<'a>)> {
    let mut members = Vec::new();
    spec.visit(&mut Vec::new(), &mut |path, index, arg| {
        if arg.group == Some(group) {
            let owner = path.iter().fold(matches, |owner, &i| &owner.flattened[i]);
            members.push((arg, &owner.slots[index]));
        }
    });
    members
}

fn target_slot<'m, 'a>(matches: &'m Matches<'a>, target: &ArgTarget) -> &'m Slot<'a> {
    let owner = target
        .path
        .iter()
        .fold(matches, |owner, &i| &owner.flattened[i]);
    &owner.slots[target.index]
}

fn target_slot_mut<'m, 'a>(matches: &'m mut Matches<'a>, target: &ArgTarget) -> &'m mut Slot<'a> {
    let owner = target
        .path
        .iter()
        .fold(matches, |owner, &i| &mut owner.flattened[i]);
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
        .filter(|arg| !arg.hidden)
        .flat_map(|arg| {
            arg.long
                .into_iter()
                .chain(arg.negate)
                .chain(arg.aliases.iter().copied())
        })
        .chain(help.then_some("help"))
        .chain(version.then_some("version"))
}

fn sub_names(sub: &SubSpec) -> impl Iterator<Item = &str> {
    iter::once(sub.name).chain(sub.aliases.iter().copied())
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

fn edit_distance(left: &str, right: &str) -> usize {
    let width = right.chars().count();
    let mut prev: Vec<usize> = (0..=width).collect();
    let mut cur = vec![0; width + 1];
    for (i, ca) in left.chars().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in right.chars().enumerate() {
            let substitute = prev[j] + usize::from(ca != cb);
            cur[j + 1] = substitute.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        mem::swap(&mut prev, &mut cur);
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

    fn argv<'a>(args: &[&'a str]) -> Vec<&'a str> {
        args.to_vec()
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

    fn parse<'a>(spec: &CommandSpec, args: &[&'a str]) -> Result<Matches<'a>, ErrorKind> {
        parse_spec(spec, argv(args)).map_err(|error| error.kind)
    }

    #[test]
    fn longs_shorts_counts_positionals() {
        let matches = parse(&FLAT, &[
            "--force", "--dir", "/x", "-vv", "alpha", "beta", "gamma",
        ])
        .unwrap();
        assert!(matches.flag(0));
        assert_eq!(matches.raw(1), Some("/x"));
        assert_eq!(matches.count(2), 2);
        assert_eq!(matches.raw(3), Some("alpha"));
        assert_eq!(matches.raws(4), ["beta", "gamma"]);
    }

    #[test]
    fn short_cluster_and_attached_value() {
        let matches = parse(&FLAT, &["-vvf", "name"]).unwrap();
        assert_eq!(matches.count(2), 2);
        assert!(matches.flag(0));

        let attached = parse(&FLAT, &["-d/x", "name"]).unwrap();
        assert_eq!(attached.raw(1), Some("/x"));

        let long_form = parse(&FLAT, &["--dir=/y", "name"]).unwrap();
        assert_eq!(long_form.raw(1), Some("/y"));
    }

    #[test]
    fn last_wins_for_single_option() {
        let matches = parse(&FLAT, &["--dir", "/a", "--dir", "/b", "name"]).unwrap();
        assert_eq!(matches.raw(1), Some("/b"));
    }

    #[test]
    fn double_dash_forces_positionals() {
        let matches = parse(&FLAT, &["--", "--weird", "-x"]).unwrap();
        assert_eq!(matches.raw(3), Some("--weird"));
        assert_eq!(matches.raws(4), ["-x"]);
    }

    #[test]
    fn trailing_takes_the_rest_after_its_first_value() {
        const ARGS: &[ArgSpec] = &[
            ArgSpec::new(Kind::Flag).long("force"),
            ArgSpec::new(Kind::Trailing).value_name("cmd"),
        ];
        let wrap = CommandSpec { args: ARGS, ..FLAT };
        let matches = parse(&wrap, &["--force", "ls", "-la", "--force"]).unwrap();
        assert!(matches.flag(0));
        assert_eq!(matches.raws(1), ["ls", "-la", "--force"]);
    }

    #[test]
    fn an_exclusive_arg_stands_alone_and_lifts_requirements() {
        const ARGS: &[ArgSpec] = &[
            ArgSpec::new(Kind::Flag).long("list").exclusive(),
            ArgSpec::new(Kind::Flag).long("force"),
            ArgSpec::new(Kind::Positional).value_name("name").required(),
        ];
        let tool = CommandSpec { args: ARGS, ..FLAT };
        parse(&tool, &["--list"]).unwrap();
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
            _ => panic!("expected an unknown argument"),
        }
        // too far off to be a typo
        match parse(&FLAT, &["--wibble"]) {
            Err(ErrorKind::Unknown { closest, .. }) => assert_eq!(closest, None),
            _ => panic!("expected an unknown argument"),
        }
    }

    #[test]
    fn help_and_version_signals() {
        const HASHED: CommandSpec = CommandSpec::new("flat").version("0.1.0").hash("abc123");
        const HASH_ONLY: CommandSpec = CommandSpec::new("flat").hash("abc123");

        assert!(matches!(parse(&FLAT, &["--help"]), Err(ErrorKind::Help(_))));
        assert!(matches!(parse(&FLAT, &["-h"]), Err(ErrorKind::Help(_))));
        match parse(&FLAT, &["--version"]) {
            Err(ErrorKind::Version(version)) => assert_eq!(version, "flat 0.1.0"),
            _ => panic!("expected version"),
        }

        match parse(&HASHED, &["--version"]) {
            Err(ErrorKind::Version(version)) => assert_eq!(version, "flat 0.1.0 (abc123)"),
            _ => panic!("expected version"),
        }

        match parse(&HASH_ONLY, &["-V"]) {
            Err(ErrorKind::Version(version)) => assert_eq!(version, "flat (abc123)"),
            _ => panic!("expected version"),
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
        let reject = |raw: &str| Err::<u8, _>(ValueError::new(raw, "bad"));
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
        let matches = parse(&SPEC, &[]).unwrap();
        assert_eq!(
            matches.optional::<String>(&SPEC, 0).unwrap().as_deref(),
            Some("info")
        );
        // a user value overrides the default
        let debug_matches = parse(&SPEC, &["--level", "debug"]).unwrap();
        assert_eq!(
            debug_matches
                .optional::<String>(&SPEC, 0)
                .unwrap()
                .as_deref(),
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

        let matches = parse(&SPEC, &["--color", "x.txt"]).unwrap();
        assert_eq!(matches.raw(0), Some("always"));
        assert_eq!(matches.raw(1), Some("x.txt"));

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
        parse(&OPT, &["--flake"]).unwrap();
        parse(&OPT, &[]).unwrap(); // not required, zero is fine
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

        parse(&SPEC, &["a"]).unwrap();
        parse(&SPEC, &["a", "b"]).unwrap();
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

        parse(&SPEC, &[]).unwrap();
        parse(&SPEC, &["--format", "json"]).unwrap();
        parse(&SPEC, &["--output", "f", "--format", "json"]).unwrap();
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
        parse(&SPEC, &["--a"]).unwrap();
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
        let matches = parse(&ROOT, &["add", "serde", "https://x", "--force"]).unwrap();
        let (idx, sub) = matches.sub().expect("a subcommand");
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
        let matches = parse(&MIXED, &["p1", "a1", "add", "serde", "https://x"]).unwrap();
        assert_eq!(matches.raw(0), Some("p1"));
        assert_eq!(matches.raw(1), Some("a1"));
        let (idx, sub) = matches.sub().expect("a subcommand");
        assert_eq!(idx, 0);
        assert_eq!(sub.raw(0), Some("serde"));

        // an unfilled required positional still wins the token
        let filled_positionals =
            parse(&MIXED, &["p1", "add", "add", "serde", "https://x"]).unwrap();
        assert_eq!(filled_positionals.raw(1), Some("add"));
        assert!(filled_positionals.sub().is_some());

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

        let matches = parse(&root, &["frob", "--force", "x"]).unwrap();
        let (index, ext) = matches.sub().unwrap();
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

        let unknown_err = parse_spec(&ROOT, argv(&["--nope"])).unwrap_err();
        assert!(
            unknown_err
                .usage
                .unwrap()
                .to_string()
                .starts_with("Usage: prog")
        );
    }
}
