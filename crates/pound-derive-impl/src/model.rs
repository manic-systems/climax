// SPDX-License-Identifier: EUPL-1.2

//! what a derive input means, checked against pound's rules before any tokens
//! are written

use std::{
    collections::HashMap,
    str::FromStr,
};

use proc_macro2::{
    Ident,
    TokenStream,
    TokenTree,
};
use quote::quote;
use venial::{
    Attribute,
    EnumVariant,
    Error,
    Fields,
    Item,
    NamedField,
    TypeExpr,
};

use crate::attr::{
    Pound,
    Text,
    ValueHint,
    doc,
    pound,
    short_help,
    summary,
};

const ITEM_ATTRIBUTES: &[&str] = &["name", "version", "required_group"];
const VARIANT_ATTRIBUTES: &[&str] = &["name", "alias", "hidden", "required_group"];
const VALUE_VARIANT_ATTRIBUTES: &[&str] = &["name"];
const FIELD_ATTRIBUTES: &[&str] = &[
    "short",
    "long",
    "positional",
    "trailing",
    "count",
    "hidden",
    "global",
    "exclusive",
    "group",
    "default",
    "default_missing",
    "env",
    "negate",
    "value_name",
    "help",
    "long_help",
    "heading",
    "min",
    "max",
    "max_len",
    "min_values",
    "max_values",
    "parse",
    "validate",
    "complete",
    "hint",
    "conflicts_with",
    "requires",
    "alias",
];

/// a `#[derive(Parse)]` input
pub enum Derive {
    Struct {
        ident:   Ident,
        header:  Header,
        command: Box<Command>,
    },
    Enum {
        ident:    Ident,
        header:   Header,
        variants: Vec<Variant>,
    },
}

impl Derive {
    pub fn new(item: &Item) -> Result<Self, Error> {
        match item {
            Item::Struct(s) => {
                let fields = CommandFields::new(&s.fields, &s.name)?;
                let header = Header::new(&s.attributes)?;
                let command =
                    Command::new(&s.name, s.name.to_string(), fields, &header.required_groups)?;

                Ok(Self::Struct {
                    ident: s.name.clone(),
                    header,
                    command: Box::new(command),
                })
            },
            Item::Enum(e) => {
                if e.variants.is_empty() {
                    return Err(Error::new_at_span(
                        e.name.span(),
                        "pound: a command enum needs at least one variant",
                    ));
                }
                let header = Header::new(&e.attributes)?;
                let variants = e
                    .variants
                    .items()
                    .map(|variant| Variant::new(&e.name, variant))
                    .collect::<Result<_, _>>()?;
                Ok(Self::Enum {
                    ident: e.name.clone(),
                    header,
                    variants,
                })
            },
            _ => {
                Err(Error::new(
                    "pound: Parse can only derive on a struct or enum",
                ))
            },
        }
    }
}

/// what the top of a derived command says about itself
pub struct Header {
    pub name:            Option<String>,
    pub version:         Option<Vec<TokenTree>>,
    pub doc:             String,
    /// groups that need exactly one member set, which only a struct's command
    /// reads
    pub required_groups: Vec<String>,
}

impl Header {
    fn new(attributes: &[Attribute]) -> Result<Self, Error> {
        let item = pound(attributes)?;
        item.allow_only(ITEM_ATTRIBUTES)?;

        Ok(Self {
            name:            item.name,
            version:         item.version,
            doc:             doc(attributes),
            required_groups: item.required_groups,
        })
    }
}

pub struct Variant {
    pub ident: Ident,
    pub kind:  VariantKind,
}

pub enum VariantKind {
    /// `#[pound(flatten)] V(Other)` offers `Other`'s commands at this level
    Flatten(TokenStream),
    /// `#[pound(external)] V(Vec<T>)` or `V(pound::External)` takes any name
    /// no other variant answers to, with the words after it
    External(ExternalField),
    Command {
        name:    String,
        aliases: Vec<String>,
        hidden:  bool,
        doc:     String,
        command: Box<Command>,
    },
}

/// what an external variant holds the line in
pub enum ExternalField {
    /// `Vec<T>`, the name then every word after it
    List,
    /// `pound::External`, which can also say what the name was close to
    Payload,
}

impl Variant {
    fn new(owner: &Ident, variant: &EnumVariant) -> Result<Self, Error> {
        let vattr = pound(&variant.attributes)?;
        let ident = variant.name.clone();

        if vattr.flatten {
            vattr.allow_only(&["flatten"])?;
            let ty = Self::single_field(variant, "flatten")?;

            return Ok(Self {
                ident,
                kind: VariantKind::Flatten(ty.tokens.iter().cloned().collect()),
            });
        }

        if vattr.external {
            vattr.allow_only(&["external"])?;
            let field = Self::external_field(variant)?;

            return Ok(Self {
                ident,
                kind: VariantKind::External(field),
            });
        }

        let fields = CommandFields::new(&variant.fields, &ident)?;
        vattr.allow_only(VARIANT_ATTRIBUTES)?;

        let command = Command::new(
            &ident,
            format!("{owner}::{ident}"),
            fields,
            &vattr.required_groups,
        )?;

        Ok(Self {
            kind: VariantKind::Command {
                name:    vattr
                    .name
                    .unwrap_or_else(|| camel_to_kebab(&ident.to_string())),
                aliases: vattr.aliases,
                hidden:  vattr.hidden,
                doc:     doc(&variant.attributes),
                command: Box::new(command),
            },
            ident,
        })
    }

    // the one tuple field of a `#[pound(flatten)]` or `#[pound(external)]`
    // enum variant
    fn single_field<'v>(variant: &'v EnumVariant, attr: &str) -> Result<&'v TypeExpr, Error> {
        let message = format!("pound: #[pound({attr})] needs exactly one unattributed tuple field");

        let Fields::Tuple(tuple) = &variant.fields else {
            return Err(Error::new_at_span(variant.name.span(), message));
        };

        let mut fields = tuple.fields.items();

        match (fields.next(), fields.next()) {
            (Some(field), None) if field.attributes.is_empty() => Ok(&field.ty),
            _ => Err(Error::new_at_span(variant.name.span(), message)),
        }
    }

    fn external_field(variant: &EnumVariant) -> Result<ExternalField, Error> {
        let ty = Self::single_field(variant, "external")?;
        if FieldType::new(ty).shape == Shape::Many {
            return Ok(ExternalField::List);
        }

        let path = ty.tokens.iter().all(|tok| {
            match tok {
                TokenTree::Ident(_) => true,
                TokenTree::Punct(p) => p.as_char() == ':',
                _ => false,
            }
        });
        if path && matches!(ty.tokens.last(), Some(TokenTree::Ident(id)) if id == "External") {
            return Ok(ExternalField::Payload);
        }

        Err(Error::new_at_span(
            ty.tokens
                .first()
                .map_or_else(|| variant.name.span(), TokenTree::span),
            "pound: #[pound(external)] needs a `Vec<T>` or `pound::External` field",
        ))
    }
}

/// one command level, a struct or a variant with fields, after its args,
/// groups and cross-references were checked
pub struct Command {
    /// `Name` or `Name::Variant`, for the messages of the const asserts
    pub owner:     String,
    pub fields:    CommandFields,
    pub groups:    Vec<Group>,
    /// pairs of arg indices that cannot be set together, smaller index first
    pub conflicts: Vec<(usize, usize)>,
    /// pairs where setting the first arg obliges the second
    pub requires:  Vec<(usize, usize)>,
}

pub struct Group {
    pub name:     String,
    pub required: bool,
    /// whether an arg declared on this command joins it, as opposed to only
    /// args of flattened structs, which only the spec can tell
    pub direct:   bool,
}

impl Command {
    fn new(
        name: &Ident,
        owner: String,
        fields: CommandFields,
        required_groups: &[String],
    ) -> Result<Self, Error> {
        let args = &fields.args;

        if let Some(arg) = args.iter().find(|arg| arg.global && !arg.kind.is_named()) {
            return Err(Error::new_at_span(
                arg.ident.span(),
                "pound: #[pound(global)] needs a flag or option (short/long), not a positional",
            ));
        }

        if let Some(arg) = args.iter().find(|arg| arg.exclusive)
            && (args.iter().any(|other| other.required)
                || fields.sub.as_ref().is_some_and(|sub| !sub.optional))
        {
            return Err(Error::new_at_span(
                arg.ident.span(),
                "pound: #[pound(exclusive)] needs every other field optional, or reading them \
                 fails when it is given alone",
            ));
        }

        let mut groups = Vec::<Group>::new();
        let named = args.iter().filter_map(|arg| arg.group.as_ref());

        for name in named.chain(required_groups) {
            if !groups.iter().any(|group| group.name == *name) {
                groups.push(Group {
                    name:     name.clone(),
                    required: required_groups.contains(name),
                    direct:   args.iter().any(|arg| arg.group.as_ref() == Some(name)),
                });
            }
        }

        if fields.flattened.is_empty()
            && let Some(group) = groups.iter().find(|group| group.required && !group.direct)
        {
            return Err(Error::new_at_span(
                name.span(),
                format!("pound: required group `{}` has no members", group.name),
            ));
        }

        let index = args
            .iter()
            .enumerate()
            .map(|(i, arg)| (arg.ident.to_string(), i))
            .collect::<HashMap<_, _>>();

        let lookup = |key: &str, field: &str, by: &Arg| {
            index.get(field).copied().ok_or_else(|| {
                Error::new_at_span(
                    by.ident.span(),
                    format!("pound: {key}: no field named `{field}`"),
                )
            })
        };

        let mut conflicts = Vec::new();

        for (i, arg) in args.iter().enumerate() {
            for field in &arg.conflicts_with {
                let j = lookup("conflicts_with", field, arg)?;
                let pair = (i.min(j), i.max(j));

                if i != j && !conflicts.contains(&pair) {
                    conflicts.push(pair);
                }
            }
        }

        // direction carries meaning here, so unlike conflicts these are kept
        // as written
        let mut requires = Vec::new();

        for (i, arg) in args.iter().enumerate() {
            for field in &arg.requires {
                let j = lookup("requires", field, arg)?;

                if i != j && !requires.contains(&(i, j)) {
                    requires.push((i, j));
                }
            }
        }

        Ok(Self {
            owner,
            fields,
            groups,
            conflicts,
            requires,
        })
    }
}

/// a struct or variant's fields, split into args, flattened structs and the
/// optional subcommand field
pub struct CommandFields {
    pub args:      Vec<Arg>,
    pub flattened: Vec<Flattened>,
    /// declaration order across `args` and `flattened`
    pub order:     Vec<Slot>,
    pub sub:       Option<Sub>,
}

pub enum Slot {
    Direct(usize),
    Flattened(usize),
}

pub struct Flattened {
    pub ident: Ident,
    pub ty:    TokenStream,
}

/// a field that delegates to its type's subcommand tree
pub struct Sub {
    pub ident:    Ident,
    pub ty:       TokenStream,
    pub optional: bool,
}

impl CommandFields {
    fn new(fields: &Fields, owner: &Ident) -> Result<Self, Error> {
        let mut out = Self {
            args:      Vec::new(),
            flattened: Vec::new(),
            order:     Vec::new(),
            sub:       None,
        };

        let named = match fields {
            Fields::Unit => return Ok(out),
            Fields::Tuple(_) => {
                return Err(Error::new_at_span(
                    owner.span(),
                    "pound: tuple fields are not supported, use named fields",
                ));
            },
            Fields::Named(named) => named,
        };

        for field in named.fields.items() {
            let attributes = pound(&field.attributes)?;
            let ty = FieldType::new(&field.ty);

            if attributes.subcommand {
                attributes.allow_only(&["subcommand"])?;

                if out.sub.is_some() {
                    return Err(Error::new_at_span(
                        field.name.span(),
                        "pound: only one #[pound(subcommand)] field is allowed",
                    ));
                }

                let optional = match ty.shape {
                    Shape::One => false,
                    Shape::Opt => true,
                    Shape::Bool | Shape::Many => {
                        return Err(Error::new_at_span(
                            field.name.span(),
                            "pound: #[pound(subcommand)] must be `T` or `Option<T>`",
                        ));
                    },
                };

                out.sub = Some(Sub {
                    ident: field.name.clone(),
                    ty: ty.inner,
                    optional,
                });
            } else if attributes.flatten {
                attributes.allow_only(&["flatten"])?;

                if ty.shape != Shape::One {
                    return Err(Error::new_at_span(
                        field.name.span(),
                        "pound: #[pound(flatten)] needs a plain `Parse` type",
                    ));
                }

                out.order.push(Slot::Flattened(out.flattened.len()));

                out.flattened.push(Flattened {
                    ident: field.name.clone(),
                    ty:    ty.inner,
                });
            } else {
                attributes.allow_only(FIELD_ATTRIBUTES)?;
                out.order.push(Slot::Direct(out.args.len()));
                out.args.push(Arg::new(field, ty, attributes)?);
            }
        }

        Ok(out)
    }
}

/// a field's type and how many values it holds
pub struct FieldType {
    pub shape: Shape,
    /// the type `FromArg` reads, inside any `Option` or `Vec`
    pub inner: TokenStream,
    /// the field's type as written
    pub full:  TokenStream,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Bool,
    One,
    Opt,
    Many,
}

impl FieldType {
    fn new(ty: &TypeExpr) -> Self {
        let toks = &ty.tokens;
        let full = toks.iter().cloned().collect::<TokenStream>();
        let text = toks.iter().map(ToString::to_string).collect::<String>();

        let (shape, inner) = if text == "bool" {
            (Shape::Bool, quote!(bool))
        } else if let Some(inner) = Self::strip_wrapper(toks, "Option", "option", &["core", "std"])
        {
            (Shape::Opt, inner)
        } else if let Some(inner) = Self::strip_wrapper(toks, "Vec", "vec", &["alloc", "std"]) {
            (Shape::Many, inner)
        } else {
            (Shape::One, full.clone())
        };

        Self { shape, inner, full }
    }

    // `Wrapper < Inner >` -> the `Inner` tokens, where `Wrapper` may also be
    // written `krate::module::Wrapper` with an optional leading `::`.
    fn strip_wrapper(
        toks: &[TokenTree],
        wrapper: &str,
        module: &str,
        crates: &[&str],
    ) -> Option<TokenStream> {
        let is_colon = |tok: Option<&TokenTree>| matches!(tok, Some(TokenTree::Punct(p)) if p.as_char() == ':');

        let mut at = if is_colon(toks.first()) && is_colon(toks.get(1)) {
            2
        } else {
            0
        };

        let mut path = Vec::new();

        while let Some(TokenTree::Ident(id)) = toks.get(at) {
            path.push(id.to_string());
            at += 1;

            if is_colon(toks.get(at)) && is_colon(toks.get(at + 1)) {
                at += 2;
            } else {
                break;
            }
        }

        let known = match path.iter().map(String::as_str).collect::<Vec<_>>()[..] {
            [name] => name == wrapper,
            [krate, home, name] => crates.contains(&krate) && home == module && name == wrapper,
            _ => false,
        };

        let open_ok = matches!(toks.get(at), Some(TokenTree::Punct(p)) if p.as_char() == '<');
        let close_ok = matches!(toks.last(), Some(TokenTree::Punct(p)) if p.as_char() == '>');

        if known && open_ok && close_ok && toks.len() >= at + 3 {
            Some(toks[at + 1..toks.len() - 1].iter().cloned().collect())
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ArgKind {
    Flag,
    Count,
    Opt,
    Positional,
    Trailing,
}

impl ArgKind {
    fn new(a: &Pound, shape: Shape, ident: &Ident) -> Result<Self, Error> {
        let shape_count =
            usize::from(a.positional) + usize::from(a.trailing) + usize::from(a.count);

        if shape_count > 1 {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: positional, trailing, and count are mutually exclusive",
            ));
        }

        if shape == Shape::Bool && shape_count > 0 {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: bool fields cannot be positional, trailing, or counted",
            ));
        }

        if (a.positional || a.trailing) && a.is_named() {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: positional fields cannot have short or long names",
            ));
        }

        if a.trailing && shape != Shape::Many {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: #[pound(trailing)] needs a `Vec` field",
            ));
        }

        if a.count && shape != Shape::One {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: #[pound(count)] needs a scalar field",
            ));
        }

        let kind = if shape == Shape::Bool {
            Self::Flag
        } else if a.count {
            Self::Count
        } else if a.trailing {
            Self::Trailing
        } else if a.is_named() {
            Self::Opt
        } else {
            Self::Positional
        };

        if !kind.is_named() && !a.aliases.is_empty() {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: aliases need a flag or option",
            ));
        }

        if kind == Self::Count && (a.default.is_some() || a.env.is_some()) {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: counted fields cannot use a fallback",
            ));
        }

        Ok(kind)
    }

    fn check(self, a: &Pound, ident: &Ident) -> Result<(), Error> {
        if a.heading.is_some() && !self.is_named() {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: #[pound(heading)] needs a flag or option, positionals are listed under \
                 Arguments",
            ));
        }

        if a.default_missing.is_some() && self != Self::Opt {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: #[pound(default_missing)] needs a value option (short/long)",
            ));
        }

        if !self.takes_value()
            && let Some(name) = [
                ("min", a.min.is_some()),
                ("max", a.max.is_some()),
                ("max_len", a.max_len.is_some()),
                ("parse", a.parse.is_some()),
                ("validate", a.validate.is_some()),
                ("complete", a.complete.is_some()),
                ("hint", a.hint.is_some()),
            ]
            .into_iter()
            .find_map(|(name, present)| present.then_some(name))
        {
            return Err(Error::new_at_span(
                ident.span(),
                format!("pound: #[pound({name})] needs a value field"),
            ));
        }

        Ok(())
    }

    pub const fn is_named(self) -> bool {
        matches!(self, Self::Flag | Self::Count | Self::Opt)
    }

    pub const fn takes_value(self) -> bool {
        matches!(self, Self::Opt | Self::Positional | Self::Trailing)
    }
}

/// how one raw value becomes the field's inner type
pub enum Conversion {
    FromArg,
    CheckedFromArg {
        min:      Option<String>,
        max:      Option<String>,
        max_len:  Option<TokenStream>,
        validate: Option<TokenStream>,
    },
    CustomParse {
        parse:    TokenStream,
        validate: Option<TokenStream>,
    },
}

impl Conversion {
    fn new(a: &Pound, ident: &Ident) -> Result<Self, Error> {
        if let Some(parse) = &a.parse {
            if a.min.is_some() || a.max.is_some() || a.max_len.is_some() {
                return Err(Error::new_at_span(
                    ident.span(),
                    "pound: #[pound(parse = \"...\")] cannot be combined with min, max, or \
                     max_len, use validate instead",
                ));
            }

            return Ok(Self::CustomParse {
                parse:    parse.clone(),
                validate: a.validate.clone(),
            });
        }

        if a.min.is_some() || a.max.is_some() || a.max_len.is_some() || a.validate.is_some() {
            return Ok(Self::CheckedFromArg {
                min:      a.min.clone(),
                max:      a.max.clone(),
                max_len:  a
                    .max_len
                    .as_deref()
                    .map(|text| {
                        TokenStream::from_str(text).map_err(|_| {
                            Error::new_at_span(ident.span(), "pound: invalid max_len bound")
                        })
                    })
                    .transpose()?,
                validate: a.validate.clone(),
            });
        }

        Ok(Self::FromArg)
    }

    /// whether the value goes through `FromArg`, which is what exposes a
    /// choice type's possible values. a custom parser may target a type
    /// without `FromArg`
    pub const fn reads_from_arg(&self) -> bool {
        matches!(self, Self::FromArg | Self::CheckedFromArg { .. })
    }
}

/// one field that parses as an argument
#[allow(clippy::struct_excessive_bools)]
pub struct Arg {
    pub ident:           Ident,
    pub kind:            ArgKind,
    pub ty:              FieldType,
    pub long:            Option<String>,
    pub short:           Option<char>,
    pub required:        bool,
    pub min_values:      Option<usize>,
    pub max_values:      Option<usize>,
    pub group:           Option<String>,
    pub default:         Option<Text>,
    pub default_missing: Option<String>,
    pub env:             Option<String>,
    pub negate:          Option<String>,
    pub value_name:      String,
    pub help:            String,
    pub long_help:       Option<String>,
    pub heading:         Option<String>,
    pub aliases:         Vec<String>,
    pub conflicts_with:  Vec<String>,
    pub requires:        Vec<String>,
    pub hidden:          bool,
    pub global:          bool,
    pub exclusive:       bool,
    pub complete:        Option<TokenStream>,
    pub hint:            Option<ValueHint>,
    /// `None` for the kinds that take no value
    pub conversion:      Option<Conversion>,
}

impl Arg {
    fn new(field: &NamedField, ty: FieldType, a: Pound) -> Result<Self, Error> {
        let ident = &field.name;
        let fname = ident.to_string();
        let kind = ArgKind::new(&a, ty.shape, ident)?;

        // long/short only for named kinds, defaulting a long name when neither
        // given.
        let (mut long, mut short) = (None, None);

        if kind.is_named() {
            if let Some(l) = &a.long {
                long = Some(l.clone().unwrap_or_else(|| fname.replace('_', "-")));
            }

            if let Some(s) = &a.short {
                short = Some(s.unwrap_or_else(|| fname.chars().next().unwrap_or('?')));
            }

            if long.is_none() && short.is_none() {
                long = Some(fname.replace('_', "-"));
            }
        }

        let doc = a.help.clone().unwrap_or_else(|| doc(&field.attributes));
        let summary = summary(&doc);

        let help = if a.help.is_some() {
            summary
        } else {
            short_help(&doc)
        }
        .to_owned();

        let long_help = a
            .long_help
            .clone()
            .or_else(|| (summary != doc).then(|| doc.clone()));

        kind.check(&a, ident)?;
        let (min_values, max_values) = Self::arity(&a, ty.shape == Shape::Many, ident)?;
        let negate = Self::negation(&a, kind, long.as_deref(), ident)?;

        let required = kind.takes_value() && ty.shape == Shape::One && a.default.is_none();

        let conversion = kind
            .takes_value()
            .then(|| Conversion::new(&a, ident))
            .transpose()?;

        Ok(Self {
            ident: ident.clone(),
            kind,
            ty,
            long,
            short,
            required,
            min_values,
            max_values,
            group: a.group,
            default: a.default,
            default_missing: a.default_missing,
            env: a.env,
            negate,
            value_name: a.value_name.unwrap_or(fname),
            help,
            long_help,
            heading: a.heading,
            aliases: a.aliases,
            conflicts_with: a.conflicts_with,
            requires: a.requires,
            hidden: a.hidden,
            global: a.global,
            exclusive: a.exclusive,
            complete: a.complete,
            hint: a.hint,
            conversion,
        })
    }

    // the `min_values`/`max_values` bounds, which only a `Vec` field can satisfy.
    fn arity(
        a: &Pound,
        multi: bool,
        ident: &Ident,
    ) -> Result<(Option<usize>, Option<usize>), Error> {
        let read = |name: &str, raw: Option<&String>| {
            match raw {
                None => Ok(None),
                Some(_) if !multi => {
                    Err(Error::new_at_span(
                        ident.span(),
                        format!("pound: #[pound({name})] needs a `Vec` field"),
                    ))
                },
                Some(text) => {
                    text.parse::<usize>().map(Some).map_err(|_| {
                        Error::new_at_span(
                            ident.span(),
                            format!("pound: #[pound({name} = {text})] is not a count"),
                        )
                    })
                },
            }
        };

        let min = read("min_values", a.min_values.as_ref())?;
        let max = read("max_values", a.max_values.as_ref())?;

        if let (Some(min), Some(max)) = (min, max)
            && min > max
        {
            return Err(Error::new_at_span(
                ident.span(),
                format!("pound: min_values {min} exceeds max_values {max}"),
            ));
        }

        Ok((min, max))
    }

    fn negation(
        a: &Pound,
        kind: ArgKind,
        long: Option<&str>,
        ident: &Ident,
    ) -> Result<Option<String>, Error> {
        let Some(spelling) = &a.negate else {
            return Ok(None);
        };

        if kind != ArgKind::Flag {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: #[pound(negate)] needs a bool field",
            ));
        }

        let Some(name) = long else {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: #[pound(negate)] needs a long name",
            ));
        };

        let spelling = spelling.clone().unwrap_or_else(|| format!("no-{name}"));

        if spelling == name || a.aliases.contains(&spelling) {
            return Err(Error::new_at_span(
                ident.span(),
                format!("pound: negation `{spelling}` is also a positive spelling"),
            ));
        }

        Ok(Some(spelling))
    }
}

/// a `#[derive(ValueEnum)]` input, each variant with the word that selects it
pub struct ValueEnum {
    pub ident:    Ident,
    pub variants: Vec<(Ident, String)>,
}

impl ValueEnum {
    pub fn new(item: &Item) -> Result<Self, Error> {
        let Item::Enum(e) = item else {
            return Err(Error::new("pound: ValueEnum can only derive on an enum"));
        };

        pound(&e.attributes)?.allow_only(&[])?;
        let mut variants = Vec::<(Ident, String)>::new();

        for variant in e.variants.items() {
            if !matches!(variant.fields, Fields::Unit) {
                return Err(Error::new_at_span(
                    variant.name.span(),
                    "pound: ValueEnum needs unit variants only",
                ));
            }

            let vname = &variant.name;
            let vattr = pound(&variant.attributes)?;
            vattr.allow_only(VALUE_VARIANT_ATTRIBUTES)?;

            let label = vattr
                .name
                .unwrap_or_else(|| camel_to_kebab(&vname.to_string()));

            if let Some((first, _)) = variants.iter().find(|(_, other)| *other == label) {
                return Err(Error::new_at_span(
                    vname.span(),
                    format!(
                        "pound: ValueEnum variants `{first}` and `{vname}` both spell `{label}`, \
                         so `{vname}` could never be parsed"
                    ),
                ));
            }

            variants.push((vname.clone(), label));
        }

        Ok(Self {
            ident: e.name.clone(),
            variants,
        })
    }
}

fn camel_to_kebab(name: &str) -> String {
    let mut out = String::new();

    for (i, ch) in name.chars().enumerate() {
        if ch == '_' {
            out.push('-');
        } else if ch.is_ascii_uppercase() {
            if i > 0 {
                out.push('-');
            }

            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }

    out
}
