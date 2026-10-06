// SPDX-License-Identifier: EUPL-1.2

//! parsing `#[pound(...)]` metas and doc comments off venial attributes

use proc_macro2::{Delimiter, TokenTree};
use venial::{Attribute, AttributeValue};

/// the parsed `#[pound(...)]` options for one field or item
// short/long are tristate: absent, bare, or with a value
#[allow(clippy::option_option, clippy::struct_excessive_bools)]
#[derive(Default)]
pub struct Pound {
    keys: Vec<String>,
    /// `None` absent, `Some(None)` bare `short`, `Some(Some(c))` `short = 'c'`
    pub short: Option<Option<char>>,
    /// `None` absent, `Some(None)` bare `long`, `Some(Some(s))` `long = "s"`
    pub long: Option<Option<String>>,
    pub positional: bool,
    pub trailing: bool,
    pub count: bool,
    /// field delegates to its type's subcommand tree
    pub subcommand: bool,
    /// field embeds another `Parse` type's args at this command level
    pub flatten: bool,
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
    /// any expression, not just a literal
    pub version: Option<Vec<TokenTree>>,
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

    pub fn allow_only(&self, allowed: &[&str]) -> Result<(), String> {
        if let Some(key) = self
            .keys
            .iter()
            .find(|key| !allowed.contains(&key.as_str()))
        {
            return Err(format!("pound: `{key}` is not valid here"));
        }
        Ok(())
    }

    /// apply one `key` or `key = value` meta
    #[rustfmt::skip]
    fn set(&mut self, key: &str, value: Option<String>, seg: &[TokenTree]) -> Result<(), String> {
        match key {
            "short" => self.short = Some(value.as_deref().map(single_char).transpose()?),
            "negate" => {
                self.negate = Some(value.map(|v| v.trim_start_matches('-').to_owned()));
            },
            "long"            => self.long            = Some(value),
            "version"         => self.version         = expr(seg),
            "positional"      => self.positional      = bare(key, value.is_some())?,
            "trailing"        => self.trailing        = bare(key, value.is_some())?,
            "count"           => self.count           = bare(key, value.is_some())?,
            "subcommand"      => self.subcommand      = bare(key, value.is_some())?,
            "flatten"         => self.flatten         = bare(key, value.is_some())?,
            "hidden"          => self.hidden          = bare(key, value.is_some())?,
            "global"          => self.global          = bare(key, value.is_some())?,
            "group"           => self.group           = Some(needed(key, value)?),
            "default"         => self.default         = Some(needed(key, value)?),
            "default_missing" => self.default_missing = Some(needed(key, value)?),
            "env"             => self.env             = Some(needed(key, value)?),
            "value_name"      => self.value_name      = Some(needed(key, value)?),
            "help"            => self.help            = Some(needed(key, value)?),
            "long_help"       => self.long_help       = Some(needed(key, value)?),
            "heading"         => self.heading         = Some(needed(key, value)?),
            "name"            => self.name            = Some(needed(key, value)?),
            "min"             => self.min             = Some(needed(key, value)?),
            "max"             => self.max             = Some(needed(key, value)?),
            "max_len"         => self.max_len         = Some(needed(key, value)?),
            "min_values"      => self.min_values      = Some(needed(key, value)?),
            "max_values"      => self.max_values      = Some(needed(key, value)?),
            "parse"           => self.parse           = Some(needed(key, value)?),
            "validate"        => self.validate        = Some(needed(key, value)?),
            "required_group"  => self.required_groups.push(needed(key, value)?),
            "conflicts_with"  => self.conflicts_with.extend(csv(&needed(key, value)?)),
            "requires"        => self.requires.extend(csv(&needed(key, value)?)),
            "alias"           => self.aliases.extend(csv(&needed(key, value)?)),
            _                 => return Err(format!("pound: unknown attribute `{key}`")),
        }
        Ok(())
    }
}

/// collect `#[pound(...)]` options from a set of attributes
pub fn pound(attrs: &[Attribute]) -> Result<Pound, String> {
    let mut out = Pound::default();
    for attr in attrs {
        if !path_is(attr, "pound") {
            continue;
        }
        let AttributeValue::Group(_, tokens) = &attr.value else {
            return Err("pound: attributes must use #[pound(...)]".to_owned());
        };
        apply_metas(&mut out, tokens)?;
    }
    Ok(out)
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
fn apply_metas(out: &mut Pound, tokens: &[TokenTree]) -> Result<(), String> {
    for seg in split_commas(tokens) {
        let Some(TokenTree::Ident(ident)) = seg.first() else {
            return Err("pound: expected `key` or `key = value`".to_owned());
        };
        let key = ident.to_string();
        let value = match seg.as_slice() {
            [_] => None,
            [_, TokenTree::Punct(p), value] if p.as_char() == '=' => Some(unquote(value)),
            _ if key == "version" && expr(&seg).is_some() => None,
            _ => return Err(format!("pound: malformed `{key}` attribute")),
        };
        out.set(&key, value, &seg)?;
        out.keys.push(key);
    }
    Ok(())
}

fn bare(key: &str, has_value: bool) -> Result<bool, String> {
    if has_value {
        return Err(format!("pound: `{key}` does not take a value"));
    }
    Ok(true)
}

fn needed(key: &str, value: Option<String>) -> Result<String, String> {
    value.ok_or_else(|| format!("pound: `{key}` needs a value"))
}

fn single_char(value: &str) -> Result<char, String> {
    let mut chars = value.chars();
    match (chars.next(), chars.next()) {
        (Some(short), None) => Ok(short),
        _ => Err("pound: `short` needs one character".to_owned()),
    }
}

/// everything after `key =`, left as tokens. [`None`] for a bare `key`, which
/// asks for the inferred value.
fn expr(seg: &[TokenTree]) -> Option<Vec<TokenTree>> {
    match seg.get(1) {
        Some(TokenTree::Punct(p)) if p.as_char() == '=' => {
            let rest = &seg[2..];
            (!rest.is_empty()).then(|| rest.to_vec())
        },
        _ => None,
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
