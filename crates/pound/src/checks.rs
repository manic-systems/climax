// SPDX-License-Identifier: EUPL-1.2

//! const checks the derive runs on every spec it builds, so a spec that could
//! never parse fails to compile

use crate::{
    Subcommands,
    spec::{
        ArgumentOrder,
        CommandSpec,
        Kind,
        SubSpec,
    },
    value::const_eq,
};

/// whether a declared default survives its own type's value list.
///
/// the `Parse` derive calls this from a `const` block, so a `default` that no
/// `ValueEnum` variant answers to fails the build instead of the first run that
/// leaves the flag out.
#[must_use]
pub const fn default_allowed(default: &str, possible: Option<&[&str]>) -> bool {
    let Some(values) = possible else {
        return true;
    };
    let mut i = 0;
    while i < values.len() {
        if const_eq(values[i], default) {
            return true;
        }
        i += 1;
    }
    false
}

/// `T`'s spec, reachable only when `T` is a command enum.
///
/// the derive reads subcommand and spliced specs through this, so a struct in
/// either place fails to compile instead of offering no commands.
#[must_use]
pub const fn subcommand_spec<T: Subcommands>() -> &'static CommandSpec {
    T::SPEC
}

/// whether every spelling in `spec`, flattened structs included, reaches a
/// single arg. the derive asserts this, since the parser would otherwise give
/// a clashing spelling to whichever arg it meets first.
#[must_use]
pub const fn names_unique(spec: &CommandSpec) -> bool {
    names_unique_within(spec, spec)
}

const fn names_unique_within(root: &CommandSpec, spec: &CommandSpec) -> bool {
    let mut i = 0;
    while i < spec.args.len() {
        let arg = &spec.args[i];
        if let Some(short) = arg.short
            && claimants(root, Spelling::Short(short)) > 1
        {
            return false;
        }
        if let Some(long) = arg.long
            && claimants(root, Spelling::Long(long)) > 1
        {
            return false;
        }
        if let Some(negate) = arg.negate
            && claimants(root, Spelling::Long(negate)) > 1
        {
            return false;
        }
        let mut alias = 0;
        while alias < arg.aliases.len() {
            if claimants(root, Spelling::Long(arg.aliases[alias])) > 1 {
                return false;
            }
            alias += 1;
        }
        i += 1;
    }
    let mut inner = 0;
    while inner < spec.flattened.len() {
        if !names_unique_within(root, spec.flattened[inner]) {
            return false;
        }
        inner += 1;
    }
    true
}

/// how many places in `spec` and its flattened structs declare subcommands.
/// the derive asserts at most one, since only one of them could be selected.
#[must_use]
pub const fn selector_count(spec: &CommandSpec) -> usize {
    let mut count = if spec.subs.is_empty() { 0 } else { 1 };
    let mut i = 0;
    while i < spec.flattened.len() {
        count += selector_count(spec.flattened[i]);
        i += 1;
    }
    count
}

/// whether every name and alias among `subs`, flattened enums spliced in,
/// selects a single command. the derive asserts this for every command enum.
#[must_use]
pub const fn commands_unique(subs: &[SubSpec]) -> bool {
    commands_unique_within(subs, subs)
}

const fn commands_unique_within(root: &[SubSpec], subs: &[SubSpec]) -> bool {
    let mut i = 0;
    while i < subs.len() {
        let sub = &subs[i];
        if sub.flattened {
            if !commands_unique_within(root, sub.spec.subs) {
                return false;
            }
        } else {
            if command_claimants(root, sub.name) > 1 {
                return false;
            }
            let mut alias = 0;
            while alias < sub.aliases.len() {
                if command_claimants(root, sub.aliases[alias]) > 1 {
                    return false;
                }
                alias += 1;
            }
        }
        i += 1;
    }
    true
}

const fn command_claimants(subs: &[SubSpec], name: &str) -> usize {
    let mut count = 0;
    let mut i = 0;
    while i < subs.len() {
        let sub = &subs[i];
        if sub.flattened {
            count += command_claimants(sub.spec.subs, name);
        } else {
            if const_eq(sub.name, name) {
                count += 1;
            }
            let mut alias = 0;
            while alias < sub.aliases.len() {
                if const_eq(sub.aliases[alias], name) {
                    count += 1;
                }
                alias += 1;
            }
        }
        i += 1;
    }
    count
}

/// whether no positional or subcommand is stuck behind a variadic or trailing
/// positional. the derive asserts this.
#[must_use]
pub const fn positionals_reachable(spec: &CommandSpec) -> bool {
    let mut scan = PositionalScan {
        greedy:            false,
        previous_required: None,
        dispatchable:      true,
    };
    scan_positionals(spec, &mut scan) && (scan.dispatchable || !spec.has_subs())
}

struct PositionalScan {
    greedy:            bool,
    previous_required: Option<bool>,
    dispatchable:      bool,
}

/// walk positionals in declaration order, false once one follows a greedy one
const fn scan_positionals(spec: &CommandSpec, scan: &mut PositionalScan) -> bool {
    let mut position = 0;
    while position < spec.order_len() {
        match spec.order_entry(position) {
            ArgumentOrder::Direct(i) => {
                let arg = &spec.args[i];
                if arg.is_positional() {
                    if scan.greedy {
                        return false;
                    }
                    if arg.multi || matches!(arg.kind, Kind::Trailing) {
                        scan.greedy = true;
                        scan.dispatchable =
                            !arg.required && matches!(scan.previous_required, Some(false));
                    }
                    scan.previous_required = Some(arg.required);
                }
            },
            ArgumentOrder::Flattened(i) => {
                if !scan_positionals(spec.flattened[i], scan) {
                    return false;
                }
            },
        }
        position += 1;
    }
    true
}

/// whether any arg in `spec`, flattened structs included, belongs to `group`.
/// the derive asserts this for a required group whose members it cannot see.
#[must_use]
pub const fn group_has_members(spec: &CommandSpec, group: &str) -> bool {
    let mut i = 0;
    while i < spec.args.len() {
        if let Some(own) = spec.args[i].group
            && const_eq(own, group)
        {
            return true;
        }
        i += 1;
    }
    let mut inner = 0;
    while inner < spec.flattened.len() {
        if group_has_members(spec.flattened[inner], group) {
            return true;
        }
        inner += 1;
    }
    false
}

#[derive(Clone, Copy)]
enum Spelling<'a> {
    Short(char),
    Long(&'a str),
}

const fn claimants(spec: &CommandSpec, spelling: Spelling<'_>) -> usize {
    let mut count = 0;
    let mut i = 0;
    while i < spec.args.len() {
        let answers = match spelling {
            Spelling::Short(short) => spec.args[i].answers_short(short),
            Spelling::Long(long) => spec.args[i].answers_long(long),
        };
        if answers {
            count += 1;
        }
        i += 1;
    }
    let mut inner = 0;
    while inner < spec.flattened.len() {
        count += claimants(spec.flattened[inner], spelling);
        inner += 1;
    }
    count
}
