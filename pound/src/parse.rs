// SPDX-License-Identifier: EUPL-1.2

//! the one parser. walks `argv` against a [`CommandSpec`] and produces
//! [`Matches`], a positional record of what was seen. the only generics are the
//! three typed readers at the bottom that forward to [`FromArg`], the rest is
//! monomorphisation-free so it compiles once however many commands you define.

use std::vec::IntoIter;

use crate::{
    error::Error,
    help,
    spec::{
        ArgSpec,
        CommandSpec,
        Kind,
    },
    value::FromArg,
};

/// what a single arg collected during a parse.
#[derive(Default, Clone, Debug)]
struct Slot {
    /// times the user supplied it (flags 0/1, counts n). defaults do not bump
    /// this, so group/required logic can tell user input apart.
    count:  u32,
    /// raw values, in order
    values: Vec<String>,
}

/// a successful parse: each spec entry's state, plus an optional chosen
/// subcommand and its nested matches.
#[derive(Debug)]
pub struct Matches {
    slots: Vec<Slot>,
    sub:   Option<(usize, Box<Self>)>,
}

impl Matches {
    fn new(len: usize) -> Self {
        Self {
            slots: vec![Slot::default(); len],
            sub:   None,
        }
    }

    /// was a flag (or any value) supplied at least once.
    #[must_use]
    pub fn flag(&self, i: usize) -> bool {
        self.slots[i].count > 0
    }

    /// how many times a count flag was supplied.
    #[must_use]
    pub fn count(&self, i: usize) -> u32 {
        self.slots[i].count
    }

    /// first raw value (or injected default), if any.
    #[must_use]
    pub fn raw(&self, i: usize) -> Option<&str> {
        self.slots[i].values.first().map(String::as_str)
    }

    /// all raw values, in order.
    #[must_use]
    pub fn raws(&self, i: usize) -> &[String] {
        &self.slots[i].values
    }

    /// chosen subcommand index and its matches, if one ran.
    #[must_use]
    pub fn sub(&self) -> Option<(usize, &Self)> {
        self.sub.as_ref().map(|(i, m)| (*i, m.as_ref()))
    }

    /// read a required value as `T`, for bare `T` fields.
    pub fn required<T: FromArg>(&self, spec: &CommandSpec, i: usize) -> Result<T, Error> {
        match self.raw(i) {
            Some(s) => parse_into(spec, i, s),
            None => Err(Error::MissingRequired(spec.args[i].display_name())),
        }
    }

    /// read an optional value as `T`, for `Option<T>` fields.
    pub fn optional<T: FromArg>(&self, spec: &CommandSpec, i: usize) -> Result<Option<T>, Error> {
        match self.raw(i) {
            Some(s) => Ok(Some(parse_into(spec, i, s)?)),
            None => Ok(None),
        }
    }

    /// read every value as `T`, for `Vec<T>` fields.
    pub fn many<T: FromArg>(&self, spec: &CommandSpec, i: usize) -> Result<Vec<T>, Error> {
        self.raws(i)
            .iter()
            .map(|s| parse_into(spec, i, s))
            .collect()
    }
}

fn parse_into<T: FromArg>(spec: &CommandSpec, i: usize, s: &str) -> Result<T, Error> {
    T::from_arg(s).map_err(|e| {
        let mut msg = e.msg;
        if let Some(values) = spec.args[i].possible
            && !values.is_empty()
        {
            msg = format!("{msg} (possible values: {})", values.join(", "));
        }
        Error::Value {
            arg:   spec.args[i].display_name(),
            value: e.value,
            msg,
        }
    })
}

/// entry point: parse `args` (already minus `argv[0]`) against `spec`.
pub(crate) fn parse_spec(
    spec: &CommandSpec,
    args: impl IntoIterator<Item = String>,
) -> Result<Matches, Error> {
    let mut it = args.into_iter().collect::<Vec<_>>().into_iter();
    parse_cmd(spec, &mut it)
}

fn parse_cmd(spec: &CommandSpec, it: &mut IntoIter<String>) -> Result<Matches, Error> {
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
                Some((n, v)) => (n, Some(v.to_owned())),
                None => (long, None),
            };
            if let Some(sig) = builtin_long(spec, name) {
                return Err(sig);
            }
            let idx = spec
                .find_long(name)
                .ok_or_else(|| Error::Unknown(format!("--{name}")))?;
            apply_named(spec, &mut m, idx, inline, it)?;
        } else if let Some(rest) = tok.strip_prefix('-').filter(|r| !r.is_empty()) {
            let first = rest.chars().next().unwrap_or('-');
            let known = spec.find_short(first).is_some() || builtin_short(spec, first).is_some();
            if known {
                shorts(spec, &mut m, rest, it)?;
            } else {
                // not an option (negative numbers, lone values) -> positional
                positional(spec, &mut m, &positionals, &mut pos_cursor, tok)?;
            }
        } else if spec.has_subs() && positionals.is_empty() {
            let sidx = spec
                .find_sub(&tok)
                .ok_or_else(|| Error::UnknownSubcommand(tok.clone()))?;
            let sub_m = parse_cmd(spec.subs[sidx].spec, it)?;
            m.sub = Some((sidx, Box::new(sub_m)));
            break; // subcommand owns the rest
        } else {
            positional(spec, &mut m, &positionals, &mut pos_cursor, tok)?;
        }
    }

    finalize(spec, &m)?;
    Ok(m)
}

/// apply a long option once resolved to a spec index.
fn apply_named(
    spec: &CommandSpec,
    m: &mut Matches,
    idx: usize,
    inline: Option<String>,
    it: &mut IntoIter<String>,
) -> Result<(), Error> {
    let a = spec.args[idx];
    match a.kind {
        Kind::Flag => {
            if inline.is_some() {
                return Err(Error::UnexpectedValue(a.display_name()));
            }
            m.slots[idx].count = 1;
        },
        Kind::Count => {
            if inline.is_some() {
                return Err(Error::UnexpectedValue(a.display_name()));
            }
            m.slots[idx].count += 1;
        },
        Kind::Opt => {
            let value = match inline {
                Some(v) => v,
                None => {
                    it.next()
                        .ok_or_else(|| Error::MissingValue(a.display_name()))?
                },
            };
            push_value(&a, &mut m.slots[idx], value);
        },
        Kind::Positional | Kind::Trailing => return Err(Error::Unknown(a.display_name())),
    }
    Ok(())
}

/// apply a short cluster, e.g. `-vvf` or `-ofile`.
fn shorts(
    spec: &CommandSpec,
    m: &mut Matches,
    cluster: &str,
    it: &mut IntoIter<String>,
) -> Result<(), Error> {
    let chars: Vec<char> = cluster.chars().collect();
    let mut ci = 0;
    while ci < chars.len() {
        let ch = chars[ci];
        if let Some(sig) = builtin_short(spec, ch) {
            return Err(sig);
        }
        let idx = spec
            .find_short(ch)
            .ok_or_else(|| Error::Unknown(format!("-{ch}")))?;
        let a = spec.args[idx];
        match a.kind {
            Kind::Flag => m.slots[idx].count = 1,
            Kind::Count => m.slots[idx].count += 1,
            Kind::Opt => {
                let rest: String = chars[ci + 1..].iter().collect();
                let value = if rest.is_empty() {
                    it.next()
                        .ok_or_else(|| Error::MissingValue(a.display_name()))?
                } else {
                    rest
                };
                push_value(&a, &mut m.slots[idx], value);
                return Ok(()); // option swallowed the cluster tail
            },
            Kind::Positional | Kind::Trailing => {
                return Err(Error::Unknown(format!("-{ch}")));
            },
        }
        ci += 1;
    }
    Ok(())
}

fn push_value(a: &ArgSpec, slot: &mut Slot, value: String) {
    if !a.multi {
        slot.values.clear(); // last wins for single-valued opts
    }
    slot.values.push(value);
    slot.count += 1;
}

/// assign a bare token to the next positional, or a trailing/variadic sink.
fn positional(
    spec: &CommandSpec,
    m: &mut Matches,
    positionals: &[usize],
    cursor: &mut usize,
    tok: String,
) -> Result<(), Error> {
    let idx = if *cursor < positionals.len() {
        positionals[*cursor]
    } else if let Some(&last) = positionals.last() {
        let a = spec.args[last];
        if a.multi || a.kind == Kind::Trailing {
            last // overflow lands in the variadic tail
        } else {
            return Err(Error::UnexpectedPositional(tok));
        }
    } else {
        return Err(Error::UnexpectedPositional(tok));
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
fn finalize(spec: &CommandSpec, m: &Matches) -> Result<(), Error> {
    for (i, a) in spec.args.iter().enumerate() {
        let present = m.slots[i].count > 0 || !m.slots[i].values.is_empty();
        if !present && a.default.is_none() && a.required {
            return Err(Error::MissingRequired(a.display_name()));
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
            return Err(Error::Conflict {
                group:  g.name.to_owned(),
                first:  set[0].clone(),
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
            return Err(Error::MissingGroup {
                group: g.name.to_owned(),
                options,
            });
        }
    }

    for &(a, b) in spec.conflicts {
        if m.slots[a].count > 0 && m.slots[b].count > 0 {
            return Err(Error::Conflict {
                group:  String::new(),
                first:  spec.args[a].display_name(),
                second: spec.args[b].display_name(),
            });
        }
    }

    if spec.has_subs() && m.sub.is_none() && !spec.sub_optional {
        // empty/sub-less invocation shows help rather than a bare error
        return Err(Error::Help(help::render(spec)));
    }

    Ok(())
}

fn builtin_long(spec: &CommandSpec, name: &str) -> Option<Error> {
    match name {
        "help" if spec.find_long("help").is_none() => Some(Error::Help(help::render(spec))),
        "version" if spec.find_long("version").is_none() => {
            Some(Error::Version(help::version_line(spec)))
        },
        _ => None,
    }
}

fn builtin_short(spec: &CommandSpec, ch: char) -> Option<Error> {
    match ch {
        'h' if spec.find_short('h').is_none() => Some(Error::Help(help::render(spec))),
        'V' if spec.find_short('V').is_none() => Some(Error::Version(help::version_line(spec))),
        _ => None,
    }
}

/// inject spec defaults into the matches so the typed readers see them. run by
/// [`crate::Parse`] before `from_matches`.
pub(crate) fn apply_defaults(spec: &CommandSpec, m: &mut Matches) {
    for (i, a) in spec.args.iter().enumerate() {
        if m.slots[i].values.is_empty()
            && m.slots[i].count == 0
            && let Some(def) = a.default
        {
            m.slots[i].values.push(def.to_owned());
        }
    }
    if let Some((sidx, sub)) = &mut m.sub {
        apply_defaults(spec.subs[*sidx].spec, sub);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{
        GroupSpec,
        SubSpec,
    };

    fn argv(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_owned()).collect()
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
        name:    "flat",
        version: "0.1.0",
        about:   "a flat command",
        args:    FLAT_ARGS,
        groups:  &[],
        conflicts: &[],
        subs:    &[],
sub_optional: false,
    };

    fn parse(spec: &CommandSpec, a: &[&str]) -> Result<Matches, Error> {
        parse_spec(spec, argv(a))
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
    fn errors() {
        assert!(matches!(parse(&FLAT, &["--nope"]), Err(Error::Unknown(_))));
        assert!(matches!(
            parse(&FLAT, &["name", "--dir"]),
            Err(Error::MissingValue(_))
        ));
        assert!(matches!(parse(&FLAT, &[]), Err(Error::MissingRequired(_))));
    }

    #[test]
    fn help_and_version_signals() {
        assert!(matches!(parse(&FLAT, &["--help"]), Err(Error::Help(_))));
        assert!(matches!(parse(&FLAT, &["-h"]), Err(Error::Help(_))));
        match parse(&FLAT, &["--version"]) {
            Err(Error::Version(v)) => assert_eq!(v, "flat 0.1.0"),
            other => panic!("expected version, got {other:?}"),
        }
    }

    #[test]
    fn defaults_are_injected() {
        const ARGS: &[ArgSpec] = &[ArgSpec::new(Kind::Opt).long("level").default("info")];
        const SPEC: CommandSpec = CommandSpec {
            name:    "d",
            version: "",
            about:   "",
            args:    ARGS,
            groups:  &[],
            conflicts: &[],
            subs:    &[],
sub_optional: false,
        };
        let mut m = parse(&SPEC, &[]).unwrap();
        apply_defaults(&SPEC, &mut m);
        assert_eq!(m.raw(0), Some("info"));
        // a user value overrides the default
        let mut m = parse(&SPEC, &["--level", "debug"]).unwrap();
        apply_defaults(&SPEC, &mut m);
        assert_eq!(m.raw(0), Some("debug"));
    }

    #[test]
    fn groups_conflict_and_require() {
        const ARGS: &[ArgSpec] = &[
            ArgSpec::new(Kind::Flag).long("flake").group("mode"),
            ArgSpec::new(Kind::Flag).long("fetch").group("mode"),
        ];
        const OPT: CommandSpec = CommandSpec {
            name:    "g",
            version: "",
            about:   "",
            args:    ARGS,
            groups:  &[GroupSpec::new("mode")],
            conflicts: &[],
            subs:    &[],
sub_optional: false,
        };
        const REQ: CommandSpec = CommandSpec {
            groups: &[GroupSpec::new("mode").required()],
            ..OPT
        };
        assert!(matches!(
            parse(&OPT, &["--flake", "--fetch"]),
            Err(Error::Conflict { .. })
        ));
        assert!(parse(&OPT, &["--flake"]).is_ok());
        assert!(parse(&OPT, &[]).is_ok()); // not required, zero is fine
        assert!(matches!(parse(&REQ, &[]), Err(Error::MissingGroup { .. })));
    }

    #[test]
    fn conflict_pairs() {
        const ARGS: &[ArgSpec] =
            &[ArgSpec::new(Kind::Flag).long("a"), ArgSpec::new(Kind::Flag).long("b")];
        const SPEC: CommandSpec = CommandSpec {
            name:      "c",
            version:   "",
            about:     "",
            args:      ARGS,
            groups:    &[],
            conflicts: &[(0, 1)],
            subs:      &[],
sub_optional: false,
        };
        assert!(parse(&SPEC, &["--a"]).is_ok());
        assert!(matches!(parse(&SPEC, &["--a", "--b"]), Err(Error::Conflict { .. })));
    }

    // a subcommand tree: `prog add <name> <url> [--force]`
    const ADD_ARGS: &[ArgSpec] = &[
        ArgSpec::new(Kind::Positional).value_name("name").required(),
        ArgSpec::new(Kind::Positional).value_name("url").required(),
        ArgSpec::new(Kind::Flag).long("force").short('f'),
    ];
    const ADD: CommandSpec = CommandSpec {
        name:    "add",
        version: "",
        about:   "add a pin",
        args:    ADD_ARGS,
        groups:  &[],
        conflicts: &[],
        subs:    &[],
sub_optional: false,
    };
    const ROOT_SUBS: &[SubSpec] = &[SubSpec {
        name:   "add",
        about:  "add a pin",
        spec:   &ADD,
        hidden: false,
    }];
    const ROOT: CommandSpec = CommandSpec {
        name:    "prog",
        version: "1.0.0",
        about:   "demo",
        args:    &[],
        groups:  &[],
        conflicts: &[],
        subs:    ROOT_SUBS,
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
            Err(Error::UnknownSubcommand(_))
        ));
        // bare invocation shows help
        assert!(matches!(parse(&ROOT, &[]), Err(Error::Help(_))));
    }
}
