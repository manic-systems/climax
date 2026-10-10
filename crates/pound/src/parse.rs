// SPDX-License-Identifier: EUPL-1.2

//! walks `argv` against a [`CommandSpec`] and produces [`Matches`]

use alloc::{borrow::Cow, vec::IntoIter};

#[cfg(not(feature = "std"))]
use crate::alloc_prelude::*;
use crate::{
    error::{Error, ErrorKind},
    help,
    spec::{
        ArgSpec,
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
    count: u32,
    /// the last spelling seen was the arg's negation
    negated: bool,
    values: Vec<&'a str>,
}

/// a successful parse
#[derive(Debug)]
pub struct Matches<'a> {
    slots:     Vec<Slot<'a>>,
    flattened: Vec<Self>,
    sub:       Option<(usize, Box<Self>)>,
    /// the deepest command this parse selected, for errors raised afterwards
    selected:  Option<Invocation>,
}

/// where a selected command sits, enough to render its usage line on demand
#[derive(Debug)]
struct Invocation {
    spec:    &'static CommandSpec,
    path:    Vec<&'static str>,
    globals: Vec<&'static ArgSpec>,
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
        fn search(
            subs: &'static [SubSpec],
            name: &str,
            chain: &mut Vec<(usize, &'static SubSpec)>,
        ) -> bool {
            for (i, sub) in subs.iter().enumerate() {
                chain.push((i, sub));
                let hit = if sub.flattened {
                    search(sub.spec.subs, name, chain)
                } else {
                    sub.name == name || sub.aliases.contains(&name)
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
                let mut target = Self::find(inner, name)?;
                target.path.insert(0, i);
                Some(target)
            });
        }
        let mut wrappers = Vec::new();
        if !search(spec.subs, name, &mut wrappers) {
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
        }
    }

    /// tag `err` with the usage line and help spelling of the deepest command
    /// selected, or of `root` when none was
    pub(crate) fn locate(&self, root: &CommandSpec, err: Error) -> Error {
        err.or_usage(|| match &self.selected {
            Some(inv) => (
                help::usage_line(inv.spec, &inv.path, &inv.globals),
                help_flag(inv.spec, &inv.globals),
            ),
            None => (help::usage_line(root, &[], &[]), help_flag(root, &[])),
        })
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
    #[must_use]
    pub fn switch(&self, spec: &CommandSpec, i: usize) -> bool {
        let slot = &self.slots[i];
        if slot.negated {
            return false;
        }
        if slot.count > 0 {
            return true;
        }
        matches!(fallback(spec, i).as_deref(), Some("1" | "true" | "yes"))
    }

    /// how many times a count flag was supplied
    #[must_use]
    pub fn count(&self, i: usize) -> u32 {
        self.slots[i].count
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
        parse_fallback(spec, i, &convert).unwrap_or_else(|| {
            Err(ErrorKind::MissingRequired(spec.args[i].display_name()).into())
        })
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
        Ok(parse_fallback(spec, i, &convert)
            .transpose()?
            .into_iter()
            .collect())
    }
}

/// the value to use when an arg was not given
fn fallback(spec: &CommandSpec, i: usize) -> Option<Cow<'static, str>> {
    match env_value(spec, i) {
        Some((_, val)) => Some(Cow::Owned(val)),
        None => spec.args[i].default.map(Cow::Borrowed),
    }
}

/// the env var an arg falls back to and its value, when the var is set
#[cfg(feature = "std")]
fn env_value(spec: &CommandSpec, i: usize) -> Option<(&'static str, String)> {
    let var = spec.args[i].env?;
    std::env::var(var).ok().map(|val| (var, val))
}

#[cfg(not(feature = "std"))]
const fn env_value(_spec: &CommandSpec, _i: usize) -> Option<(&'static str, String)> {
    None
}

/// convert the fallback value, naming the env var in the error when that is
/// where the value came from
fn parse_fallback<T>(
    spec: &CommandSpec,
    i: usize,
    convert: &impl Fn(&str) -> Result<T, ValueError>,
) -> Option<Result<T, Error>> {
    if let Some((var, val)) = env_value(spec, i) {
        return Some(parse_with(spec, i, &val, Some(var), convert));
    }
    spec.args[i]
        .default
        .map(|default| parse_with(spec, i, default, None, convert))
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
        if let Some(values) = spec.args[i].possible
            && !values.is_empty()
        {
            msg = format!("{msg} (possible values: {})", values.join(", "));
        }
        let arg = match env {
            Some(var) => format!("{} (from ${var})", spec.args[i].display_name()),
            None => spec.args[i].display_name(),
        };
        ErrorKind::Value {
            arg,
            value: e.value,
            msg,
        }
        .into()
    })
}

/// entrypoint. parse `args[1..]` against `spec`
pub(crate) fn parse_spec<'a>(
    spec: &CommandSpec,
    args: impl IntoIterator<Item = &'a str>,
) -> Result<Matches<'a>, Error> {
    let mut it = args.into_iter().collect::<Vec<_>>().into_iter();
    let mut hits = Vec::new();
    parse_cmd(spec, &[], &mut it, &[], &mut hits)
}

/// parse one command, tagging whatever went wrong with this command's usage
/// line unless a nested command already claimed it
fn parse_cmd<'a>(
    spec: &CommandSpec,
    path: &[&'static str],
    it: &mut IntoIter<&'a str>,
    globals: &[&'static ArgSpec],
    hits: &mut Vec<GlobalHit<'a>>,
) -> Result<Matches<'a>, Error> {
    walk_cmd(spec, path, it, globals, hits).map_err(|e| {
        e.or_usage(|| {
            (
                help::usage_line(spec, path, globals),
                help_flag(spec, globals),
            )
        })
    })
}

/// the spelling that still reaches `spec`'s generated help, if any does
pub(crate) fn help_flag(spec: &CommandSpec, globals: &[&ArgSpec]) -> Option<&'static str> {
    let own: Vec<&ArgSpec> = spec.arguments().collect();
    if !claims_long(&own, globals, "help") {
        Some("--help")
    } else if !claims_short(&own, globals, 'h') {
        Some("-h")
    } else {
        None
    }
}

fn walk_cmd<'a>(
    spec: &CommandSpec,
    path: &[&'static str],
    it: &mut IntoIter<&'a str>,
    globals: &[&'static ArgSpec],
    hits: &mut Vec<GlobalHit<'a>>,
) -> Result<Matches<'a>, Error> {
    let mut targets = Vec::new();
    spec.visit(&mut Vec::new(), &mut |path, index, arg| {
        targets.push(ArgTarget {
            path: path.to_vec(),
            index,
            arg,
        });
    });
    let mut m = Matches::new(spec);

    let positionals: Vec<&ArgTarget> = targets.iter().filter(|t| t.arg.is_positional()).collect();
    let mut pos_cursor = 0_usize;
    let mut only_positional = false;

    let builtin = |ch| builtin_short(spec, path, &targets, ch, globals);

    while let Some(tok) = it.next() {
        if only_positional {
            positional(&mut m, &positionals, &mut pos_cursor, tok)?;
            continue;
        }

        if tok == "--" {
            only_positional = true;
        } else if let Some(long) = tok.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v)),
                None => (long, None),
            };
            if let Some(sig) = builtin_long(spec, path, &targets, name, globals) {
                return Err(sig.into());
            }
            if let Some(target) = targets
                .iter()
                .find(|t| t.arg.answers_long(name) && t.arg.negate != Some(name))
            {
                apply_named(&mut m, target, inline, it)?;
            } else if let Some(target) = targets.iter().find(|t| t.arg.negate == Some(name)) {
                apply_negation(target_slot_mut(&mut m, target), name, inline)?;
            } else if let Some((index, g)) = find_global_long(globals, name) {
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
            } else {
                return Err(ErrorKind::Unknown {
                    arg: format!("--{name}"),
                    closest: closest_long(spec, globals, name),
                }
                .into());
            }
        } else if let Some(rest) = tok.strip_prefix('-').filter(|r| !r.is_empty()) {
            let first = rest.chars().next().unwrap_or('-');
            let known = targets.iter().any(|t| t.arg.answers_short(first))
                || builtin(first).is_some()
                || find_global_short(globals, first).is_some();
            if known {
                shorts(&targets, &mut m, rest, it, globals, hits, builtin)?;
            } else if first.is_ascii_digit() || first == '.' {
                // a negative number is a value, not a flag
                positional(&mut m, &positionals, &mut pos_cursor, tok)?;
            } else {
                return Err(unknown(format!("-{first}")).into());
            }
        } else if let Some(target) = sub_dispatch(spec, &positionals, pos_cursor, tok)? {
            let mut child_globals: Vec<&'static ArgSpec> = globals.to_vec();
            child_globals.extend(targets.iter().map(|t| t.arg).filter(|a| a.global));
            let child_path: Vec<&'static str> = path.iter().copied().chain([spec.name]).collect();
            let mut sub_m =
                parse_cmd(target.command.spec, &child_path, it, &child_globals, hits)?;
            m.selected = sub_m.selected.take().or(Some(Invocation {
                spec:    target.command.spec,
                path:    child_path,
                globals: child_globals,
            }));
            target.store(&mut m, sub_m);
            break; // subcommand owns the rest
        } else {
            positional(&mut m, &positionals, &mut pos_cursor, tok)?;
        }
    }

    // must run before finalise so owned globals count toward required/group checks
    apply_global_hits(&targets, globals.len(), &mut m, hits);
    finalise(spec, path, &m, globals)?;
    Ok(m)
}

/// a bare token names a subcommand only once every required positional is filled,
/// and never past a variadic one, which stays greedy so its tail can fill
fn sub_dispatch(
    spec: &CommandSpec,
    positionals: &[&ArgTarget],
    cursor: usize,
    tok: &str,
) -> Result<Option<SubTarget>, Error> {
    if !spec.has_subs() {
        return Ok(None);
    }
    let next = positionals.get(cursor).map(|t| t.arg);
    if next.is_some_and(|a| a.multi || a.kind == Kind::Trailing) {
        return Ok(None);
    }
    if positionals.iter().skip(cursor).any(|t| t.arg.required) {
        return Ok(None);
    }
    match SubTarget::find(spec, tok) {
        Some(target) => Ok(Some(target)),
        None if next.is_some() => Ok(None), // an open optional slot can still take it
        None => {
            Err(ErrorKind::UnknownSubcommand {
                name:    tok.to_owned(),
                closest: closest(
                    spec.subcommands().filter(|s| !s.hidden).flat_map(sub_names),
                    tok,
                ),
            }
            .into())
        },
    }
}

/// apply a long option
fn apply_named<'a>(
    m: &mut Matches<'a>,
    target: &ArgTarget,
    inline: Option<&'a str>,
    it: &mut IntoIter<&'a str>,
) -> Result<(), ErrorKind> {
    let a = target.arg;
    let slot = target_slot_mut(m, target);
    match a.kind {
        Kind::Flag => {
            if inline.is_some() {
                return Err(ErrorKind::UnexpectedValue(a.display_name()));
            }
            slot.count = 1;
            slot.negated = false;
        },
        Kind::Count => {
            if inline.is_some() {
                return Err(ErrorKind::UnexpectedValue(a.display_name()));
            }
            slot.count += 1;
        },
        Kind::Opt => {
            let value = match inline {
                Some(v) => v,
                None => detached_value(a, it)?,
            };
            push_value(a, slot, value);
        },
        Kind::Positional | Kind::Trailing => return Err(unknown(a.display_name())),
    }
    Ok(())
}

/// apply a cluster of short args
fn shorts<'a>(
    targets: &[ArgTarget],
    m: &mut Matches<'a>,
    cluster: &'a str,
    it: &mut IntoIter<&'a str>,
    globals: &[&'static ArgSpec],
    hits: &mut Vec<GlobalHit<'a>>,
    builtin: impl Fn(char) -> Option<ErrorKind>,
) -> Result<(), ErrorKind> {
    for (off, ch) in cluster.char_indices() {
        if let Some(sig) = builtin(ch) {
            return Err(sig);
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
                    return Ok(()); // option swallowed the cluster tail
                },
                Kind::Positional | Kind::Trailing => {
                    return Err(unknown(format!("-{ch}")));
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
                    hits.push(GlobalHit {
                        index,
                        value: Some(value),
                        negated: false,
                    });
                    return Ok(());
                },
                Kind::Positional | Kind::Trailing => {
                    return Err(unknown(format!("-{ch}")));
                },
            }
        } else {
            return Err(unknown(format!("-{ch}")));
        }
    }
    Ok(())
}

/// a short option's value
fn cluster_value<'a>(
    cluster: &'a str,
    off: usize,
    ch: char,
    it: &mut IntoIter<&'a str>,
    a: &ArgSpec,
) -> Result<&'a str, ErrorKind> {
    let rest = &cluster[off + ch.len_utf8()..];
    if rest.is_empty() {
        detached_value(a, it)
    } else {
        Ok(rest)
    }
}

/// the value for an option written without an attached one. an option that
/// declares `default_missing` takes that instead of eating the next token.
fn detached_value<'a>(a: &ArgSpec, it: &mut IntoIter<&'a str>) -> Result<&'a str, ErrorKind> {
    if let Some(missing) = a.default_missing {
        return Ok(missing);
    }
    it.next()
        .ok_or_else(|| ErrorKind::MissingValue(a.display_name()))
}

fn push_value<'a>(a: &ArgSpec, slot: &mut Slot<'a>, value: &'a str) {
    if !a.multi {
        slot.values.clear(); // last wins for single-valued opts
    }
    slot.values.push(value);
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
    it: &mut IntoIter<&'a str>,
    hits: &mut Vec<GlobalHit<'a>>,
) -> Result<(), ErrorKind> {
    match g.kind {
        Kind::Flag | Kind::Count => {
            if inline.is_some() {
                return Err(ErrorKind::UnexpectedValue(g.display_name()));
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
            hits.push(GlobalHit {
                index,
                value: Some(value),
                negated: false,
            });
        },
        Kind::Positional | Kind::Trailing => return Err(unknown(g.display_name())),
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

/// assign a bare token to the next positional, or a trailing/variadic sink
fn positional<'a>(
    m: &mut Matches<'a>,
    positionals: &[&ArgTarget],
    cursor: &mut usize,
    tok: &'a str,
) -> Result<(), ErrorKind> {
    let target = if *cursor < positionals.len() {
        positionals[*cursor]
    } else if let Some(&last) = positionals.last() {
        let a = last.arg;
        if a.multi || a.kind == Kind::Trailing {
            last // overflow lands in the variadic tail
        } else {
            return Err(ErrorKind::UnexpectedPositional(tok.to_owned()));
        }
    } else {
        return Err(ErrorKind::UnexpectedPositional(tok.to_owned()));
    };

    let a = target.arg;
    let slot = target_slot_mut(m, target);
    slot.values.push(tok);
    slot.count += 1;
    // single positional advances the cursor, a variadic one keeps eating
    if !(a.multi || a.kind == Kind::Trailing) {
        *cursor += 1;
    }
    Ok(())
}

/// enforce a list arg's arity. an absent arg with a fallback is left alone,
/// since the fallback supplies the one value the reader will see.
fn count_values(a: &ArgSpec, got: usize, has_fallback: bool) -> Result<(), ErrorKind> {
    let filled_by_fallback = got == 0 && has_fallback;
    if let Some(min) = a.min_values
        && got < min
        && !filled_by_fallback
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

/// enforce `required` and group constraints. defaults resolve when a value is
/// read, so a defaulted arg never counts as missing here.
fn finalise(
    spec: &CommandSpec,
    path: &[&'static str],
    m: &Matches,
    globals: &[&'static ArgSpec],
) -> Result<(), ErrorKind> {
    finalise_args(spec, m)?;
    finalise_groups(spec, m)?;
    if spec.has_subs() && !selected_command(m) && !spec.subcommand_optional() {
        // empty/sub-less invocation shows help rather than a bare error
        return Err(ErrorKind::Help(help::render(spec, path, globals, false)));
    }
    Ok(())
}

/// whether a subcommand was chosen here or inside a flattened struct
fn selected_command(m: &Matches) -> bool {
    m.sub.is_some() || m.flattened.iter().any(selected_command)
}

fn finalise_args(spec: &CommandSpec, m: &Matches) -> Result<(), ErrorKind> {
    for (i, a) in spec.args.iter().enumerate() {
        if !supplied(spec, m, i) && a.required {
            return Err(ErrorKind::MissingRequired(a.display_name()));
        }
        count_values(a, m.slots[i].values.len(), fallback(spec, i).is_some())?;
    }

    for &(a, b) in spec.requires {
        if m.slots[a].count > 0 && !supplied(spec, m, b) {
            return Err(ErrorKind::Requires {
                arg: spec.args[a].display_name(),
                needs: spec.args[b].display_name(),
            });
        }
    }

    for &(a, b) in spec.conflicts {
        if m.slots[a].count > 0 && m.slots[b].count > 0 {
            return Err(ErrorKind::Conflict {
                group: String::new(),
                first: spec.args[a].display_name(),
                second: spec.args[b].display_name(),
            });
        }
    }

    for (inner, inner_matches) in spec.flattened.iter().zip(&m.flattened) {
        finalise_args(inner, inner_matches)?;
    }

    Ok(())
}

/// a group spans every flattened struct at a command level, so each name is
/// checked once against all its members, required if any declaration says so
fn finalise_groups(spec: &CommandSpec, m: &Matches) -> Result<(), ErrorKind> {
    for (name, required) in command_groups(spec) {
        let members = group_members(spec, m, name);
        let set: Vec<String> = members
            .iter()
            .filter(|(_, slot)| slot.count > 0)
            .map(|(a, _)| a.display_name())
            .collect();
        if set.len() > 1 {
            return Err(ErrorKind::Conflict {
                group: name.to_owned(),
                first: set[0].clone(),
                second: set[1].clone(),
            });
        }
        if set.is_empty() && required {
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

fn builtin_long(
    spec: &CommandSpec,
    path: &[&'static str],
    targets: &[ArgTarget],
    name: &str,
    globals: &[&'static ArgSpec],
) -> Option<ErrorKind> {
    let mut claimants = targets.iter().map(|t| t.arg).chain(globals.iter().copied());
    if claimants.any(|a| a.answers_long(name)) {
        return None;
    }
    match name {
        "help" => Some(ErrorKind::Help(help::render(spec, path, globals, true))),
        "version" if spec.has_version_info() => Some(ErrorKind::Version(help::version_line(spec))),
        _ => None,
    }
}

fn builtin_short(
    spec: &CommandSpec,
    path: &[&'static str],
    targets: &[ArgTarget],
    ch: char,
    globals: &[&'static ArgSpec],
) -> Option<ErrorKind> {
    let mut claimants = targets.iter().map(|t| t.arg).chain(globals.iter().copied());
    if claimants.any(|a| a.answers_short(ch)) {
        return None;
    }
    match ch {
        'h' => Some(ErrorKind::Help(help::render(spec, path, globals, false))),
        'V' if spec.has_version_info() => Some(ErrorKind::Version(help::version_line(spec))),
        _ => None,
    }
}

/// an unrecognized spelling with no suggestion worth offering
const fn unknown(arg: String) -> ErrorKind {
    ErrorKind::Unknown { arg, closest: None }
}

/// every long spelling this command answers to, including the implicit ones
fn long_names<'s>(
    spec: &'s CommandSpec,
    globals: &'s [&'static ArgSpec],
) -> impl Iterator<Item = &'s str> {
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

fn closest_long(spec: &CommandSpec, globals: &[&'static ArgSpec], name: &str) -> Option<String> {
    closest(long_names(spec, globals), name).map(|found| format!("--{found}"))
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
    use crate::spec::{GroupSpec, SubSpec};

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
        version:        "0.1.0",
        hash:           None,
        long_about:     "",
        about:          "a flat command",
        args:           FLAT_ARGS,
        flattened:      &[],
        argument_order: &[],
        groups:         &[],
        conflicts:      &[],
        requires:       &[],
        subs:           &[],
        sub_optional:   false,
    };

    fn parse<'a>(spec: &CommandSpec, a: &[&'a str]) -> Result<Matches<'a>, ErrorKind> {
        parse_spec(spec, argv(a)).map_err(|e| e.kind)
    }

    #[test]
    fn longs_shorts_counts_positionals() {
        let m = parse(
            &FLAT,
            &["--force", "--dir", "/x", "-vv", "alpha", "beta", "gamma"],
        )
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
    fn positionals_are_named_as_usage_names_them() {
        let Err(ErrorKind::MissingRequired(name)) = parse(&FLAT, &[]) else {
            panic!("expected a missing positional");
        };
        assert_eq!(name, "NAME");
        assert_eq!(FLAT_ARGS[1].display_name(), "--dir");
        assert_eq!(FLAT_ARGS[0].display_name(), "--force");
    }

    #[test]
    fn errors() {
        assert!(matches!(parse(&FLAT, &["--nope"]), Err(ErrorKind::Unknown { .. })));
        assert!(matches!(parse(&FLAT, &["-q"]), Err(ErrorKind::Unknown { .. })));
        assert_eq!(parse(&FLAT, &["-5"]).unwrap().raw(3), Some("-5"));
        assert!(matches!(
            parse(&FLAT, &["name", "--dir"]),
            Err(ErrorKind::MissingValue(_))
        ));
        assert!(matches!(parse(&FLAT, &[]), Err(ErrorKind::MissingRequired(_))));
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
        const ARGS: &[ArgSpec] = &[ArgSpec::new(Kind::Opt)
            .long("replicas")
            .env("POUND_TEST_BAD_REPLICAS")];
        const SPEC: CommandSpec = CommandSpec::new("e").args(ARGS);
        // SAFETY: no other test reads this variable.
        unsafe { std::env::set_var("POUND_TEST_BAD_REPLICAS", "99") };
        let from_env = parse(&SPEC, &[])
            .unwrap()
            .required_map::<u8>(&SPEC, 0, |_| Err(ValueError::new("99", "too many")));
        let from_cli = parse(&SPEC, &["--replicas", "99"])
            .unwrap()
            .required_map::<u8>(&SPEC, 0, |_| Err(ValueError::new("99", "too many")));
        unsafe { std::env::remove_var("POUND_TEST_BAD_REPLICAS") };
        assert_eq!(
            from_env.unwrap_err().to_string(),
            "invalid value '99' for --replicas (from $POUND_TEST_BAD_REPLICAS): too many"
        );
        assert_eq!(
            from_cli.unwrap_err().to_string(),
            "invalid value '99' for --replicas: too many"
        );
    }

    #[test]
    fn defaults_resolve_in_readers() {
        const ARGS: &[ArgSpec] = &[ArgSpec::new(Kind::Opt).long("level").default("info")];
        const SPEC: CommandSpec = CommandSpec {
            name:           "d",
            version:        "",
            hash:           None,
            long_about:     "",
            about:          "",
            args:           ARGS,
            flattened:      &[],
            argument_order: &[],
            groups:         &[],
            conflicts:      &[],
            requires:       &[],
            subs:           &[],
            sub_optional:   false,
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

        assert!(parse(&SPEC, &[]).unwrap().switch(&SPEC, 0));
        assert!(!parse(&SPEC, &["--no-color"]).unwrap().switch(&SPEC, 0));
        // the last spelling on the line wins
        assert!(
            parse(&SPEC, &["--no-color", "--color"])
                .unwrap()
                .switch(&SPEC, 0)
        );
        assert!(matches!(
            parse(&SPEC, &["--no-color=1"]),
            Err(ErrorKind::UnexpectedValue(_))
        ));
    }

    #[test]
    fn optional_value_leaves_the_next_token_alone() {
        const ARGS: &[ArgSpec] = &[
            ArgSpec::new(Kind::Opt).long("color").default_missing("always"),
            ArgSpec::new(Kind::Positional).value_name("file"),
        ];
        const SPEC: CommandSpec = CommandSpec::new("o").args(ARGS);

        let m = parse(&SPEC, &["--color", "x.txt"]).unwrap();
        assert_eq!(m.raw(0), Some("always"));
        assert_eq!(m.raw(1), Some("x.txt"));

        assert_eq!(parse(&SPEC, &["--color=never"]).unwrap().raw(0), Some("never"));
    }

    #[test]
    fn groups_conflict_and_require() {
        const ARGS: &[ArgSpec] = &[
            ArgSpec::new(Kind::Flag).long("flake").group("mode"),
            ArgSpec::new(Kind::Flag).long("fetch").group("mode"),
        ];
        const OPT: CommandSpec = CommandSpec {
            name:           "g",
            version:        "",
            hash:           None,
            long_about:     "",
            about:          "",
            args:           ARGS,
            flattened:      &[],
            argument_order: &[],
            groups:         &[GroupSpec::new("mode")],
            conflicts:      &[],
            requires:       &[],
            subs:           &[],
            sub_optional:   false,
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
        assert!(matches!(parse(&REQ, &[]), Err(ErrorKind::MissingGroup { .. })));
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
            version:        "",
            hash:           None,
            long_about:     "",
            about:          "",
            args:           ARGS,
            flattened:      &[],
            argument_order: &[],
            groups:         &[],
            conflicts:      &[(0, 1)],
            requires:       &[],
            subs:           &[],
            sub_optional:   false,
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
        version:        "",
        hash:           None,
        long_about:     "",
        about:          "add a pin",
        args:           ADD_ARGS,
        flattened:      &[],
        argument_order: &[],
        groups:         &[],
        conflicts:      &[],
        requires:       &[],
        subs:           &[],
        sub_optional:   false,
    };
    const ROOT_SUBS: &[SubSpec] = &[SubSpec {
        name:      "add",
        aliases:   &[],
        about:     "add a pin",
        spec:      &ADD,
        hidden:    false,
        flattened: false,
    }];
    const ROOT: CommandSpec = CommandSpec {
        name:           "prog",
        version:        "1.0.0",
        hash:           None,
        long_about:     "",
        about:          "demo",
        args:           &[],
        flattened:      &[],
        argument_order: &[],
        groups:         &[],
        conflicts:      &[],
        requires:       &[],
        subs:           ROOT_SUBS,
        sub_optional:   false,
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
        // bare invocation shows help
        assert!(matches!(parse(&ROOT, &[]), Err(ErrorKind::Help(_))));
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
    fn subcommand_usage_carries_the_program_path() {
        let err = parse_spec(&ROOT, argv(&["add", "only"])).unwrap_err();
        assert_eq!(err.usage.as_deref(), Some(expected_add_usage()));

        let ErrorKind::Help(text) = parse(&ROOT, &["add", "--help"]).unwrap_err() else {
            panic!("expected help");
        };
        assert!(text.contains(expected_add_usage()), "{text}");

        let err = parse_spec(&ROOT, argv(&["--nope"])).unwrap_err();
        assert!(err.usage.unwrap().starts_with("Usage: prog"));
    }
}
