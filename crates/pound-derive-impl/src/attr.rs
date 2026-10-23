// SPDX-License-Identifier: EUPL-1.2

//! parsing `#[pound(...)]` metas and doc comments off venial attributes

use core::{
    mem::take,
    str::FromStr as _,
};

use proc_macro2::{
    Delimiter,
    Group,
    Ident,
    Span,
    TokenStream,
    TokenTree,
};
use quote::quote;
use venial::{
    Attribute,
    AttributeValue,
    Error,
    Fields,
    Item,
};

/// a `default` value, written as a literal, a path or a braced expression
#[derive(Clone)]
pub enum Text {
    Lit(String),
    Expr(Vec<TokenTree>),
    /// a value enum variant written as a path, `Format::Json`, read back
    /// through the label its derive gives it
    Value(Vec<TokenTree>),
}

impl Text {
    pub fn tokens(&self) -> TokenStream {
        match self {
            Self::Lit(text) => quote! { #text },
            Self::Expr(tokens) => tokens.iter().cloned().collect(),
            Self::Value(value_path) => {
                let value_tokens = value_path.iter().cloned().collect::<TokenStream>();
                quote! { #value_tokens.__pound_label() }
            },
        }
    }
}

/// how a `short`, `long` or `negate` key was written
#[derive(Clone)]
pub enum Spelling<T> {
    /// bare, so the spelling comes from the field name
    Inferred,
    Given(T),
}

impl<T: Clone> Spelling<T> {
    pub fn or_infer(&self, infer: impl FnOnce() -> T) -> T {
        match self {
            Self::Inferred => infer(),
            Self::Given(given) => given.clone(),
        }
    }
}

impl<T> From<Option<T>> for Spelling<T> {
    fn from(value: Option<T>) -> Self {
        value.map_or(Self::Inferred, Self::Given)
    }
}

/// the parsed `#[pound(...)]` options for one field or item
// short/long are tristate: absent, bare, or with a value
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bare key is its own switch"
)]
#[derive(Default)]
pub(crate) struct Pound {
    pub keys:            Vec<Ident>,
    /// `None` absent, `Inferred` bare `short`, `Given(c)` `short = 'c'`
    pub short:           Option<Spelling<char>>,
    /// `None` absent, `Inferred` bare `long`, `Given(s)` `long = "s"`
    pub long:            Option<Spelling<String>>,
    pub positional:      bool,
    pub trailing:        bool,
    pub last:            bool,
    pub count:           bool,
    /// field delegates to its type's subcommand tree
    pub subcommand:      bool,
    /// field embeds another `Parse` type's args at this command level
    pub flatten:         bool,
    /// variant-level: takes any subcommand name no other variant answers to
    pub external:        bool,
    /// field-level: given, the field must be the only arg of its command
    pub exclusive:       bool,
    /// item- or variant-level: given no words, the command prints its help
    pub help_if_empty:   bool,
    /// `ValueEnum`-level: its values match in any ASCII case
    pub ignore_case:     bool,
    /// keep this arg/variant out of help output
    pub hidden:          bool,
    /// the note a use warns with, empty for a bare `deprecated`
    pub deprecated:      Option<String>,
    pub hide_possible:   bool,
    pub hide_default:    bool,
    pub hide_env:        bool,
    /// named flag/option that descendant subcommands also accept
    pub global:          bool,
    pub hyphen_values:   bool,
    pub group:           Option<String>,
    /// `group` and `required_group` names written bare, linked so rename and
    /// go-to-definition reach every mention
    pub group_refs:      Vec<Ident>,
    pub default:         Option<Text>,
    /// value an option takes when written with no `=value`
    pub default_missing: Option<String>,
    pub env:             Option<String>,
    /// `None` absent, `Inferred` bare `negate`, which infers `no-<long>`
    pub negate:          Option<Spelling<String>>,
    pub value_name:      Option<String>,
    pub help:            Option<String>,
    /// help text `--help` shows in place of the short form
    pub long_help:       Option<String>,
    pub before_help:     Option<String>,
    pub after_help:      Option<String>,
    /// help section this arg is listed under
    pub heading:         Option<String>,
    pub display_order:   Option<usize>,
    pub name:            Option<String>,
    /// any expression, not just a literal
    pub version:         Option<Vec<TokenTree>>,
    /// any expression, not just a literal
    pub long_version:    Option<Vec<TokenTree>>,
    /// field-level: minimum accepted parsed value
    pub min:             Option<String>,
    /// field-level: maximum accepted parsed value
    pub max:             Option<String>,
    /// field-level: maximum accepted raw character count
    pub max_len:         Option<String>,
    /// field-level: fewest values a `Vec` field accepts
    pub min_values:      Option<String>,
    /// field-level: most values a `Vec` field accepts
    pub max_values:      Option<String>,
    /// field-level: splits each value of a `Vec` field into several
    pub delimiter:       Option<char>,
    pub num_args:        Option<NumArgs>,
    /// field-level: custom raw-value parser, a path or any callable expression
    pub parse:           Option<TokenStream>,
    /// field-level: custom parsed-value validation, a path or any callable
    /// expression
    pub validate:        Option<TokenStream>,
    /// field-level: shell completion candidates, a path or any callable
    /// expression coercing to `pound::complete::Completer`
    pub complete:        Option<TokenStream>,
    /// field-level: what the value names, overriding its type's hint
    pub hint:            Option<ValueHint>,
    /// item-level: groups that must have exactly one member set
    pub required_groups: Vec<String>,
    /// field-level: fields this one cannot be combined with
    pub conflicts_with:  Vec<Ident>,
    /// field-level: fields this one obliges when set
    pub requires:        Vec<Ident>,
    /// extra long names (fields) or command names (variants) that also match
    pub aliases:         Vec<String>,
    pub short_aliases:   Vec<char>,
}

impl Pound {
    /// true if this field is a named option/flag rather than a positional.
    pub const fn is_named(&self) -> bool {
        self.short.is_some() || self.long.is_some()
    }

    pub fn allow_only(&self, allowed: &[&str]) -> Result<(), Error> {
        if let Some(key) = self
            .keys
            .iter()
            .find(|key| !allowed.contains(&key.to_string().as_str()))
        {
            return Err(Error::new_at_span(
                key.span(),
                format!("pound: `{key}` is not valid here"),
            ));
        }

        Ok(())
    }

    /// apply one `key` or `key = value` meta
    #[rustfmt::skip]
    fn set(&mut self, ident: &Ident, value: Option<String>, seg: &[TokenTree]) -> Result<bool, Error> {
        let (name, span) = (ident.to_string(), ident.span());
        let key = name.as_str();

        match key {
            "short" => self.short = Some(Spelling::from(value.as_deref().map(|short| single_char(key, short, span)).transpose()?)),
            "negate" => {
                self.negate = Some(Spelling::from(value.map(|negate| negate.trim_start_matches('-').to_owned())));
            },
            "long"            => self.long            = Some(Spelling::from(value)),
            "version"         => self.version         = expr(seg),
            "long_version"    => self.long_version    = expr(seg),
            "positional"      => self.positional      = bare(key, span, value.is_some())?,
            "trailing"        => self.trailing        = bare(key, span, value.is_some())?,
            "last"            => self.last            = bare(key, span, value.is_some())?,
            "count"           => self.count           = bare(key, span, value.is_some())?,
            "subcommand"      => self.subcommand      = bare(key, span, value.is_some())?,
            "flatten"         => self.flatten         = bare(key, span, value.is_some())?,
            "external"        => self.external        = bare(key, span, value.is_some())?,
            "exclusive"       => self.exclusive       = bare(key, span, value.is_some())?,
            "help_if_empty"   => self.help_if_empty   = bare(key, span, value.is_some())?,
            "ignore_case"     => self.ignore_case     = bare(key, span, value.is_some())?,
            "hidden"          => self.hidden          = bare(key, span, value.is_some())?,
            "hide_possible"   => self.hide_possible   = bare(key, span, value.is_some())?,
            "hide_default"    => self.hide_default    = bare(key, span, value.is_some())?,
            "hide_env"        => self.hide_env        = bare(key, span, value.is_some())?,
            "global"          => self.global          = bare(key, span, value.is_some())?,
            "allow_hyphen_values" => self.hyphen_values = bare(key, span, value.is_some())?,
            "deprecated"      => self.deprecated      = Some(value.unwrap_or_default()),
            "group"           => {
                self.group = Some(needed(key, span, value)?);
                self.group_refs.extend(bare_ident(seg));
            },
            "default"         => self.default         = Some(text(key, span, seg)?),
            "default_missing" => self.default_missing = Some(needed(key, span, value)?),
            "env"             => self.env             = Some(needed(key, span, value)?),
            "value_name"      => self.value_name      = Some(needed(key, span, value)?),
            "help"            => self.help            = Some(needed(key, span, value)?),
            "long_help"       => self.long_help       = Some(needed(key, span, value)?),
            "before_help"     => self.before_help     = Some(needed(key, span, value)?),
            "after_help"      => self.after_help      = Some(needed(key, span, value)?),
            "heading"         => self.heading         = Some(needed(key, span, value)?),
            "display_order"   => self.display_order   = Some(number(key, span, value)?),
            "name"            => self.name            = Some(needed(key, span, value)?),
            "min"             => self.min             = Some(needed(key, span, value)?),
            "max"             => self.max             = Some(needed(key, span, value)?),
            "max_len"         => self.max_len         = Some(needed(key, span, value)?),
            "min_values"      => self.min_values      = Some(needed(key, span, value)?),
            "max_values"      => self.max_values      = Some(needed(key, span, value)?),
            "delimiter"       => self.delimiter       = Some(single_char(key, &needed(key, span, value)?, span)?),
            "num_args"        => self.num_args        = Some(NumArgs::new(&needed(key, span, value)?, span)?),
            "parse"           => self.parse           = Some(callable(key, span, seg)?),
            "validate"        => self.validate        = Some(callable(key, span, seg)?),
            "complete"        => self.complete        = Some(callable(key, span, seg)?),
            "hint"            => self.hint            = Some(ValueHint::new(&needed(key, span, value)?, seg.get(2).map_or(span, TokenTree::span))?),
            "required_group"  => {
                self.required_groups.push(needed(key, span, value)?);
                self.group_refs.extend(bare_ident(seg));
            },
            "conflicts_with"  => self.conflicts_with.extend(field_refs(key, span, seg)?),
            "requires"        => self.requires.extend(field_refs(key, span, seg)?),
            "alias"           => self.aliases.extend(names(key, span, seg)?),
            "short_alias"     => {
                for short in names(key, span, seg)? {
                    self.short_aliases.push(single_char(key, &short, span)?);
                }
            },
            _                 => return Ok(false),
        }

        Ok(true)
    }
}

/// collect `#[pound(...)]` options from a set of attributes
pub fn pound(attrs: &[Attribute]) -> Result<Pound, Error> {
    let mut out = Pound::default();

    for attr in attrs {
        if !path_is(attr, "pound") {
            continue;
        }

        let AttributeValue::Group(_, tokens) = &attr.value else {
            return Err(Error::new_at_span(
                attr.path[0].span(),
                "pound: attributes must use #[pound(...)]",
            ));
        };

        apply_metas(&mut out, tokens)?;
    }

    Ok(out)
}

/// every key written in a `#[pound(...)]` on the item, its variants and its
/// fields, malformed or not
pub fn written_keys(item: &Item) -> Vec<Ident> {
    attribute_lists(item)
        .into_iter()
        .flatten()
        .filter(|attr| path_is(attr, "pound"))
        .filter_map(|attr| {
            match &attr.value {
                AttributeValue::Group(_, tokens) => Some(split_commas(tokens)),
                AttributeValue::Equals(..) | AttributeValue::Empty => None,
            }
        })
        .flatten()
        .filter_map(|seg| {
            match seg.first() {
                Some(TokenTree::Ident(key)) => Some(key.clone()),
                _ => None,
            }
        })
        .collect()
}

/// whether the item, a variant or a field carries a native `#[deprecated]`,
/// which the generated impls touch
pub fn mentions_deprecated(item: &Item) -> bool {
    attribute_lists(item)
        .into_iter()
        .flatten()
        .any(|attr| path_is(attr, "deprecated"))
}

/// the note of a native `#[deprecated]`, `#[deprecated = "..."]` or
/// `#[deprecated(note = "...")]`, empty when it gives none
pub fn native_deprecated(attrs: &[Attribute]) -> Option<String> {
    let attr = attrs.iter().find(|attr| path_is(attr, "deprecated"))?;

    let note = match &attr.value {
        AttributeValue::Empty => None,
        AttributeValue::Equals(_, tokens) => tokens.first().map(unquote),
        AttributeValue::Group(_, tokens) => {
            split_commas(tokens).into_iter().find_map(|seg| {
                match seg.as_slice() {
                    [TokenTree::Ident(key), TokenTree::Punct(punct), value]
                        if key == "note" && punct.as_char() == '=' =>
                    {
                        Some(unquote(value))
                    },
                    _ => None,
                }
            })
        },
    };

    Some(note.unwrap_or_default())
}

/// the attributes of the item, its variants and its fields
fn attribute_lists(item: &Item) -> Vec<&Vec<Attribute>> {
    fn field_attrs(fields: &Fields) -> Vec<&Vec<Attribute>> {
        match fields {
            Fields::Named(named) => {
                named
                    .fields
                    .items()
                    .map(|field| &field.attributes)
                    .collect()
            },
            Fields::Tuple(tuple) => {
                tuple
                    .fields
                    .items()
                    .map(|field| &field.attributes)
                    .collect()
            },
            Fields::Unit => Vec::new(),
        }
    }

    let (own, variants, fields) = match item {
        Item::Struct(shape) => (&shape.attributes, None, Some(&shape.fields)),
        Item::Enum(shape) => (&shape.attributes, Some(&shape.variants), None),
        Item::Union(_)
        | Item::Module(_)
        | Item::Trait(_)
        | Item::Impl(_)
        | Item::TypeAlias(_)
        | Item::Function(_)
        | Item::Constant(_)
        | Item::UseDeclaration(_)
        | Item::Macro(_)
        | Item::ExternBlock(_)
        | Item::ExternCrate(_)
        | _ => return Vec::new(),
    };

    let mut lists = vec![own];
    lists.extend(fields.map(field_attrs).unwrap_or_default());

    for variant in variants
        .iter()
        .flat_map(|variant_list| variant_list.items())
    {
        lists.push(&variant.attributes);
        lists.extend(field_attrs(&variant.fields));
    }

    lists
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

/// the summary as short help shows it, without the period that ends the
/// sentence. an abbreviation like `e.g.` or `etc.` keeps its period
pub fn short_help(doc: &str) -> &str {
    let summary = summary(doc);

    let Some(rest) = summary.strip_suffix('.') else {
        return summary;
    };

    let last_word = rest.rsplit(char::is_whitespace).next().unwrap_or(rest);

    if last_word.contains('.') || last_word == "etc" {
        summary
    } else {
        rest
    }
}

fn path_is(attr: &Attribute, name: &str) -> bool {
    attr.path.len() == 1 && matches!(&attr.path[0], TokenTree::Ident(id) if *id == name)
}

/// split the comma-separated metas inside `pound(...)` and apply each
fn apply_metas(out: &mut Pound, tokens: &[TokenTree]) -> Result<(), Error> {
    for seg in split_commas(tokens) {
        let Some(TokenTree::Ident(ident)) = seg.first() else {
            return Err(Error::new_at_span(
                seg[0].span(),
                "pound: expected `key` or `key = value`",
            ));
        };

        let key = ident.to_string();

        let value = match seg.as_slice() {
            [_] => None,
            [_, TokenTree::Punct(punct), value] if punct.as_char() == '=' => Some(unquote(value)),
            _ if EXPR_KEYS.contains(&key.as_str()) && expr(&seg).is_some() => None,
            [_, TokenTree::Punct(punct), rest @ ..]
                if punct.as_char() == '=' && SPLICED_KEYS.contains(&key.as_str()) =>
            {
                Some(rest.iter().map(unquote).collect())
            },
            _ => {
                return Err(Error::new_at_span(
                    ident.span(),
                    format!("pound: malformed `{key}` attribute"),
                ));
            },
        };

        // an unknown key is left to rustc, which rejects it at the emitted
        // `pound::attr` path with the closest match
        if out.set(ident, value, &seg)? {
            out.keys.push(ident.clone());
        }
    }

    Ok(())
}

/// keys whose value may span tokens, `min = -1` or `num_args = 1..=3`, read as
/// the text they spell
const SPLICED_KEYS: &[&str] = &["min", "max", "num_args"];

/// keys whose value is read as tokens rather than as a single literal
const EXPR_KEYS: &[&str] = &[
    "version",
    "long_version",
    "default",
    "parse",
    "validate",
    "complete",
];

/// a `default` is a literal, a bare word, or a braced expression of any
/// `Display` type. a lone expression drops its braces for `unused_braces`
fn text(key: &str, span: Span, seg: &[TokenTree]) -> Result<Text, Error> {
    let tokens = expr(seg)
        .ok_or_else(|| Error::new_at_span(span, format!("pound: `{key}` needs a value")))?;

    let is_path = tokens.len() > 1
        && tokens.iter().all(|tok| {
            matches!(tok, TokenTree::Ident(_))
                || matches!(tok, TokenTree::Punct(punct) if punct.as_char() == ':')
        });

    if is_path {
        return Ok(Text::Value(tokens));
    }

    let [raw_tok] = tokens.as_slice() else {
        return Err(Error::new_at_span(span, braces_hint(key)));
    };

    let tok = match raw_tok {
        TokenTree::Group(group) if group.delimiter() == Delimiter::None => {
            let mut inner = group.stream().into_iter();
            match (inner.next(), inner.next()) {
                (Some(only), None) => only,
                _ => return Err(Error::new_at_span(span, braces_hint(key))),
            }
        },
        TokenTree::Group(_) | TokenTree::Ident(_) | TokenTree::Punct(_) | TokenTree::Literal(_) => {
            raw_tok.clone()
        },
    };

    match &tok {
        TokenTree::Ident(word) if is_constant_name(&word.to_string()) => {
            Err(Error::new_at_span(
                span,
                format!(
                    "pound: `{key} = {word}` would be the text \"{word}\", write `{key} = {{ \
                     {word} }}` for the constant or quote it for the text"
                ),
            ))
        },
        TokenTree::Literal(_) | TokenTree::Ident(_) => Ok(Text::Lit(unquote(&tok))),
        TokenTree::Group(group) if group.delimiter() == Delimiter::Brace => {
            let inner = group.stream().into_iter().collect::<Vec<_>>();
            let has_statements = inner
                .iter()
                .any(|tree| matches!(tree, TokenTree::Punct(punct) if punct.as_char() == ';'));
            Ok(Text::Expr(if has_statements { vec![tok] } else { inner }))
        },
        TokenTree::Group(_) | TokenTree::Punct(_) => {
            Err(Error::new_at_span(span, braces_hint(key)))
        },
    }
}

fn is_constant_name(word: &str) -> bool {
    word.chars().any(|ch| ch.is_ascii_uppercase())
        && word
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

fn braces_hint(key: &str) -> String {
    format!(
        "pound: `{key}` takes a literal, a bare word, a value enum variant like `Format::Json` or \
         a braced expression like `{{ CONST }}`"
    )
}

/// a string literal names a path, anything else is already the callable
fn callable(key: &str, span: Span, seg: &[TokenTree]) -> Result<TokenStream, Error> {
    let tokens = expr(seg)
        .ok_or_else(|| Error::new_at_span(span, format!("pound: `{key}` needs a value")))?;

    let stream = match tokens.as_slice() {
        [tok] if is_quoted(tok) => {
            TokenStream::from_str(&unquote(tok)).map_err(|parse_error| {
                Error::new_at_span(
                    span,
                    format!("pound: `{key}` is not a valid path: {parse_error}"),
                )
            })?
        },
        _ => tokens.into_iter().collect(),
    };

    if stream.is_empty() {
        return Err(Error::new_at_span(
            span,
            format!("pound: `{key}` needs a value"),
        ));
    }

    Ok(stream)
}

/// a string literal, possibly forwarded through a `macro_rules!` `$x:expr` as
/// a transparent group
fn is_quoted(tok: &TokenTree) -> bool {
    match tok {
        TokenTree::Literal(lit) => lit.to_string().starts_with('"'),
        TokenTree::Group(group) if group.delimiter() == Delimiter::None => {
            let mut inner = group.stream().into_iter();
            matches!(
                (inner.next(), inner.next()),
                (Some(quoted_tok), None) if is_quoted(&quoted_tok)
            )
        },
        TokenTree::Group(_) | TokenTree::Ident(_) | TokenTree::Punct(_) => false,
    }
}

fn bare(key: &str, span: Span, has_value: bool) -> Result<bool, Error> {
    if has_value {
        return Err(Error::new_at_span(
            span,
            format!("pound: `{key}` does not take a value"),
        ));
    }

    Ok(true)
}

/// a `hint`, what a value names for shell completion
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueHint {
    Path,
    /// a `pound::Hint` variant named as written, which rustc and
    /// rust-analyzer resolve
    Variant(Ident),
}

impl ValueHint {
    fn new(word: &str, span: Span) -> Result<Self, Error> {
        match word {
            "path" => Ok(Self::Path),
            _ if word.starts_with(|ch: char| ch.is_ascii_uppercase()) => {
                Ok(Self::Variant(Ident::new(word, span)))
            },
            _ => {
                Err(Error::new_at_span(
                    span,
                    "pound: `hint` takes `path` or a `pound::Hint` variant",
                ))
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NumArgs {
    pub min: usize,
    pub max: Option<usize>,
}

impl NumArgs {
    fn new(text: &str, span: Span) -> Result<Self, Error> {
        let invalid = || {
            Error::new_at_span(
                span,
                format!("pound: `num_args = {text}` is not `N`, `N..` or `N..=M` with N above 0"),
            )
        };
        let count = |word: &str| {
            word.trim().parse::<usize>().map_err(|parse_error| {
                Error::new_at_span(
                    span,
                    format!("pound: `num_args = {word}` is not a count: {parse_error}"),
                )
            })
        };

        let (min, max) = match text.split_once("..") {
            None => (count(text)?, Some(count(text)?)),
            Some((low, "")) => (count(low)?, None),
            Some((low, high)) => {
                let inclusive = high.strip_prefix('=').ok_or_else(invalid)?;
                (count(low)?, Some(count(inclusive)?))
            },
        };

        if min == 0 || max.is_some_and(|limit| limit < min) {
            return Err(invalid());
        }
        Ok(Self { min, max })
    }
}

fn number(key: &str, span: Span, value: Option<String>) -> Result<usize, Error> {
    let text = needed(key, span, value)?;
    text.parse().map_err(|parse_error| {
        Error::new_at_span(
            span,
            format!("pound: `{key} = {text}` is not a number: {parse_error}"),
        )
    })
}

fn needed(key: &str, span: Span, value: Option<String>) -> Result<String, Error> {
    value.ok_or_else(|| Error::new_at_span(span, format!("pound: `{key}` needs a value")))
}

fn single_char(key: &str, value: &str, span: Span) -> Result<char, Error> {
    let mut chars = value.chars();

    match (chars.next(), chars.next()) {
        (Some(ch), None) => Ok(ch),
        _ => {
            Err(Error::new_at_span(
                span,
                format!("pound: `{key}` needs one character"),
            ))
        },
    }
}

/// everything after `key =`, left as tokens. [`None`] for a bare `key`, which
/// asks for the inferred value.
fn expr(seg: &[TokenTree]) -> Option<Vec<TokenTree>> {
    match seg.get(1) {
        Some(TokenTree::Punct(punct)) if punct.as_char() == '=' => {
            let rest = &seg[2..];
            (!rest.is_empty()).then(|| rest.to_vec())
        },
        _ => None,
    }
}

/// the fields a `requires` or `conflicts_with` names, written bare as `a`, as
/// a list `[a, b]`, or quoted as `"a, b"`
fn field_refs(key: &str, span: Span, seg: &[TokenTree]) -> Result<Vec<Ident>, Error> {
    let Some(value) = expr(seg) else {
        return Err(Error::new_at_span(
            span,
            format!("pound: `{key}` needs a value"),
        ));
    };

    match value.as_slice() {
        [TokenTree::Group(group)] if group.delimiter() == Delimiter::Bracket => {
            list_items(group)
                .iter()
                .map(|item| field_ref(key, item))
                .collect::<Result<Vec<_>, _>>()
                .map(|nested| nested.into_iter().flatten().collect())
        },
        _ => field_ref(key, &value),
    }
}

fn field_ref(key: &str, tokens: &[TokenTree]) -> Result<Vec<Ident>, Error> {
    match tokens {
        [TokenTree::Ident(ident)] => Ok(vec![ident.clone()]),
        [TokenTree::Group(group)] if group.delimiter() == Delimiter::None => {
            field_ref(key, &group.stream().into_iter().collect::<Vec<_>>())
        },
        [lit @ TokenTree::Literal(_)] if is_quoted(lit) => {
            csv(&unquote(lit))
                .map(|name| {
                    let raw = name.strip_prefix("r#");
                    let plain = raw.unwrap_or(&name);
                    let mut chars = plain.chars();
                    let valid = chars
                        .next()
                        .is_some_and(|ch| ch == '_' || ch.is_alphabetic())
                        && chars.all(|ch| ch == '_' || ch.is_alphanumeric());

                    if !valid {
                        Err(Error::new_at_span(
                            lit.span(),
                            format!("pound: {key}: `{name}` is not a field name"),
                        ))
                    } else if raw.is_some() {
                        Ok(Ident::new_raw(plain, lit.span()))
                    } else {
                        Ok(Ident::new(plain, lit.span()))
                    }
                })
                .collect()
        },
        _ => {
            Err(Error::new_at_span(
                tokens.first().map_or_else(Span::call_site, TokenTree::span),
                format!("pound: `{key}` takes a field name or a list of them"),
            ))
        },
    }
}

fn bare_ident(seg: &[TokenTree]) -> Option<Ident> {
    match seg {
        [_, TokenTree::Punct(punct), TokenTree::Ident(ident)] if punct.as_char() == '=' => {
            Some(ident.clone())
        },
        _ => None,
    }
}

/// one quoted name, or several as `["a", "b"]`
fn names(key: &str, span: Span, seg: &[TokenTree]) -> Result<Vec<String>, Error> {
    let Some(value) = expr(seg) else {
        return Err(Error::new_at_span(
            span,
            format!("pound: `{key}` needs a value"),
        ));
    };

    let items = match value.as_slice() {
        [TokenTree::Group(group)] if group.delimiter() == Delimiter::Bracket => list_items(group),
        _ => vec![value],
    };

    items
        .iter()
        .map(|item| {
            match item.as_slice() {
                [lit] if !unquote(lit).contains(',') => Ok(unquote(lit)),
                _ => {
                    Err(Error::new_at_span(
                        item.first().map_or(span, TokenTree::span),
                        format!("pound: `{key}` takes one name, or a list as `[\"a\", \"b\"]`"),
                    ))
                },
            }
        })
        .collect()
}

fn csv(list: &str) -> impl Iterator<Item = String> + '_ {
    list.split(',')
        .map(|entry| entry.trim().to_owned())
        .filter(|entry| !entry.is_empty())
}

fn list_items(group: &Group) -> Vec<Vec<TokenTree>> {
    group
        .stream()
        .into_iter()
        .collect::<Vec<_>>()
        .split(|tok| matches!(tok, TokenTree::Punct(punct) if punct.as_char() == ','))
        .filter(|item| !item.is_empty())
        .map(<[TokenTree]>::to_vec)
        .collect()
}

fn split_commas(tokens: &[TokenTree]) -> Vec<Vec<TokenTree>> {
    let mut segs = Vec::new();
    let mut cur = Vec::new();

    for (i, tok) in tokens.iter().enumerate() {
        if matches!(tok, TokenTree::Punct(punct) if punct.as_char() == ',')
            && starts_meta(&tokens[i + 1..])
        {
            if !cur.is_empty() {
                segs.push(take(&mut cur));
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

// an expression value can hold top-level commas, in a turbofish or a
// closure's parameters, so a comma separates two metas only when a
// `key = ..`, a known bare key or the end follows it
fn starts_meta(rest: &[TokenTree]) -> bool {
    let [TokenTree::Ident(key_ident), after @ ..] = rest else {
        return rest.is_empty();
    };

    let key = key_ident.to_string();

    if !key.starts_with(|ch: char| ch.is_ascii_lowercase()) {
        return false;
    }

    match after.first() {
        None => true,
        Some(TokenTree::Punct(punct)) if punct.as_char() == '=' => {
            !matches!(after.get(1), Some(TokenTree::Punct(next)) if next.as_char() == '=')
        },
        Some(TokenTree::Punct(punct)) if punct.as_char() == ',' => {
            BARE_KEYS.contains(&key.as_str())
        },
        Some(_) => false,
    }
}

const BARE_KEYS: &[&str] = &[
    "short",
    "long",
    "negate",
    "positional",
    "trailing",
    "last",
    "count",
    "subcommand",
    "flatten",
    "external",
    "ignore_case",
    "exclusive",
    "help_if_empty",
    "hidden",
    "deprecated",
    "hide_possible",
    "hide_default",
    "hide_env",
    "global",
    "allow_hyphen_values",
];

fn unquote(tok: &TokenTree) -> String {
    let raw = match tok {
        TokenTree::Literal(lit) => lit.to_string(),
        TokenTree::Group(group) if group.delimiter() == Delimiter::None => {
            return group
                .stream()
                .into_iter()
                .next()
                .as_ref()
                .map_or_else(String::new, unquote);
        },
        TokenTree::Group(_) | TokenTree::Ident(_) | TokenTree::Punct(_) => return tok.to_string(),
    };

    if let Some(hashed) = raw.strip_prefix('r') {
        let fence = &hashed[..hashed.len() - hashed.trim_start_matches('#').len()];
        let inner = hashed
            .strip_prefix(fence)
            .and_then(|rest| rest.strip_suffix(fence))
            .and_then(|rest| rest.strip_prefix('"'))
            .and_then(|rest| rest.strip_suffix('"'));
        if let Some(body) = inner {
            return body.to_owned();
        }
    }

    let bytes = raw.as_bytes();

    if bytes.len() >= 2
        && (bytes[0] == b'"' || bytes[0] == b'\'')
        && bytes[bytes.len() - 1] == bytes[0]
    {
        unescape(&raw[1..raw.len() - 1])
    } else {
        raw
    }
}

/// the text a string or char literal's body stands for. the compiler has
/// already rejected any escape that is not valid.
fn unescape(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }

        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('0') => out.push('\0'),
            Some('x') => {
                let code = chars.by_ref().take(2).collect::<String>();
                out.extend(u8::from_str_radix(&code, 16).ok().map(char::from));
            },
            Some('u') => {
                let code = chars
                    .by_ref()
                    .skip(1)
                    .take_while(|&digit| digit != '}')
                    .filter(|&digit| digit != '_')
                    .collect::<String>();
                out.extend(u32::from_str_radix(&code, 16).ok().and_then(char::from_u32));
            },
            Some('\n') => {
                chars = chars
                    .as_str()
                    .trim_start_matches([' ', '\t', '\n', '\r'])
                    .chars();
            },
            Some(other) => out.push(other),
            None => {},
        }
    }

    out
}
