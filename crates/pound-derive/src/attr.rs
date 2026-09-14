// SPDX-License-Identifier: EUPL-1.2

//! parsing `#[pound(...)]` metas and doc comments off venial attributes

use proc_macro2::{Delimiter, TokenTree};
use venial::{Attribute, AttributeValue};

/// the parsed `#[pound(...)]` options for one field or item
// short/long are tristate: absent, bare, or with a value
#[allow(clippy::option_option, clippy::struct_excessive_bools)]
#[derive(Default)]
pub struct Pound {
    /// `None` absent, `Some(None)` bare `short`, `Some(Some(c))` `short = 'c'`
    pub short: Option<Option<char>>,
    /// `None` absent, `Some(None)` bare `long`, `Some(Some(s))` `long = "s"`
    pub long: Option<Option<String>>,
    pub positional: bool,
    pub trailing: bool,
    pub count: bool,
    /// field delegates to its type's subcommand tree
    pub subcommand: bool,
    /// keep this arg/variant out of help output
    pub hidden: bool,
    /// named flag/option that descendant subcommands also accept
    pub global: bool,
    pub group: Option<String>,
    pub default: Option<String>,
    /// value an option takes when written with no `=value`
    pub default_missing: Option<String>,
    pub env: Option<String>,
    /// `None` absent, `Some(None)` bare `negate`, which infers `no-<long>`
    pub negate: Option<Option<String>>,
    pub value_name: Option<String>,
    pub help: Option<String>,
    /// help text `--help` shows in place of the short form
    pub long_help: Option<String>,
    /// help section this arg is listed under
    pub heading: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    /// field-level: minimum accepted parsed value
    pub min: Option<String>,
    /// field-level: maximum accepted parsed value
    pub max: Option<String>,
    /// field-level: maximum accepted raw character count
    pub max_len: Option<String>,
    /// field-level: fewest values a `Vec` field accepts
    pub min_values: Option<String>,
    /// field-level: most values a `Vec` field accepts
    pub max_values: Option<String>,
    /// field-level: custom raw-value parser function
    pub parse: Option<String>,
    /// field-level: custom parsed-value validation function
    pub validate: Option<String>,
    /// item-level: groups that must have exactly one member set
    pub required_groups: Vec<String>,
    /// field-level: names of fields this one cannot be combined with
    pub conflicts_with: Vec<String>,
    /// field-level: names of fields this one obliges when set
    pub requires: Vec<String>,
    /// extra long names (fields) or command names (variants) that also match
    pub aliases: Vec<String>,
}

impl Pound {
    /// true if this field is a named option/flag rather than a positional.
    pub const fn is_named(&self) -> bool {
        self.short.is_some() || self.long.is_some()
    }
}

/// collect `#[pound(...)]` options from a set of attributes
pub fn pound(attrs: &[Attribute]) -> Pound {
    let mut out = Pound::default();
    for attr in attrs {
        if path_is(attr, "pound")
            && let AttributeValue::Group(_, tokens) = &attr.value
        {
            apply_metas(&mut out, tokens);
        }
    }
    out
}

/// the doc comment of an item or field, empty when none. lines are joined into
/// paragraphs, and a blank line stays a paragraph break so `--help` can show
/// more than `-h` does.
pub fn doc(attrs: &[Attribute]) -> String {
    let mut out = String::new();
    let mut fresh = true;
    for attr in attrs {
        if path_is(attr, "doc")
            && let AttributeValue::Equals(_, tokens) = &attr.value
            && let Some(text) = tokens.first().map(unquote)
        {
            let line = text.trim();
            if line.is_empty() {
                if !out.is_empty() {
                    out.push_str("\n\n");
                }
                fresh = true;
            } else {
                if !fresh {
                    out.push(' ');
                }
                out.push_str(line);
                fresh = false;
            }
        }
    }
    out.trim().to_owned()
}

/// the opening paragraph, which is all `-h` shows
pub fn summary(doc: &str) -> &str {
    doc.split("\n\n").next().unwrap_or(doc)
}

fn path_is(attr: &Attribute, name: &str) -> bool {
    attr.path.len() == 1 && matches!(&attr.path[0], TokenTree::Ident(id) if *id == name)
}

/// split the comma-separated metas inside `pound(...)` and apply each
fn apply_metas(out: &mut Pound, tokens: &[TokenTree]) {
    for seg in split_commas(tokens) {
        let Some(TokenTree::Ident(key)) = seg.first() else {
            continue;
        };
        // `key = value`?
        let value = match seg.get(1) {
            Some(TokenTree::Punct(p)) if p.as_char() == '=' => seg.get(2).map(unquote),
            _ => None,
        };
        match key.to_string().as_str() {
            "short" => out.short = Some(value.and_then(|v| v.chars().next())),
            "long" => out.long = Some(value),
            "positional" => out.positional = true,
            "trailing" => out.trailing = true,
            "count" => out.count = true,
            "subcommand" => out.subcommand = true,
            "hidden" => out.hidden = true,
            "global" => out.global = true,
            "group" => out.group = value,
            "default" => out.default = value,
            "default_missing" => out.default_missing = value,
            "env" => out.env = value,
            "negate" => {
                out.negate = Some(value.map(|v| v.trim_start_matches('-').to_owned()));
            },
            "value_name" => out.value_name = value,
            "help" => out.help = value,
            "long_help" => out.long_help = value,
            "heading" => out.heading = value,
            "name" => out.name = value,
            "version" => out.version = value,
            "min" => out.min = value,
            "max" => out.max = value,
            "max_len" => out.max_len = value,
            "min_values" => out.min_values = value,
            "max_values" => out.max_values = value,
            "parse" => out.parse = value,
            "validate" => out.validate = value,
            "required_group" => {
                if let Some(v) = value {
                    out.required_groups.push(v);
                }
            },
            "conflicts_with" => {
                if let Some(v) = value {
                    out.conflicts_with.extend(csv(&v));
                }
            },
            "requires" => {
                if let Some(v) = value {
                    out.requires.extend(csv(&v));
                }
            },
            "alias" => {
                if let Some(v) = value {
                    out.aliases.extend(csv(&v));
                }
            },
            _ => {},
        }
    }
}

fn csv(v: &str) -> impl Iterator<Item = String> + '_ {
    v.split(',')
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

fn split_commas(tokens: &[TokenTree]) -> Vec<Vec<TokenTree>> {
    let mut segs = Vec::new();
    let mut cur = Vec::new();
    for tok in tokens {
        if matches!(tok, TokenTree::Punct(p) if p.as_char() == ',') {
            if !cur.is_empty() {
                segs.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(tok.clone());
        }
    }
    if !cur.is_empty() {
        segs.push(cur);
    }
    segs
}

fn unquote(tok: &TokenTree) -> String {
    let raw = match tok {
        TokenTree::Literal(l) => l.to_string(),
        TokenTree::Group(g) if g.delimiter() == Delimiter::None => {
            return g
                .stream()
                .into_iter()
                .next()
                .as_ref()
                .map_or_else(String::new, unquote);
        },
        other => return other.to_string(),
    };
    let bytes = raw.as_bytes();
    if bytes.len() >= 2
        && (bytes[0] == b'"' || bytes[0] == b'\'')
        && bytes[bytes.len() - 1] == bytes[0]
    {
        raw[1..raw.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\'", "'")
            .replace("\\\\", "\\")
    } else {
        raw
    }
}
