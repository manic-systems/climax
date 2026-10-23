// SPDX-License-Identifier: EUPL-1.2

//! what a derive input means, checked against pound's rules before any tokens
//! are written

use core::{
    iter::once,
    mem::take,
    str::FromStr as _,
};
use std::collections::HashMap;

use proc_macro2::{
    Ident,
    Span,
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
    NumArgs,
    Pound,
    Text,
    ValueHint,
    doc,
    native_deprecated,
    pound,
    short_help,
    summary,
};

const ITEM_ATTRIBUTES: &[&str] = &[
    "name",
    "version",
    "long_version",
    "required_group",
    "help_if_empty",
    "before_help",
    "after_help",
];
const VARIANT_ATTRIBUTES: &[&str] = &[
    "name",
    "display_order",
    "alias",
    "hidden",
    "deprecated",
    "required_group",
    "help_if_empty",
    "before_help",
    "after_help",
];
const VALUE_VARIANT_ATTRIBUTES: &[&str] = &["name", "alias", "hidden", "deprecated"];
const FIELD_ATTRIBUTES: &[&str] = &[
    "short",
    "long",
    "positional",
    "trailing",
    "last",
    "count",
    "hidden",
    "deprecated",
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
    "display_order",
    "min",
    "max",
    "max_len",
    "min_values",
    "max_values",
    "delimiter",
    "num_args",
    "parse",
    "validate",
    "complete",
    "hint",
    "conflicts_with",
    "requires",
    "alias",
    "short_alias",
    "hide_possible",
    "hide_default",
    "hide_env",
    "allow_hyphen_values",
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
            Item::Struct(shape) => {
                let fields = CommandFields::new(&shape.fields, &shape.name)?;
                let mut header = Header::new(&shape.attributes)?;
                let command = Command::new(
                    &shape.name,
                    shape.name.to_string(),
                    fields,
                    &header.required_groups,
                    take(&mut header.group_refs),
                )?;

                Ok(Self::Struct {
                    ident: shape.name.clone(),
                    header,
                    command: Box::new(command),
                })
            },
            Item::Enum(shape) => {
                if shape.variants.is_empty() {
                    return Err(Error::new_at_span(
                        shape.name.span(),
                        "pound: a command enum needs at least one variant",
                    ));
                }
                let header = Header::new(&shape.attributes)?;
                let variants = shape
                    .variants
                    .items()
                    .map(|variant| Variant::new(&shape.name, variant))
                    .collect::<Result<_, _>>()?;
                Ok(Self::Enum {
                    ident: shape.name.clone(),
                    header,
                    variants,
                })
            },
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
            | _ => {
                Err(Error::new(
                    "pound: Parse can only derive on a struct or enum",
                ))
            },
        }
    }
}

pub struct Prose {
    pub doc:         String,
    pub before_help: Option<String>,
    pub after_help:  Option<String>,
}

impl Prose {
    fn new(
        attributes: &[Attribute],
        before_help: Option<String>,
        after_help: Option<String>,
    ) -> Self {
        Self {
            doc: doc(attributes),
            before_help,
            after_help,
        }
    }
}

/// what the top of a derived command says about itself
pub struct Header {
    pub name:            Option<String>,
    pub version:         Option<Vec<TokenTree>>,
    pub long_version:    Option<Vec<TokenTree>>,
    pub prose:           Prose,
    /// groups that need exactly one member set, which only a struct's command
    /// reads
    pub required_groups: Vec<String>,
    /// moved into the command, which links them
    pub group_refs:      Vec<Ident>,
    pub help_if_empty:   bool,
}

impl Header {
    fn new(attributes: &[Attribute]) -> Result<Self, Error> {
        let item = pound(attributes)?;
        item.allow_only(ITEM_ATTRIBUTES)?;

        Ok(Self {
            name:            item.name,
            version:         item.version,
            long_version:    item.long_version,
            prose:           Prose::new(attributes, item.before_help, item.after_help),
            required_groups: item.required_groups,
            group_refs:      item.group_refs,
            help_if_empty:   item.help_if_empty,
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
    Command(Box<Named>),
}

pub struct Named {
    pub name:          String,
    pub aliases:       Vec<String>,
    pub hidden:        bool,
    pub deprecated:    Option<String>,
    pub display_order: Option<usize>,
    pub help_if_empty: bool,
    pub prose:         Prose,
    pub command:       Command,
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
            vattr.group_refs,
        )?;

        Ok(Self {
            kind: VariantKind::Command(Box::new(Named {
                name: vattr
                    .name
                    .unwrap_or_else(|| camel_to_kebab(&ident.to_string())),
                aliases: vattr.aliases,
                hidden: vattr.hidden,
                deprecated: vattr
                    .deprecated
                    .or_else(|| native_deprecated(&variant.attributes)),
                display_order: vattr.display_order,
                help_if_empty: vattr.help_if_empty,
                prose: Prose::new(&variant.attributes, vattr.before_help, vattr.after_help),
                command,
            })),
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
                TokenTree::Punct(punct) => punct.as_char() == ':',
                TokenTree::Group(_) | TokenTree::Literal(_) => false,
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
    pub owner:         String,
    pub fields:        CommandFields,
    pub groups:        Vec<Group>,
    /// bare `required_group` names, the args holding their own `group` ones
    pub required_refs: Vec<Ident>,
    /// pairs of arg indices that cannot be set together, smaller index first
    pub conflicts:     Vec<(usize, usize)>,
    /// pairs where setting the first arg obliges the second
    pub requires:      Vec<(usize, usize)>,
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
        required_refs: Vec<Ident>,
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

        for group_name in named.chain(required_groups) {
            if !groups.iter().any(|group| group.name == *group_name) {
                groups.push(Group {
                    name:     group_name.clone(),
                    required: required_groups.contains(group_name),
                    direct:   args
                        .iter()
                        .any(|arg| arg.group.as_ref() == Some(group_name)),
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

        // a name matching no field at all is left to rustc, which reports it on
        // the emitted field reference, so the expansion still holds the
        // half-typed name rust-analyzer completes against
        let lookup = |key: &str, field: &Ident| {
            let field_name = field.to_string();
            let other = fields
                .flattened
                .iter()
                .map(|flat| &flat.ident)
                .chain(fields.sub.iter().map(|sub| &sub.ident))
                .any(|ident| *ident == field_name);

            match index.get(&field_name) {
                Some(&j) => Ok(Some(j)),
                None if other => {
                    Err(Error::new_at_span(
                        field.span(),
                        format!("pound: {key}: `{field}` is not an arg of this command"),
                    ))
                },
                None => Ok(None),
            }
        };

        let mut conflicts = Vec::new();

        for (i, arg) in args.iter().enumerate() {
            for field in &arg.conflicts_with {
                let Some(j) = lookup("conflicts_with", field)? else {
                    continue;
                };
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
                let Some(j) = lookup("requires", field)? else {
                    continue;
                };

                if i != j && !requires.contains(&(i, j)) {
                    requires.push((i, j));
                }
            }
        }

        Ok(Self {
            owner,
            fields,
            groups,
            required_refs,
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
                    Shape::Bool | Shape::Many | Shape::OptMany | Shape::OptOpt => {
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
    pub shape:   Shape,
    /// the type `FromArg` reads, inside any `Option`, `Vec` or `Sourced`
    pub inner:   TokenStream,
    pub sourced: bool,
    /// the field's type as written
    pub full:    TokenStream,
    /// where an error about `inner` points
    pub span:    Span,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Bool,
    One,
    Opt,
    Many,
    /// `Option<Vec<T>>`, `None` when the arg was never given
    OptMany,
    /// `Option<Option<T>>`, `Some(None)` when the option was given with no
    /// value
    OptOpt,
}

impl Shape {
    pub const fn is_many(self) -> bool {
        matches!(self, Self::Many | Self::OptMany)
    }
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
            let inner_toks = inner.clone().into_iter().collect::<Vec<_>>();
            let many = Self::strip_wrapper(&inner_toks, "Vec", "vec", &["alloc", "std"]);
            let maybe = Self::strip_wrapper(&inner_toks, "Option", "option", &["core", "std"]);
            match (many, maybe) {
                (Some(item), _) => (Shape::OptMany, item),
                (None, Some(item)) => (Shape::OptOpt, item),
                (None, None) => (Shape::Opt, inner),
            }
        } else if let Some(inner) = Self::strip_wrapper(toks, "Vec", "vec", &["alloc", "std"]) {
            (Shape::Many, inner)
        } else {
            (Shape::One, full.clone())
        };

        let wrapped = inner.clone().into_iter().collect::<Vec<_>>();
        let (value, sourced) = Self::strip_path(&wrapped, |path| path.last() == Some(&"Sourced"))
            .map_or((inner, false), |stripped| (stripped, true));
        let resolved_shape = if sourced && shape == Shape::One && value.to_string() == "bool" {
            Shape::Bool
        } else {
            shape
        };

        let span = value
            .clone()
            .into_iter()
            .next()
            .map_or_else(Span::call_site, |token| token.span());

        Self {
            shape: resolved_shape,
            inner: value,
            sourced,
            full,
            span,
        }
    }

    // `Wrapper < Inner >` -> the `Inner` tokens, where `Wrapper` may also be
    // written `krate::module::Wrapper` with an optional leading `::`.
    fn strip_wrapper(
        toks: &[TokenTree],
        wrapper: &str,
        module: &str,
        crates: &[&str],
    ) -> Option<TokenStream> {
        Self::strip_path(toks, |path| {
            match path {
                [name] => *name == wrapper,
                [krate, home, name] => {
                    crates.contains(krate) && *home == module && *name == wrapper
                },
                _ => false,
            }
        })
    }

    fn strip_path(toks: &[TokenTree], known: impl Fn(&[&str]) -> bool) -> Option<TokenStream> {
        let is_colon = |tok: Option<&TokenTree>| matches!(tok, Some(TokenTree::Punct(punct)) if punct.as_char() == ':');

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

        let is_known = known(&path.iter().map(String::as_str).collect::<Vec<_>>());

        let open_ok =
            matches!(toks.get(at), Some(TokenTree::Punct(punct)) if punct.as_char() == '<');
        let close_ok =
            matches!(toks.last(), Some(TokenTree::Punct(punct)) if punct.as_char() == '>');

        (is_known && open_ok && close_ok && toks.len() >= at + 3)
            .then(|| toks[at + 1..toks.len() - 1].iter().cloned().collect())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ArgKind {
    Flag,
    Count,
    Opt,
    Positional,
    Trailing,
    Last,
}

impl ArgKind {
    #[expect(
        clippy::too_many_lines,
        reason = "one guard per attribute conflict, read top to bottom"
    )]
    fn new(attr: &Pound, ty: &FieldType, ident: &Ident) -> Result<Self, Error> {
        let (shape, sourced) = (ty.shape, ty.sourced);
        let shape_count = usize::from(attr.positional)
            + usize::from(attr.trailing)
            + usize::from(attr.last)
            + usize::from(attr.count);

        if shape_count > 1 {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: positional, trailing, last, and count are mutually exclusive",
            ));
        }

        if shape == Shape::Bool && shape_count > 0 {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: bool fields cannot be positional, trailing, last, or counted",
            ));
        }

        if (attr.positional || attr.trailing || attr.last) && attr.is_named() {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: positional fields cannot have short or long names",
            ));
        }

        if attr.trailing && !shape.is_many() {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: #[pound(trailing)] needs a `Vec` or `Option<Vec>` field",
            ));
        }

        if attr.last && !shape.is_many() {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: #[pound(last)] needs a `Vec` or `Option<Vec>` field",
            ));
        }

        if attr.delimiter.is_some() && (!shape.is_many() || attr.trailing || attr.last) {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: #[pound(delimiter)] needs a `Vec` or `Option<Vec>` field that is not \
                 trailing or last",
            ));
        }

        if attr.min_values.is_some() && shape == Shape::OptMany {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: #[pound(min_values)] needs a `Vec` field, an `Option<Vec>` is `None` when \
                 never given",
            ));
        }

        if attr.num_args.is_some() && !shape.is_many() {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: #[pound(num_args)] needs a `Vec` or `Option<Vec>` field",
            ));
        }

        if attr.num_args.is_some() && attr.delimiter.is_some() {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: #[pound(num_args)] counts words and #[pound(delimiter)] splits them, pick \
                 one",
            ));
        }

        if shape == Shape::OptOpt
            && (!attr.is_named() || attr.count || attr.default_missing.is_some())
        {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: an `Option<Option<T>>` field needs a value option without \
                 `default_missing`",
            ));
        }

        if sourced && (!matches!(shape, Shape::Bool | Shape::One | Shape::Opt) || attr.count) {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: `Sourced<T>` holds one value, as `Sourced<T>`, `Option<Sourced<T>>` or a \
                 `Sourced<bool>` flag",
            ));
        }

        if attr.count && shape != Shape::One {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: #[pound(count)] needs a scalar field",
            ));
        }

        let kind = if shape == Shape::Bool {
            Self::Flag
        } else if attr.count {
            Self::Count
        } else if attr.trailing {
            Self::Trailing
        } else if attr.last {
            Self::Last
        } else if attr.is_named() {
            Self::Opt
        } else {
            Self::Positional
        };

        let aliased = !attr.aliases.is_empty() || !attr.short_aliases.is_empty();
        if aliased && !kind.is_named() {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: aliases need a flag or option",
            ));
        }

        if kind == Self::Count && (attr.default.is_some() || attr.env.is_some()) {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: counted fields cannot use a fallback",
            ));
        }

        Ok(kind)
    }

    fn check(self, attr: &Pound, ident: &Ident) -> Result<(), Error> {
        if attr.display_order.is_some() && !self.is_named() {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: #[pound(display_order)] needs a flag or option, positionals are listed in \
                 the order they fill",
            ));
        }

        if attr.hyphen_values && !matches!(self, Self::Opt | Self::Positional) {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: #[pound(allow_hyphen_values)] needs an option or a positional",
            ));
        }

        if attr.heading.is_some() && !self.is_named() {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: #[pound(heading)] needs a flag or option, positionals are listed under \
                 Arguments",
            ));
        }

        if attr.num_args.is_some() && (self != Self::Opt || attr.default_missing.is_some()) {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: #[pound(num_args)] needs an option without `default_missing`",
            ));
        }

        if attr.default_missing.is_some() && self != Self::Opt {
            return Err(Error::new_at_span(
                ident.span(),
                "pound: #[pound(default_missing)] needs a value option (short/long)",
            ));
        }

        if !self.takes_value()
            && let Some(name) = [
                ("min", attr.min.is_some()),
                ("max", attr.max.is_some()),
                ("max_len", attr.max_len.is_some()),
                ("parse", attr.parse.is_some()),
                ("validate", attr.validate.is_some()),
                ("complete", attr.complete.is_some()),
                ("hint", attr.hint.is_some()),
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
        matches!(
            self,
            Self::Opt | Self::Positional | Self::Trailing | Self::Last
        )
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
    fn new(attr: &Pound, ident: &Ident) -> Result<Self, Error> {
        if let Some(parse) = &attr.parse {
            if attr.min.is_some() || attr.max.is_some() || attr.max_len.is_some() {
                return Err(Error::new_at_span(
                    ident.span(),
                    "pound: #[pound(parse = \"...\")] cannot be combined with min, max, or \
                     max_len, use validate instead",
                ));
            }

            return Ok(Self::CustomParse {
                parse:    parse.clone(),
                validate: attr.validate.clone(),
            });
        }

        if attr.min.is_some()
            || attr.max.is_some()
            || attr.max_len.is_some()
            || attr.validate.is_some()
        {
            return Ok(Self::CheckedFromArg {
                min:      attr.min.clone(),
                max:      attr.max.clone(),
                max_len:  attr
                    .max_len
                    .as_deref()
                    .map(|text| {
                        TokenStream::from_str(text).map_err(|parse_error| {
                            Error::new_at_span(
                                ident.span(),
                                format!("pound: invalid max_len bound: {parse_error}"),
                            )
                        })
                    })
                    .transpose()?,
                validate: attr.validate.clone(),
            });
        }

        Ok(Self::FromArg)
    }

    /// whether the value goes through `FromArg` or its `FromStr` fallback,
    /// which is what exposes a choice type's possible values. a custom parser
    /// may target a type with neither
    pub const fn reads_from_arg(&self) -> bool {
        matches!(self, Self::FromArg | Self::CheckedFromArg { .. })
    }
}

/// one field that parses as an argument
#[expect(
    clippy::struct_excessive_bools,
    reason = "argument fields need independent boolean switches"
)]
pub struct Arg {
    pub ident:           Ident,
    pub kind:            ArgKind,
    pub ty:              FieldType,
    pub long:            Option<String>,
    pub short:           Option<char>,
    pub required:        bool,
    pub min_values:      Option<usize>,
    pub max_values:      Option<usize>,
    pub delimiter:       Option<char>,
    pub num_args:        Option<NumArgs>,
    pub group:           Option<String>,
    pub group_refs:      Vec<Ident>,
    pub default:         Option<Text>,
    pub default_missing: Option<String>,
    pub env:             Option<String>,
    pub negate:          Option<String>,
    pub value_name:      String,
    pub help:            String,
    pub long_help:       Option<String>,
    pub heading:         Option<String>,
    pub display_order:   Option<usize>,
    pub aliases:         Vec<String>,
    pub short_aliases:   Vec<char>,
    pub conflicts_with:  Vec<Ident>,
    pub requires:        Vec<Ident>,
    pub hidden:          bool,
    pub deprecated:      Option<String>,
    pub hide_possible:   bool,
    pub hide_default:    bool,
    pub hide_env:        bool,
    pub hyphen_values:   bool,
    pub global:          bool,
    pub exclusive:       bool,
    pub complete:        Option<TokenStream>,
    pub hint:            Option<ValueHint>,
    /// `None` for the kinds that take no value
    pub conversion:      Option<Conversion>,
}

impl Arg {
    fn new(field: &NamedField, ty: FieldType, attr: Pound) -> Result<Self, Error> {
        let ident = &field.name;
        let fname = ident.to_string().trim_start_matches("r#").to_owned();
        let kind = ArgKind::new(&attr, &ty, ident)?;

        // long/short only for named kinds, defaulting a long name when neither
        // given.
        let (mut long, mut short) = (None, None);

        if kind.is_named() {
            if let Some(given) = &attr.long {
                long = Some(given.or_infer(|| fname.replace('_', "-")));
            }

            if let Some(given) = &attr.short {
                short = Some(given.or_infer(|| fname.chars().next().unwrap_or('?')));
            }

            if long.is_none() && short.is_none() {
                long = Some(fname.replace('_', "-"));
            }
        }

        let doc = attr.help.clone().unwrap_or_else(|| doc(&field.attributes));
        let summary = summary(&doc);

        let help = if attr.help.is_some() {
            summary
        } else {
            short_help(&doc)
        }
        .to_owned();

        let long_help = attr
            .long_help
            .clone()
            .or_else(|| (summary != doc).then(|| doc.clone()));

        kind.check(&attr, ident)?;
        let (min_values, max_values) = Self::arity(&attr, ty.shape.is_many(), ident)?;
        let negate = Self::negation(&attr, kind, long.as_deref(), ident)?;

        let required = kind.takes_value() && ty.shape == Shape::One && attr.default.is_none();

        let conversion = kind
            .takes_value()
            .then(|| Conversion::new(&attr, ident))
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
            delimiter: attr.delimiter,
            num_args: attr.num_args,
            group: attr.group,
            group_refs: attr.group_refs,
            default: attr.default,
            default_missing: attr.default_missing,
            env: attr.env,
            negate,
            value_name: attr.value_name.unwrap_or(fname),
            help,
            long_help,
            heading: attr.heading,
            display_order: attr.display_order,
            aliases: attr.aliases,
            short_aliases: attr.short_aliases,
            conflicts_with: attr.conflicts_with,
            requires: attr.requires,
            hidden: attr.hidden,
            deprecated: attr
                .deprecated
                .or_else(|| native_deprecated(&field.attributes)),
            hide_possible: attr.hide_possible,
            hide_default: attr.hide_default,
            hide_env: attr.hide_env,
            hyphen_values: attr.hyphen_values,
            global: attr.global,
            exclusive: attr.exclusive,
            complete: attr.complete,
            hint: attr.hint,
            conversion,
        })
    }

    // the `min_values`/`max_values` bounds, which only a `Vec` field can
    // satisfy.
    fn arity(
        attr: &Pound,
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
                    text.parse::<usize>().map(Some).map_err(|parse_error| {
                        Error::new_at_span(
                            ident.span(),
                            format!(
                                "pound: #[pound({name} = {text})] is not a count: {parse_error}"
                            ),
                        )
                    })
                },
            }
        };

        let min = read("min_values", attr.min_values.as_ref())?;
        let max = read("max_values", attr.max_values.as_ref())?;

        if let (Some(minimum), Some(maximum)) = (min, max)
            && minimum > maximum
        {
            return Err(Error::new_at_span(
                ident.span(),
                format!("pound: min_values {minimum} exceeds max_values {maximum}"),
            ));
        }

        Ok((min, max))
    }

    fn negation(
        attr: &Pound,
        kind: ArgKind,
        long: Option<&str>,
        ident: &Ident,
    ) -> Result<Option<String>, Error> {
        let Some(spelling) = &attr.negate else {
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

        let resolved_spelling = spelling.or_infer(|| format!("no-{name}"));

        if resolved_spelling == name || attr.aliases.contains(&resolved_spelling) {
            return Err(Error::new_at_span(
                ident.span(),
                format!("pound: negation `{resolved_spelling}` is also a positive spelling"),
            ));
        }

        Ok(Some(resolved_spelling))
    }
}

/// a `#[derive(ValueEnum)]` input
pub struct ValueEnum {
    pub ident:       Ident,
    pub variants:    Vec<ValueVariant>,
    pub ignore_case: bool,
}

/// one variant of a `ValueEnum`, with every word that selects it
pub struct ValueVariant {
    pub ident:      Ident,
    /// the spelling listed and printed back
    pub label:      String,
    pub aliases:    Vec<String>,
    pub help:       String,
    pub hidden:     bool,
    pub deprecated: Option<String>,
}

impl ValueVariant {
    pub fn spellings(&self) -> impl Iterator<Item = &str> {
        once(self.label.as_str()).chain(self.aliases.iter().map(String::as_str))
    }
}

impl ValueEnum {
    pub fn new(item: &Item) -> Result<Self, Error> {
        let Item::Enum(shape) = item else {
            return Err(Error::new("pound: ValueEnum can only derive on an enum"));
        };

        let attributes = pound(&shape.attributes)?;
        attributes.allow_only(&["ignore_case"])?;
        let same = |left: &str, right: &str| {
            if attributes.ignore_case {
                left.eq_ignore_ascii_case(right)
            } else {
                left == right
            }
        };

        let mut variants = Vec::<ValueVariant>::new();

        for variant in shape.variants.items() {
            if !matches!(variant.fields, Fields::Unit) {
                return Err(Error::new_at_span(
                    variant.name.span(),
                    "pound: ValueEnum needs unit variants only",
                ));
            }

            let vname = &variant.name;
            let vattr = pound(&variant.attributes)?;
            vattr.allow_only(VALUE_VARIANT_ATTRIBUTES)?;

            let this = ValueVariant {
                ident:      vname.clone(),
                label:      vattr
                    .name
                    .unwrap_or_else(|| camel_to_kebab(&vname.to_string())),
                aliases:    vattr.aliases,
                help:       short_help(&doc(&variant.attributes)).to_owned(),
                hidden:     vattr.hidden,
                deprecated: vattr
                    .deprecated
                    .or_else(|| native_deprecated(&variant.attributes)),
            };

            for word in this.spellings() {
                if let Some(first_variant) = variants
                    .iter()
                    .find(|other| other.spellings().any(|word_text| same(word_text, word)))
                {
                    let first_ident = &first_variant.ident;
                    return Err(Error::new_at_span(
                        vname.span(),
                        format!(
                            "pound: ValueEnum variants `{first_ident}` and `{vname}` both spell \
                             `{word}`, so `{vname}` could never be parsed by it"
                        ),
                    ));
                }
            }

            variants.push(this);
        }

        Ok(Self {
            ident: shape.name.clone(),
            variants,
            ignore_case: attributes.ignore_case,
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
