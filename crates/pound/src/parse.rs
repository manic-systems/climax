// SPDX-License-Identifier: EUPL-1.2

//! walks `argv` against a [`CommandSpec`] and produces [`Matches`]

use alloc::{borrow::Cow, vec::IntoIter};

#[cfg(not(feature = "std"))]
use crate::alloc_prelude::*;
use crate::{
    error::{Error, ErrorKind},
    help,
    spec::{ArgSpec, CommandSpec, Kind, SubSpec},
    value::{FromArg, ValueError},
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
    slots: Vec<Slot<'a>>,
    sub: Option<(usize, Box<Self>)>,
}

/// a global flag/option seen in a descendant
struct GlobalHit<'a> {
    arg: &'static ArgSpec,
    value: Option<&'a str>,
    negated: bool,
}

impl<'a> Matches<'a> {
    fn new(len: usize) -> Self {
        Self {
            slots: vec![Slot::default(); len],
            sub: None,
        }
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

    /// first raw value (or injected default), if any
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
            return parse_with(spec, i, s, &convert);
        }
        match fallback(spec, i) {
            Some(c) => parse_with(spec, i, &c, &convert),
            None => Err(ErrorKind::MissingRequired(spec.args[i].display_name()).into()),
        }
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
            return Ok(Some(parse_with(spec, i, s, &convert)?));
        }
        match fallback(spec, i) {
            Some(c) => Ok(Some(parse_with(spec, i, &c, &convert)?)),
            None => Ok(None),
        }
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
                .map(|&s| parse_with(spec, i, s, &convert))
                .collect();
        }
        match fallback(spec, i) {
            Some(c) => Ok(vec![parse_with(spec, i, &c, &convert)?]),
            None => Ok(Vec::new()),
        }
    }
}

/// the value to use when an arg was not given
fn fallback(spec: &CommandSpec, i: usize) -> Option<Cow<'static, str>> {
    #[cfg(feature = "std")]
    if let Some(var) = spec.args[i].env
        && let Ok(val) = std::env::var(var)
    {
        return Some(Cow::Owned(val));
    }
    spec.args[i].default.map(Cow::Borrowed)
}

fn parse_with<T>(
    spec: &CommandSpec,
    i: usize,
    s: &str,
    convert: &impl Fn(&str) -> Result<T, ValueError>,
) -> Result<T, Error> {
    convert(s).map_err(|e| {
        let mut msg = e.msg;
        if let Some(values) = spec.args[i].possible
            && !values.is_empty()
        {
            msg = format!("{msg} (possible values: {})", values.join(", "));
        }
        ErrorKind::Value {
            arg: spec.args[i].display_name(),
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
    parse_cmd(spec, &mut it, &[], &mut hits)
}

/// parse one command, tagging whatever went wrong with this command's usage
/// line unless a nested command already claimed it
fn parse_cmd<'a>(
    spec: &CommandSpec,
    it: &mut IntoIter<&'a str>,
    globals: &[&'static ArgSpec],
    hits: &mut Vec<GlobalHit<'a>>,
) -> Result<Matches<'a>, Error> {
    walk_cmd(spec, it, globals, hits).map_err(|e| e.or_usage(|| help::usage_line(spec, globals)))
}

fn walk_cmd<'a>(
    spec: &CommandSpec,
    it: &mut IntoIter<&'a str>,
    globals: &[&'static ArgSpec],
    hits: &mut Vec<GlobalHit<'a>>,
) -> Result<Matches<'a>, Error> {
    let mut m = Matches::new(spec.args.len());

    let positionals: Vec<usize> = spec
        .args
        .iter()
        .enumerate()
        .filter(|(_, a)| a.is_positional())
        .map(|(i, _)| i)
        .collect();
    let mut pos_cursor = 0_usize;
    let mut only_positional = false;

    while let Some(tok) = it.next() {
        if only_positional {
            positional(spec, &mut m, &positionals, &mut pos_cursor, tok)?;
            continue;
        }

        if tok == "--" {
            only_positional = true;
        } else if let Some(long) = tok.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v)),
                None => (long, None),
            };
            if let Some(sig) = builtin_long(spec, name, globals) {
                return Err(sig.into());
            }
            if let Some(idx) = spec.find_long(name) {
                apply_named(spec, &mut m, idx, inline, it)?;
            } else if let Some(idx) = spec.find_negate(name) {
                apply_negation(&mut m.slots[idx], name, inline)?;
            } else if let Some(g) = find_global_long(globals, name) {
                record_global(g, inline, it, hits)?;
            } else if let Some(g) = find_global_negate(globals, name) {
                if inline.is_some() {
                    return Err(ErrorKind::UnexpectedValue(format!("--{name}")).into());
                }
                hits.push(GlobalHit {
                    arg: g,
                    value: None,
                    negated: true,
                });
            } else {
                return Err(ErrorKind::Unknown {
                    arg: format!("--{name}"),
                    closest: closest_long(spec, globals, name),
                }
                .into());
            }
        } else if let Some(rest) = tok.strip_prefix('-').filter(|r| !r.is_empty()) {
            let first = rest.chars().next().unwrap_or('-');
            let known = spec.find_short(first).is_some()
                || builtin_short(spec, first, globals).is_some()
                || find_global_short(globals, first).is_some();
            if known {
                shorts(spec, &mut m, rest, it, globals, hits)?;
            } else {
                // not an option (negative numbers, lone values) -> positional
                positional(spec, &mut m, &positionals, &mut pos_cursor, tok)?;
            }
        } else if spec.has_subs() && positionals.is_empty() {
            let sidx = spec.find_sub(tok).ok_or_else(|| ErrorKind::UnknownSubcommand {
                name: tok.to_owned(),
                closest: closest(spec.subs.iter().flat_map(sub_names), tok),
            })?;
            let mut child_globals: Vec<&'static ArgSpec> = globals.to_vec();
            child_globals.extend(spec.args.iter().filter(|a| a.global));
            let sub_m = parse_cmd(spec.subs[sidx].spec, it, &child_globals, hits)?;
            m.sub = Some((sidx, Box::new(sub_m)));
            break; // subcommand owns the rest
        } else {
            positional(spec, &mut m, &positionals, &mut pos_cursor, tok)?;
        }
    }

    // must run before finalise so owned globals count toward required/group checks
    apply_global_hits(spec, &mut m, hits);
    finalise(spec, &m, globals)?;
    Ok(m)
}

/// apply a long option
fn apply_named<'a>(
    spec: &CommandSpec,
    m: &mut Matches<'a>,
    idx: usize,
    inline: Option<&'a str>,
    it: &mut IntoIter<&'a str>,
) -> Result<(), ErrorKind> {
    let a = spec.args[idx];
    match a.kind {
        Kind::Flag => {
            if inline.is_some() {
                return Err(ErrorKind::UnexpectedValue(a.display_name()));
            }
            m.slots[idx].count = 1;
            m.slots[idx].negated = false;
        },
        Kind::Count => {
            if inline.is_some() {
                return Err(ErrorKind::UnexpectedValue(a.display_name()));
            }
            m.slots[idx].count += 1;
        },
        Kind::Opt => {
            let value = match inline {
                Some(v) => v,
                None => detached_value(&a, it)?,
            };
            push_value(&a, &mut m.slots[idx], value);
        },
        Kind::Positional | Kind::Trailing => return Err(unknown(a.display_name())),
    }
    Ok(())
}

/// apply a cluster of short args
fn shorts<'a>(
    spec: &CommandSpec,
    m: &mut Matches<'a>,
    cluster: &'a str,
    it: &mut IntoIter<&'a str>,
    globals: &[&'static ArgSpec],
    hits: &mut Vec<GlobalHit<'a>>,
) -> Result<(), ErrorKind> {
    for (off, ch) in cluster.char_indices() {
        if let Some(sig) = builtin_short(spec, ch, globals) {
            return Err(sig);
        }
        if let Some(idx) = spec.find_short(ch) {
            let a = spec.args[idx];
            match a.kind {
                Kind::Flag => {
                    m.slots[idx].count = 1;
                    m.slots[idx].negated = false;
                },
                Kind::Count => m.slots[idx].count += 1,
                Kind::Opt => {
                    let value = cluster_value(cluster, off, ch, it, &a)?;
                    push_value(&a, &mut m.slots[idx], value);
                    return Ok(()); // option swallowed the cluster tail
                },
                Kind::Positional | Kind::Trailing => {
                    return Err(unknown(format!("-{ch}")));
                },
            }
        } else if let Some(g) = find_global_short(globals, ch) {
            match g.kind {
                Kind::Flag | Kind::Count => hits.push(GlobalHit {
                    arg: g,
                    value: None,
                    negated: false,
                }),
                Kind::Opt => {
                    let value = cluster_value(cluster, off, ch, it, g)?;
                    hits.push(GlobalHit {
                        arg: g,
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

#[allow(clippy::manual_contains)]
fn find_global_long(globals: &[&'static ArgSpec], name: &str) -> Option<&'static ArgSpec> {
    globals
        .iter()
        .copied()
        .find(|a| a.long == Some(name) || a.aliases.iter().any(|&al| al == name))
}

fn find_global_negate(globals: &[&'static ArgSpec], name: &str) -> Option<&'static ArgSpec> {
    globals.iter().copied().find(|a| a.negate == Some(name))
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

fn find_global_short(globals: &[&'static ArgSpec], ch: char) -> Option<&'static ArgSpec> {
    globals.iter().copied().find(|a| a.short == Some(ch))
}

fn record_global<'a>(
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
                arg: g,
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
                arg: g,
                value: Some(value),
                negated: false,
            });
        },
        Kind::Positional | Kind::Trailing => return Err(unknown(g.display_name())),
    }
    Ok(())
}

/// apply the hits this `spec` owns into its slots, leaving the rest to bubble up
fn apply_global_hits<'a>(spec: &CommandSpec, m: &mut Matches<'a>, hits: &mut Vec<GlobalHit<'a>>) {
    hits.retain(|h| {
        let Some(idx) = spec.args.iter().position(|a| core::ptr::eq(a, h.arg)) else {
            return true;
        };
        let a = spec.args[idx];
        match a.kind {
            Kind::Flag => {
                m.slots[idx].count = u32::from(!h.negated);
                m.slots[idx].negated = h.negated;
            },
            Kind::Count => m.slots[idx].count += 1,
            Kind::Opt => {
                if let Some(v) = h.value {
                    push_value(&a, &mut m.slots[idx], v);
                }
            },
            Kind::Positional | Kind::Trailing => {},
        }
        false
    });
}

/// assign a bare token to the next positional, or a trailing/variadic sink
fn positional<'a>(
    spec: &CommandSpec,
    m: &mut Matches<'a>,
    positionals: &[usize],
    cursor: &mut usize,
    tok: &'a str,
) -> Result<(), ErrorKind> {
    let idx = if *cursor < positionals.len() {
        positionals[*cursor]
    } else if let Some(&last) = positionals.last() {
        let a = spec.args[last];
        if a.multi || a.kind == Kind::Trailing {
            last // overflow lands in the variadic tail
        } else {
            return Err(ErrorKind::UnexpectedPositional(tok.to_owned()));
        }
    } else {
        return Err(ErrorKind::UnexpectedPositional(tok.to_owned()));
    };

    let a = spec.args[idx];
    m.slots[idx].values.push(tok);
    m.slots[idx].count += 1;
    // single positional advances the cursor, a variadic one keeps eating
    if !(a.multi || a.kind == Kind::Trailing) {
        *cursor += 1;
    }
    Ok(())
}

/// enforce `required` and group constraints. defaults are injected separately
/// by `apply_defaults`, so a defaulted arg never counts as missing here.
fn finalise(
    spec: &CommandSpec,
    m: &Matches,
    globals: &[&'static ArgSpec],
) -> Result<(), ErrorKind> {
    for (i, a) in spec.args.iter().enumerate() {
        // fallback is resolved later, so it counts as present
        let present = m.slots[i].count > 0 || !m.slots[i].values.is_empty();
        if !present && a.default.is_none() && a.env.is_none() && a.required {
            return Err(ErrorKind::MissingRequired(a.display_name()));
        }
    }

    for g in spec.groups {
        let set: Vec<String> = spec
            .args
            .iter()
            .enumerate()
            .filter(|(i, a)| a.group == Some(g.name) && m.slots[*i].count > 0)
            .map(|(_, a)| a.display_name())
            .collect();
        if set.len() > 1 {
            return Err(ErrorKind::Conflict {
                group: g.name.to_owned(),
                first: set[0].clone(),
                second: set[1].clone(),
            });
        }
        if set.is_empty() && g.required {
            let options = spec
                .args
                .iter()
                .filter(|a| a.group == Some(g.name))
                .map(ArgSpec::display_name)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ErrorKind::MissingGroup {
                group: g.name.to_owned(),
                options,
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

    if spec.has_subs() && m.sub.is_none() && !spec.sub_optional {
        // empty/sub-less invocation shows help rather than a bare error
        return Err(ErrorKind::Help(help::render(spec, globals)));
    }

    Ok(())
}

fn builtin_long(spec: &CommandSpec, name: &str, globals: &[&'static ArgSpec]) -> Option<ErrorKind> {
    match name {
        "help" if spec.find_long("help").is_none() => {
            Some(ErrorKind::Help(help::render(spec, globals)))
        },
        "version" if spec.has_version_info() && spec.find_long("version").is_none() => {
            Some(ErrorKind::Version(help::version_line(spec)))
        },
        _ => None,
    }
}

fn builtin_short(spec: &CommandSpec, ch: char, globals: &[&'static ArgSpec]) -> Option<ErrorKind> {
    match ch {
        'h' if spec.find_short('h').is_none() => Some(ErrorKind::Help(help::render(spec, globals))),
        'V' if spec.has_version_info() && spec.find_short('V').is_none() => {
            Some(ErrorKind::Version(help::version_line(spec)))
        },
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
    spec.args
        .iter()
        .chain(globals.iter().map(|a| &**a))
        .flat_map(|a| {
            a.long
                .into_iter()
                .chain(a.negate)
                .chain(a.aliases.iter().copied())
        })
        .chain(["help", "version"])
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
        name: "flat",
        version: "0.1.0",
        hash: None,
        about: "a flat command",
        args: FLAT_ARGS,
        groups: &[],
        conflicts: &[],
        subs: &[],
        sub_optional: false,
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
    fn errors() {
        assert!(matches!(parse(&FLAT, &["--nope"]), Err(ErrorKind::Unknown { .. })));
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
        assert!(matches!(parse(&FLAT, &["--help"]), Err(ErrorKind::Help(_))));
        assert!(matches!(parse(&FLAT, &["-h"]), Err(ErrorKind::Help(_))));
        match parse(&FLAT, &["--version"]) {
            Err(ErrorKind::Version(v)) => assert_eq!(v, "flat 0.1.0"),
            other => panic!("expected version, got {other:?}"),
        }

        const HASHED: CommandSpec = CommandSpec::new("flat").version("0.1.0").hash("abc123");
        match parse(&HASHED, &["--version"]) {
            Err(ErrorKind::Version(v)) => assert_eq!(v, "flat 0.1.0 (abc123)"),
            other => panic!("expected version, got {other:?}"),
        }

        const HASH_ONLY: CommandSpec = CommandSpec::new("flat").hash("abc123");
        match parse(&HASH_ONLY, &["-V"]) {
            Err(ErrorKind::Version(v)) => assert_eq!(v, "flat (abc123)"),
            other => panic!("expected version, got {other:?}"),
        }
    }

    #[test]
    fn defaults_resolve_in_readers() {
        const ARGS: &[ArgSpec] = &[ArgSpec::new(Kind::Opt).long("level").default("info")];
        const SPEC: CommandSpec = CommandSpec {
            name: "d",
            version: "",
            hash: None,
            about: "",
            args: ARGS,
            groups: &[],
            conflicts: &[],
            subs: &[],
            sub_optional: false,
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
            name: "g",
            version: "",
            hash: None,
            about: "",
            args: ARGS,
            groups: &[GroupSpec::new("mode")],
            conflicts: &[],
            subs: &[],
            sub_optional: false,
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
    fn conflict_pairs() {
        const ARGS: &[ArgSpec] = &[
            ArgSpec::new(Kind::Flag).long("a"),
            ArgSpec::new(Kind::Flag).long("b"),
        ];
        const SPEC: CommandSpec = CommandSpec {
            name: "c",
            version: "",
            hash: None,
            about: "",
            args: ARGS,
            groups: &[],
            conflicts: &[(0, 1)],
            subs: &[],
            sub_optional: false,
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
        name: "add",
        version: "",
        hash: None,
        about: "add a pin",
        args: ADD_ARGS,
        groups: &[],
        conflicts: &[],
        subs: &[],
        sub_optional: false,
    };
    const ROOT_SUBS: &[SubSpec] = &[SubSpec {
        name: "add",
        aliases: &[],
        about: "add a pin",
        spec: &ADD,
        hidden: false,
    }];
    const ROOT: CommandSpec = CommandSpec {
        name: "prog",
        version: "1.0.0",
        hash: None,
        about: "demo",
        args: &[],
        groups: &[],
        conflicts: &[],
        subs: ROOT_SUBS,
        sub_optional: false,
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
}
