// SPDX-License-Identifier: EUPL-1.2

//! parsing `#[pound(...)]` metas and doc comments off venial attributes.

use proc_macro2::{
    Delimiter,
    TokenTree,
};
use venial::{
    Attribute,
    AttributeValue,
};

/// the parsed `#[pound(...)]` options for one field or item.
// short/long are tristate: absent, bare, or with a value. the bool flags are an
// attribute bag, not a state machine, so the bool-count lint does not apply.
#[allow(clippy::option_option, clippy::struct_excessive_bools)]
#[derive(Default)]
pub struct Pound {
    /// `None` absent, `Some(None)` bare `short`, `Some(Some(c))` `short = 'c'`
    pub short:      Option<Option<char>>,
    /// `None` absent, `Some(None)` bare `long`, `Some(Some(s))` `long = "s"`
    pub long:       Option<Option<String>>,
    pub positional: bool,
    pub trailing:   bool,
    pub count:      bool,
    /// field delegates to its type's subcommand tree
    pub subcommand: bool,
    /// keep this arg/variant out of help output
    pub hidden:     bool,
    pub group:      Option<String>,
    pub default:    Option<String>,
    pub env:        Option<String>,
    pub value_name: Option<String>,
    pub help:       Option<String>,
    pub name:       Option<String>,
    pub version:    Option<String>,
    /// item-level: groups that must have exactly one member set
    pub required_groups: Vec<String>,
    /// field-level: names of fields this one cannot be combined with
    pub conflicts_with:  Vec<String>,
    /// extra long names (fields) or command names (variants) that also match
    pub aliases:         Vec<String>,
}

impl Pound {
    /// true if this field is a named option/flag rather than a positional.
    pub const fn is_named(&self) -> bool {
        self.short.is_some() || self.long.is_some()
    }
}

/// collect `#[pound(...)]` options from a set of attributes.
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

/// the joined, trimmed doc comment of an item or field, empty when none.
pub fn doc(attrs: &[Attribute]) -> String {
    let mut lines = Vec::new();
    for attr in attrs {
        if path_is(attr, "doc")
            && let AttributeValue::Equals(_, tokens) = &attr.value
            && let Some(text) = tokens.first().map(unquote)
        {
            lines.push(text.trim().to_owned());
        }
    }
    lines.join(" ").trim().to_owned()
}

fn path_is(attr: &Attribute, name: &str) -> bool {
    attr.path.len() == 1 && matches!(&attr.path[0], TokenTree::Ident(id) if *id == name)
}

/// split the comma-separated metas inside `pound(...)` and apply each.
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
            "group" => out.group = value,
            "default" => out.default = value,
            "env" => out.env = value,
            "value_name" => out.value_name = value,
            "help" => out.help = value,
            "name" => out.name = value,
            "version" => out.version = value,
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
            "alias" => {
                if let Some(v) = value {
                    out.aliases.extend(csv(&v));
                }
            },
            _ => {},
        }
    }
}

/// split a comma list value into trimmed, non-empty names.
fn csv(v: &str) -> impl Iterator<Item = String> + '_ {
    v.split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

/// split a flat token list on top-level commas.
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

/// strip the quotes off a string or char literal token, leaving its content.
fn unquote(tok: &TokenTree) -> String {
    let raw = match tok {
        TokenTree::Literal(l) => l.to_string(),
        TokenTree::Group(g) if g.delimiter() == Delimiter::None => {
            return g.stream().into_iter().next().as_ref().map_or_else(String::new, unquote);
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
