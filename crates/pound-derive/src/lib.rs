// SPDX-License-Identifier: EUPL-1.2

//! derive macros for pound. `#[derive(Parse)]` turns a struct into a flat
//! command and an enum into a subcommand tree, `#[derive(ValueEnum)]` wires a
//! unit enum up as a `FromArg` choice type. all of it just emits the static
//! `CommandSpec` plus a `from_matches` reader, the runtime does the work.

mod attr;

use std::{
    collections::HashMap,
    process::Command,
    str::FromStr,
};

use proc_macro::TokenStream;
use proc_macro2::{
    TokenStream as TokenStream2,
    TokenTree,
};
use quote::{
    format_ident,
    quote,
};
use venial::{
    Fields,
    Item,
    NamedField,
    TypeExpr,
    parse_item,
};

use crate::attr::Pound;

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
    "conflicts_with",
    "requires",
    "alias",
];

#[proc_macro_derive(Parse, attributes(pound))]
pub fn derive_parse(input: TokenStream) -> TokenStream {
    match parse_item(input.into()) {
        Ok(Item::Struct(s)) => parse_struct(&s),
        Ok(Item::Enum(e)) => parse_enum(&e),
        Ok(_) => err("pound: Parse can only derive on a struct or enum"),
        Err(e) => TokenStream::from(e.to_compile_error()),
    }
}

#[proc_macro_derive(ValueEnum, attributes(pound))]
pub fn derive_value_enum(input: TokenStream) -> TokenStream {
    match parse_item(input.into()) {
        Ok(Item::Enum(e)) => value_enum(&e),
        Ok(_) => err("pound: ValueEnum can only derive on an enum"),
        Err(e) => TokenStream::from(e.to_compile_error()),
    }
}

// how many values a field carries.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Card {
    One,
    Opt,
    Many,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ArgKind {
    Flag,
    Count,
    Opt,
    Positional,
    Trailing,
}

impl ArgKind {
    const fn is_named(self) -> bool {
        matches!(self, Self::Flag | Self::Count | Self::Opt)
    }

    const fn takes_value(self) -> bool {
        matches!(self, Self::Opt | Self::Positional | Self::Trailing)
    }
}

// how one raw value becomes the field's inner type.
enum Conversion {
    FromArg,
    CheckedFromArg {
        min:      Option<String>,
        max:      Option<String>,
        max_len:  Option<String>,
        validate: Option<String>,
    },
    CustomParse {
        parse:    String,
        validate: Option<String>,
    },
}

// the resolved plan for one field.
#[allow(clippy::struct_excessive_bools)]
struct Plan {
    ident:           proc_macro2::Ident,
    kind:            ArgKind,
    long:            Option<String>,
    short:           Option<char>,
    required:        bool,
    multi:           bool,
    min_values:      Option<usize>,
    max_values:      Option<usize>,
    group:           Option<String>,
    default:         Option<String>,
    default_missing: Option<String>,
    env:             Option<String>,
    negate:          Option<String>,
    value_name:      String,
    help:            String,
    long_help:       Option<String>,
    heading:         Option<String>,
    aliases:         Vec<String>,
    conflicts_with:  Vec<String>,
    requires:        Vec<String>,
    hidden:          bool,
    global:          bool,
    card:            Card,
    conversion:      Option<Conversion>,
    inner_ty:        TokenStream2,
    full_ty:         TokenStream2,
}

// a field that delegates to its type's subcommand tree.
struct SubField {
    ident:    proc_macro2::Ident,
    ty:       TokenStream2,
    optional: bool,
}

struct FlattenField {
    ident: proc_macro2::Ident,
    ty:    TokenStream2,
}

impl FlattenField {
    fn reader(&self, i: usize, m: &TokenStream2) -> TokenStream2 {
        let Self { ident, ty } = self;
        quote! {
            #ident: <#ty as ::pound::Parse>::from_matches(
                <#ty as ::pound::Parse>::SPEC,
                ::pound::Matches::flattened(#m, #i),
            )?
        }
    }
}

enum FieldOrder {
    Direct(usize),
    Flattened(usize),
}

struct FieldPlan {
    args:      Vec<Plan>,
    flattened: Vec<FlattenField>,
    order:     Vec<FieldOrder>,
    sub:       Option<SubField>,
}

impl FieldPlan {
    /// the parser never descends into a flattened type's subcommands, so one
    /// that has them could never parse
    fn flatten_asserts(&self) -> TokenStream2 {
        self.flattened
            .iter()
            .map(|field| {
                let ty = &field.ty;
                let message = format!("pound: `{ty}` has subcommands and cannot be flattened");
                quote! {
                    const _: () = ::core::assert!(
                        <#ty as ::pound::Parse>::SPEC.subs.is_empty(),
                        #message
                    );
                }
            })
            .collect()
    }

    /// the builder calls that embed the flattened fields, absent when there
    /// are none so plain commands keep the default order
    fn flatten_calls(&self) -> TokenStream2 {
        if self.flattened.is_empty() {
            return TokenStream2::new();
        }
        let specs = self.flattened.iter().map(|field| {
            let ty = &field.ty;
            quote! { <#ty as ::pound::Parse>::SPEC }
        });
        let order = self.order.iter().map(|entry| {
            match entry {
                FieldOrder::Direct(i) => quote! { ::pound::ArgumentOrder::Direct(#i) },
                FieldOrder::Flattened(i) => quote! { ::pound::ArgumentOrder::Flattened(#i) },
            }
        });
        quote! {
            .flattened(&[ #(#specs),* ])
            .argument_order(&[ #(#order),* ])
        }
    }
}

fn parse_struct(s: &venial::Struct) -> TokenStream {
    let fields = match analyze(&s.fields) {
        Ok(v) => v,
        Err(e) => return err(&e),
    };
    let item = match command_attributes(&s.attributes) {
        Ok(item) => item,
        Err(e) => return err(&e),
    };
    let name = &s.name;
    let plans = &fields.args;
    let sub = &fields.sub;

    if let Err(e) = validate_fields(plans, &item.required_groups, !fields.flattened.is_empty()) {
        return err(&e);
    }
    let conflicts = match conflict_pairs(plans) {
        Ok(c) => c,
        Err(e) => return err(&e),
    };
    let requires = match require_pairs(plans) {
        Ok(r) => index_pairs(&r),
        Err(e) => return err(&e),
    };
    let args = plans.iter().map(arg_expr);
    let default_asserts = plans.iter().filter_map(default_assert);
    let flatten_asserts = fields.flatten_asserts();
    let groups = group_exprs(plans, &item.required_groups);
    let conflicts = index_pairs(&conflicts);
    let (subs, sub_optional) = sub_parts(sub.as_ref());
    let name_expr = name_expr(&item);
    let version_expr = version_expr(&item);
    let hash_call = git_hash_call();
    let (about, long_about) = about_calls(&attr::doc(&s.attributes));

    let m = quote!(m);
    let sp = quote!(spec);
    let readers = plans.iter().enumerate().map(|(i, p)| reader(p, i, &m, &sp));
    let flattened_readers = fields
        .flattened
        .iter()
        .enumerate()
        .map(|(i, f)| f.reader(i, &m));
    let sub_reader = sub.as_ref().map(|sf| sub_reader(sf, &m));
    let flatten_calls = fields.flatten_calls();
    let unique_message = format!("pound: two args of `{name}` answer to the same spelling");
    let group_asserts = group_asserts(plans, &item.required_groups, &format_ident!("CMD"));

    // avoid unused-param warnings when a command carries only a subcommand.
    let spec_param = if plans.is_empty() {
        quote!(_spec)
    } else {
        quote!(spec)
    };
    let m_param = if plans.is_empty() && fields.flattened.is_empty() && sub.is_none() {
        quote!(_m)
    } else {
        quote!(m)
    };

    // parameterless builder, so only chain it when the flag is actually set.
    let sub_optional_call = if sub_optional {
        quote!(.sub_optional())
    } else {
        quote!()
    };

    quote! {
        impl ::pound::Parse for #name {
            const SPEC: &'static ::pound::CommandSpec = {
                #(#default_asserts)*
                #flatten_asserts
                const ARGS: &[::pound::ArgSpec] = &[ #(#args),* ];
                const GROUPS: &[::pound::GroupSpec] = &[ #(#groups),* ];
                const CONFLICTS: &[(usize, usize)] = #conflicts;
                const REQUIRES: &[(usize, usize)] = #requires;
                const CMD: ::pound::CommandSpec = ::pound::CommandSpec::new(#name_expr)
                    .version(#version_expr)
                    #hash_call
                    #about
                    #long_about
                    .args(ARGS)
                    #flatten_calls
                    .groups(GROUPS)
                    .conflicts(CONFLICTS)
                    .requires(REQUIRES)
                    .subs(#subs)
                    #sub_optional_call;
                const _: () = ::core::assert!(::pound::names_unique(&CMD), #unique_message);
                #group_asserts
                &CMD
            };

            fn from_matches(#spec_param: &'static ::pound::CommandSpec, #m_param: &::pound::Matches)
                -> ::core::result::Result<Self, ::pound::Error>
            {
                ::core::result::Result::Ok(Self {
                    #(#readers,)* #(#flattened_readers,)* #sub_reader
                })
            }
        }
    }
    .into()
}

#[allow(
    clippy::too_many_lines,
    reason = "one cohesive codegen pass reads best whole"
)]
fn parse_enum(e: &venial::Enum) -> TokenStream {
    if e.variants.is_empty() {
        return err("pound: a command enum needs at least one variant");
    }
    let item = match command_attributes(&e.attributes) {
        Ok(item) => item,
        Err(e) => return err(&e),
    };
    let name = &e.name;
    let name_expr = name_expr(&item);
    let version_expr = version_expr(&item);
    let hash_call = git_hash_call();
    let (about, long_about) = about_calls(&attr::doc(&e.attributes));

    let mut sub_consts = Vec::new();
    let mut sub_specs = Vec::new();
    let mut arms = Vec::new();
    let mut uses_spec = false;
    let mut indexed_variants = 0;
    let mut last_spec_index = 0;

    for (idx, variant) in e.variants.items().enumerate() {
        let fields = match analyze(&variant.fields) {
            Ok(v) => v,
            Err(msg) => return err(&msg),
        };
        let plans = &fields.args;
        let sub = &fields.sub;
        let vattr = match attr::pound(&variant.attributes) {
            Ok(attributes) => attributes,
            Err(e) => return err(&e),
        };
        let vname = &variant.name;
        let sub_name = vattr
            .name
            .clone()
            .unwrap_or_else(|| camel_to_kebab(&vname.to_string()));
        let variant_doc = attr::doc(&variant.attributes);
        let sub_about = help_lit(attr::summary(&variant_doc));
        let (about_call, long_about_call) = about_calls(&variant_doc);
        let hidden = vattr.hidden;

        if let Err(e) = vattr.allow_only(VARIANT_ATTRIBUTES) {
            return err(&e);
        }
        if let Err(msg) =
            validate_fields(plans, &vattr.required_groups, !fields.flattened.is_empty())
        {
            return err(&msg);
        }
        let conflicts = match conflict_pairs(plans) {
            Ok(c) => c,
            Err(msg) => return err(&msg),
        };
        let requires = match require_pairs(plans) {
            Ok(r) => index_pairs(&r),
            Err(msg) => return err(&msg),
        };
        let args = plans.iter().map(arg_expr);
        let default_asserts = plans.iter().filter_map(default_assert);
        let flatten_asserts = fields.flatten_asserts();
        let groups = group_exprs(plans, &vattr.required_groups);
        let conflicts = index_pairs(&conflicts);
        let (subs, sub_optional) = sub_parts(sub.as_ref());
        let ak = format_ident!("ARGS{}", idx);
        let gk = format_ident!("GROUPS{}", idx);
        let xk = format_ident!("CONFLICTS{}", idx);
        let rk = format_ident!("REQUIRES{}", idx);
        let ck = format_ident!("CMD{}", idx);
        let flatten_calls = fields.flatten_calls();
        let unique_message = format!(
            "pound: two args of `{name}::{}` answer to the same spelling",
            variant.name
        );
        let group_asserts = group_asserts(plans, &vattr.required_groups, &ck);
        // parameterless builders, so only chain them when the flag is set.
        let sub_optional_call = if sub_optional {
            quote!(.sub_optional())
        } else {
            quote!()
        };
        let hidden_call = if hidden { quote!(.hidden()) } else { quote!() };
        sub_consts.push(quote! {
            #(#default_asserts)*
            #flatten_asserts
            const #ak: &[::pound::ArgSpec] = &[ #(#args),* ];
            const #gk: &[::pound::GroupSpec] = &[ #(#groups),* ];
            const #xk: &[(usize, usize)] = #conflicts;
            const #rk: &[(usize, usize)] = #requires;
            const #ck: ::pound::CommandSpec = ::pound::CommandSpec::new(#sub_name)
                #about_call
                #long_about_call
                .args(#ak)
                #flatten_calls
                .groups(#gk)
                .conflicts(#xk)
                .requires(#rk)
                .subs(#subs)
                #sub_optional_call;
            const _: () = ::core::assert!(::pound::names_unique(&#ck), #unique_message);
            #group_asserts
        });
        let valias = &vattr.aliases;
        sub_specs.push(quote! {
            ::pound::SubSpec::new(#sub_name, &#ck)
                .aliases(&[ #(#valias),* ])
                .about(#sub_about)
                #hidden_call
        });

        let m = quote!(__sm);
        let sp = quote!(__s);
        arms.push(if plans.is_empty() && fields.flattened.is_empty() && sub.is_none() {
            quote! { ::core::option::Option::Some((#idx, _)) => ::core::result::Result::Ok(Self::#vname), }
        } else {
            let readers = plans.iter().enumerate().map(|(i, p)| reader(p, i, &m, &sp));
            let flattened_readers =
                fields.flattened.iter().enumerate().map(|(i, f)| f.reader(i, &m));
            let sub_r = sub.as_ref().map(|sf| sub_reader(sf, &m));
            let bind = if plans.is_empty() {
                quote! {}
            } else {
                uses_spec = true;
                indexed_variants += 1;
                last_spec_index = idx;
                quote! { let __s = spec.subs[#idx].spec; }
            };
            quote! {
                ::core::option::Option::Some((#idx, __sm)) => {
                    #bind
                    ::core::result::Result::Ok(Self::#vname {
                        #(#readers,)* #(#flattened_readers,)* #sub_r
                    })
                },
            }
        });
    }

    // `spec` is only read when some variant has its own args
    let spec_param = if uses_spec {
        quote!(spec)
    } else {
        quote!(_spec)
    };
    let spec_assert = if indexed_variants > 1 {
        quote! { assert!(spec.subs.len() > #last_spec_index); }
    } else {
        quote! {}
    };

    quote! {
        impl ::pound::Parse for #name {
            const SPEC: &'static ::pound::CommandSpec = {
                #(#sub_consts)*
                const SUBS: &[::pound::SubSpec] = &[ #(#sub_specs),* ];
                const ROOT: ::pound::CommandSpec = ::pound::CommandSpec::new(#name_expr)
                    .version(#version_expr)
                    #hash_call
                    #about
                    #long_about
                    .subs(SUBS);
                &ROOT
            };

            fn from_matches(#spec_param: &'static ::pound::CommandSpec, m: &::pound::Matches)
                -> ::core::result::Result<Self, ::pound::Error>
            {
                #spec_assert
                match ::pound::Matches::sub(m) {
                    #(#arms)*
                    _ => ::core::result::Result::Err(::pound::ErrorKind::MissingSubcommand.into()),
                }
            }
        }
    }
    .into()
}

fn value_enum(e: &venial::Enum) -> TokenStream {
    let item = match attr::pound(&e.attributes) {
        Ok(item) => item,
        Err(e) => return err(&e),
    };
    if let Err(e) = item.allow_only(&[]) {
        return err(&e);
    }
    let name = &e.name;
    let mut names = Vec::new();
    let mut arms = Vec::new();
    for (variant, _) in &e.variants.inner {
        if !matches!(variant.fields, Fields::Unit) {
            return err("pound: ValueEnum needs unit variants only");
        }
        let vname = &variant.name;
        let vattr = match attr::pound(&variant.attributes) {
            Ok(attributes) => attributes,
            Err(e) => return err(&e),
        };
        if let Err(e) = vattr.allow_only(VALUE_VARIANT_ATTRIBUTES) {
            return err(&e);
        }
        let label = vattr
            .name
            .unwrap_or_else(|| camel_to_kebab(&vname.to_string()));
        arms.push(quote! { #label => ::core::result::Result::Ok(Self::#vname), });
        names.push(label);
    }

    quote! {
        impl ::pound::FromArg for #name {
            const POSSIBLE: ::core::option::Option<&'static [&'static str]> =
                ::core::option::Option::Some(&[ #(#names),* ]);

            fn from_arg(s: &str) -> ::core::result::Result<Self, ::pound::ValueError> {
                match s {
                    #(#arms)*
                    other => ::core::result::Result::Err(
                        ::pound::ValueError::new(other, "unrecognized value")
                    ),
                }
            }
        }
    }
    .into()
}

// --- field planning

// split a struct/variant's fields into regular args, flattened structs, and an
// optional single `#[pound(subcommand)]` field.
fn analyze(fields: &Fields) -> Result<FieldPlan, String> {
    let mut plan = FieldPlan {
        args:      Vec::new(),
        flattened: Vec::new(),
        order:     Vec::new(),
        sub:       None,
    };
    let named = match fields {
        Fields::Unit => return Ok(plan),
        Fields::Tuple(_) => {
            return Err("pound: tuple fields are not supported, use named fields".into());
        },
        Fields::Named(named) => named,
    };
    for field in named.fields.items() {
        let attributes = attr::pound(&field.attributes)?;
        if attributes.subcommand {
            attributes.allow_only(&["subcommand"])?;
            if plan.sub.is_some() {
                return Err("pound: only one #[pound(subcommand)] field is allowed".into());
            }
            plan.sub = Some(sub_field(field)?);
        } else if attributes.flatten {
            attributes.allow_only(&["flatten"])?;
            let (is_bool, card, ty) = classify(&field.ty);
            if is_bool || card != Card::One {
                return Err(format!(
                    "pound: #[pound(flatten)] needs a plain `Parse` type (`{}`)",
                    field.name
                ));
            }
            plan.order.push(FieldOrder::Flattened(plan.flattened.len()));
            plan.flattened.push(FlattenField {
                ident: field.name.clone(),
                ty,
            });
        } else {
            attributes.allow_only(FIELD_ATTRIBUTES)?;
            plan.order.push(FieldOrder::Direct(plan.args.len()));
            plan.args.push(plan_field(field, attributes)?);
        }
    }
    Ok(plan)
}

fn command_attributes(attributes: &[venial::Attribute]) -> Result<Pound, String> {
    let item = attr::pound(attributes)?;
    item.allow_only(ITEM_ATTRIBUTES)?;
    Ok(item)
}

fn sub_field(field: &NamedField) -> Result<SubField, String> {
    let (is_bool, card, inner) = classify(&field.ty);
    if is_bool || card == Card::Many {
        return Err("pound: #[pound(subcommand)] must be `T` or `Option<T>`".into());
    }
    Ok(SubField {
        ident:    field.name.clone(),
        ty:       inner,
        optional: card == Card::Opt,
    })
}

// the `subs` expression and `sub_optional` flag for a command, given its
// optional subcommand field.
fn sub_parts(sub: Option<&SubField>) -> (TokenStream2, bool) {
    match sub {
        Some(sf) => {
            let ty = &sf.ty;
            (quote! { <#ty as ::pound::Parse>::SPEC.subs }, sf.optional)
        },
        None => (quote! { &[] }, false),
    }
}

// the `field: <built subcommand>` reader for a subcommand field.
fn sub_reader(sf: &SubField, m: &TokenStream2) -> TokenStream2 {
    let ident = &sf.ident;
    let ty = &sf.ty;
    let build =
        quote! { <#ty as ::pound::Parse>::from_matches(<#ty as ::pound::Parse>::SPEC, #m)? };
    if sf.optional {
        quote! {
            #ident: if ::pound::Matches::sub(#m).is_some() {
                ::core::option::Option::Some(#build)
            } else {
                ::core::option::Option::None
            }
        }
    } else {
        quote! { #ident: #build }
    }
}

fn negation(
    a: &Pound,
    kind: ArgKind,
    long: Option<&str>,
    ident: &proc_macro2::Ident,
) -> Result<Option<String>, String> {
    let Some(spelling) = &a.negate else {
        return Ok(None);
    };
    if kind != ArgKind::Flag {
        return Err(format!(
            "pound: #[pound(negate)] needs a bool field (`{ident}`)"
        ));
    }
    let Some(name) = long else {
        return Err(format!("pound: #[pound(negate)] needs a long name (`{ident}`)"));
    };
    let spelling = spelling.clone().unwrap_or_else(|| format!("no-{name}"));
    if spelling == name || a.aliases.contains(&spelling) {
        return Err(format!(
            "pound: negation `{spelling}` is also a positive spelling (`{ident}`)"
        ));
    }
    Ok(Some(spelling))
}

// the `min_values`/`max_values` bounds, which only a `Vec` field can satisfy.
fn arity(
    a: &Pound,
    multi: bool,
    ident: &proc_macro2::Ident,
) -> Result<(Option<usize>, Option<usize>), String> {
    let read = |name: &str, raw: Option<&String>| match raw {
        None => Ok(None),
        Some(_) if !multi => Err(format!(
            "pound: #[pound({name})] needs a `Vec` field (`{ident}`)"
        )),
        Some(text) => text
            .parse::<usize>()
            .map(Some)
            .map_err(|_| format!("pound: #[pound({name} = {text})] is not a count (`{ident}`)")),
    };

    let min = read("min_values", a.min_values.as_ref())?;
    let max = read("max_values", a.max_values.as_ref())?;
    if let (Some(min), Some(max)) = (min, max)
        && min > max
    {
        return Err(format!(
            "pound: min_values {min} exceeds max_values {max} (`{ident}`)"
        ));
    }
    Ok((min, max))
}

fn check_kind(a: &Pound, kind: ArgKind, ident: &proc_macro2::Ident) -> Result<(), String> {
    if a.heading.is_some() && !kind.is_named() {
        return Err(format!(
            "pound: #[pound(heading)] needs a flag or option, positionals are listed under \
             Arguments (`{ident}`)"
        ));
    }
    if a.default_missing.is_some() && kind != ArgKind::Opt {
        return Err(format!(
            "pound: #[pound(default_missing)] needs a value option (short/long) (`{ident}`)"
        ));
    }
    if !kind.takes_value()
        && let Some(name) = [
            ("min", a.min.is_some()),
            ("max", a.max.is_some()),
            ("max_len", a.max_len.is_some()),
            ("parse", a.parse.is_some()),
            ("validate", a.validate.is_some()),
        ]
        .into_iter()
        .find_map(|(name, present)| present.then_some(name))
    {
        return Err(format!(
            "pound: #[pound({name})] needs a value field (`{ident}`)"
        ));
    }
    Ok(())
}

fn field_kind(
    a: &Pound,
    is_bool: bool,
    card: Card,
    ident: &proc_macro2::Ident,
) -> Result<ArgKind, String> {
    let shape_count = usize::from(a.positional) + usize::from(a.trailing) + usize::from(a.count);
    if shape_count > 1 {
        return Err(format!(
            "pound: positional, trailing, and count are mutually exclusive (`{ident}`)"
        ));
    }
    if is_bool && shape_count > 0 {
        return Err(format!(
            "pound: bool fields cannot be positional, trailing, or counted (`{ident}`)"
        ));
    }
    if (a.positional || a.trailing) && a.is_named() {
        return Err(format!(
            "pound: positional fields cannot have short or long names (`{ident}`)"
        ));
    }
    if a.trailing && card != Card::Many {
        return Err(format!(
            "pound: #[pound(trailing)] needs a `Vec` field (`{ident}`)"
        ));
    }
    if a.count && card != Card::One {
        return Err(format!(
            "pound: #[pound(count)] needs a scalar field (`{ident}`)"
        ));
    }

    let kind = if is_bool {
        ArgKind::Flag
    } else if a.count {
        ArgKind::Count
    } else if a.trailing {
        ArgKind::Trailing
    } else if a.is_named() {
        ArgKind::Opt
    } else {
        ArgKind::Positional
    };
    if !kind.is_named() && !a.aliases.is_empty() {
        return Err(format!("pound: aliases need a flag or option (`{ident}`)"));
    }
    if kind == ArgKind::Count && (a.default.is_some() || a.env.is_some()) {
        return Err(format!(
            "pound: counted fields cannot use a fallback (`{ident}`)"
        ));
    }
    Ok(kind)
}

fn plan_field(field: &NamedField, a: Pound) -> Result<Plan, String> {
    let (is_bool, card, inner_ty) = classify(&field.ty);
    let full_ty: TokenStream2 = field.ty.tokens.iter().cloned().collect();
    let fname = field.name.to_string();
    let kind = field_kind(&a, is_bool, card, &field.name)?;

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

    let doc = a.help.clone().unwrap_or_else(|| attr::doc(&field.attributes));
    let help = attr::summary(&doc).to_owned();
    let long_help = a.long_help.clone().or_else(|| (help != doc).then(|| doc.clone()));

    check_kind(&a, kind, &field.name)?;
    let (min_values, max_values) = arity(&a, card == Card::Many, &field.name)?;
    let negate = negation(&a, kind, long.as_deref(), &field.name)?;

    let required = kind.takes_value() && card == Card::One && a.default.is_none();
    let conversion = kind
        .takes_value()
        .then(|| conversion_for(&a, &field.name))
        .transpose()?;

    Ok(Plan {
        ident: field.name.clone(),
        kind,
        long,
        short,
        required,
        multi: card == Card::Many,
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
        card,
        conversion,
        inner_ty,
        full_ty,
    })
}

fn conversion_for(a: &Pound, ident: &proc_macro2::Ident) -> Result<Conversion, String> {
    if let Some(parse) = &a.parse {
        if a.min.is_some() || a.max.is_some() || a.max_len.is_some() {
            return Err(format!(
                "pound: #[pound(parse = \"...\")] cannot be combined with min, max, or max_len \
                 (`{ident}`); use validate instead"
            ));
        }
        return Ok(Conversion::CustomParse {
            parse:    parse.clone(),
            validate: a.validate.clone(),
        });
    }
    if a.min.is_some() || a.max.is_some() || a.max_len.is_some() || a.validate.is_some() {
        return Ok(Conversion::CheckedFromArg {
            min:      a.min.clone(),
            max:      a.max.clone(),
            max_len:  a.max_len.clone(),
            validate: a.validate.clone(),
        });
    }
    Ok(Conversion::FromArg)
}

// (is_bool, cardinality, inner type to parse with FromArg)
fn classify(ty: &TypeExpr) -> (bool, Card, TokenStream2) {
    let toks = &ty.tokens;
    let s: String = toks.iter().map(ToString::to_string).collect();
    if s == "bool" {
        return (true, Card::One, quote!(bool));
    }
    if let Some(inner) = strip_wrapper(toks, "Option") {
        return (false, Card::Opt, inner);
    }
    if let Some(inner) = strip_wrapper(toks, "Vec") {
        return (false, Card::Many, inner);
    }
    (false, Card::One, toks.iter().cloned().collect())
}

// `Wrapper < Inner >` -> the `Inner` tokens.
fn strip_wrapper(toks: &[TokenTree], wrapper: &str) -> Option<TokenStream2> {
    let head_ok = matches!(toks.first(), Some(TokenTree::Ident(id)) if *id == wrapper);
    let open_ok = matches!(toks.get(1), Some(TokenTree::Punct(p)) if p.as_char() == '<');
    let close_ok = matches!(toks.last(), Some(TokenTree::Punct(p)) if p.as_char() == '>');
    if toks.len() >= 4 && head_ok && open_ok && close_ok {
        Some(toks[2..toks.len() - 1].iter().cloned().collect())
    } else {
        None
    }
}

// --- emit helpers

// a const assertion that a declared default is one of its type's values. only
// a `FromArg` conversion exposes a value list, so a custom parser gets none.
fn default_assert(p: &Plan) -> Option<TokenStream2> {
    let default = p.default.as_ref()?;
    if !matches!(
        p.conversion,
        Some(Conversion::FromArg | Conversion::CheckedFromArg { .. })
    ) {
        return None;
    }
    let inner = &p.inner_ty;
    let message = format!(
        "pound: default \"{default}\" is not one of the possible values for `{}`",
        p.ident
    );
    Some(quote! {
        const _: () = ::core::assert!(
            ::pound::default_allowed(#default, <#inner as ::pound::FromArg>::POSSIBLE),
            #message
        );
    })
}

fn arg_expr(p: &Plan) -> TokenStream2 {
    let kind = match p.kind {
        ArgKind::Flag => quote! { ::pound::Kind::Flag },
        ArgKind::Count => quote! { ::pound::Kind::Count },
        ArgKind::Opt => quote! { ::pound::Kind::Opt },
        ArgKind::Positional => quote! { ::pound::Kind::Positional },
        ArgKind::Trailing => quote! { ::pound::Kind::Trailing },
    };
    let mut e = quote! { ::pound::ArgSpec::new(#kind) };
    if let Some(l) = &p.long {
        e = quote! { #e.long(#l) };
    }
    if let Some(c) = p.short {
        e = quote! { #e.short(#c) };
    }
    if p.required {
        e = quote! { #e.required() };
    }
    if p.multi {
        e = quote! { #e.multi() };
    }
    if let Some(n) = p.min_values {
        e = quote! { #e.min_values(#n) };
    }
    if let Some(n) = p.max_values {
        e = quote! { #e.max_values(#n) };
    }
    if let Some(g) = &p.group {
        e = quote! { #e.group(#g) };
    }
    if let Some(d) = &p.default {
        e = quote! { #e.default(#d) };
    }
    if let Some(dm) = &p.default_missing {
        e = quote! { #e.default_missing(#dm) };
    }
    if let Some(ev) = &p.env {
        e = quote! { #e.env(#ev) };
    }
    if let Some(n) = &p.negate {
        e = quote! { #e.negate(#n) };
    }
    if !p.aliases.is_empty() {
        let al = &p.aliases;
        e = quote! { #e.aliases(&[ #(#al),* ]) };
    }
    if let Some(h) = &p.heading {
        e = quote! { #e.heading(#h) };
    }
    let vn = &p.value_name;
    e = quote! { #e.value_name(#vn) };
    // valued kinds pull a possible-value list from a value enum (None
    // otherwise). custom parsers may target types without FromArg, so they
    // cannot expose one.
    if matches!(
        &p.conversion,
        Some(Conversion::FromArg | Conversion::CheckedFromArg { .. })
    ) {
        let inner = &p.inner_ty;
        e = quote! { #e.possible_opt(<#inner as ::pound::FromArg>::POSSIBLE) };
    }
    if p.hidden {
        e = quote! { #e.hidden() };
    }
    if p.global {
        e = quote! { #e.global() };
    }
    if let Some(lh) = &p.long_help {
        let lh = help_lit(lh);
        e = quote! { #e.long_help(#lh) };
    }
    let help = help_lit(&p.help);
    quote! { #e.help(#help) }
}

fn reader(p: &Plan, i: usize, m: &TokenStream2, spec: &TokenStream2) -> TokenStream2 {
    let fname = &p.ident;
    let body = match p.kind {
        ArgKind::Flag => quote! { #m.switch(#spec, #i) },
        ArgKind::Count => {
            let ty = &p.full_ty;
            quote! { #m.count(#i) as #ty }
        },
        _ => {
            let inner = &p.inner_ty;
            if let Some(conversion) = &p.conversion {
                let convert = conversion_closure(conversion, inner);
                match p.card {
                    Card::One => quote! { #m.required_map::<#inner>(#spec, #i, #convert)? },
                    Card::Opt => quote! { #m.optional_map::<#inner>(#spec, #i, #convert)? },
                    Card::Many => quote! { #m.many_map::<#inner>(#spec, #i, #convert)? },
                }
            } else {
                match p.card {
                    Card::One => quote! { #m.required::<#inner>(#spec, #i)? },
                    Card::Opt => quote! { #m.optional::<#inner>(#spec, #i)? },
                    Card::Many => quote! { #m.many::<#inner>(#spec, #i)? },
                }
            }
        },
    };
    quote! { #fname: #body }
}

fn conversion_closure(conversion: &Conversion, inner: &TokenStream2) -> TokenStream2 {
    let parse = parse_value_expr(conversion, inner);
    let max_len = match conversion {
        Conversion::CheckedFromArg { max_len, .. } => max_len.as_deref().map(max_len_check),
        Conversion::FromArg | Conversion::CustomParse { .. } => None,
    };
    let min = match conversion {
        Conversion::CheckedFromArg { min, .. } => {
            min.as_deref().map(|v| bound_check(inner, "min", v))
        },
        Conversion::FromArg | Conversion::CustomParse { .. } => None,
    };
    let max = match conversion {
        Conversion::CheckedFromArg { max, .. } => {
            max.as_deref().map(|v| bound_check(inner, "max", v))
        },
        Conversion::FromArg | Conversion::CustomParse { .. } => None,
    };
    let validate = match conversion {
        Conversion::CheckedFromArg { validate, .. } | Conversion::CustomParse { validate, .. } => {
            validate.as_deref().map(validate_check)
        },
        Conversion::FromArg => None,
    };
    quote! {
        |__s: &str| -> ::core::result::Result<#inner, ::pound::ValueError> {
            #max_len
            let __value = #parse;
            #min
            #max
            #validate
            ::core::result::Result::Ok(__value)
        }
    }
}

fn parse_value_expr(conversion: &Conversion, inner: &TokenStream2) -> TokenStream2 {
    match conversion {
        Conversion::FromArg | Conversion::CheckedFromArg { .. } => {
            quote! { <#inner as ::pound::FromArg>::from_arg(__s)? }
        },
        Conversion::CustomParse { parse, .. } => {
            if let Ok(path) = TokenStream2::from_str(parse) {
                quote! {
                    #path(__s).map_err(|__msg| ::pound::ValueError::new(__s, __msg))?
                }
            } else {
                quote! { ::core::compile_error!("pound: invalid parse function path") }
            }
        },
    }
}

fn max_len_check(value: &str) -> TokenStream2 {
    if let Ok(max) = TokenStream2::from_str(value) {
        quote! {
            if __s.chars().count() > (#max) {
                return ::core::result::Result::Err(::pound::ValueError::new(
                    __s,
                    ::core::concat!("must be at most ", ::core::stringify!(#max), " chars"),
                ));
            }
        }
    } else {
        quote! { ::core::compile_error!("pound: invalid max_len bound") }
    }
}

fn bound_check(inner: &TokenStream2, kind: &str, value: &str) -> TokenStream2 {
    let op = if kind == "min" { quote!(<) } else { quote!(>) };
    let msg = if kind == "min" {
        quote! { ::core::concat!("must be at least ", #value) }
    } else {
        quote! { ::core::concat!("must be at most ", #value) }
    };
    let invalid = if kind == "min" {
        quote! { ::core::concat!("invalid min: ", #value) }
    } else {
        quote! { ::core::concat!("invalid max: ", #value) }
    };
    quote! {
        let __bound = <#inner as ::pound::FromArg>::from_arg(#value)
            .map_err(|_| ::pound::ValueError::new(__s, #invalid))?;
        if __value #op __bound {
            return ::core::result::Result::Err(::pound::ValueError::new(__s, #msg));
        }
    }
}

fn validate_check(value: &str) -> TokenStream2 {
    if let Ok(path) = TokenStream2::from_str(value) {
        quote! {
            if let ::core::result::Result::Err(__msg) = #path(&__value) {
                return ::core::result::Result::Err(::pound::ValueError::new(__s, __msg));
            }
        }
    } else {
        quote! { ::core::compile_error!("pound: invalid validate function path") }
    }
}

// distinct group names in first-seen order, marked required where listed.
fn group_exprs(plans: &[Plan], required: &[String]) -> Vec<TokenStream2> {
    let mut seen = Vec::new();
    for p in plans {
        if let Some(g) = &p.group
            && !seen.contains(g)
        {
            seen.push(g.clone());
        }
    }
    for g in required {
        if !seen.contains(g) {
            seen.push(g.clone());
        }
    }
    seen.into_iter()
        .map(|g| {
            let base = quote! { ::pound::GroupSpec::new(#g) };
            if required.contains(&g) {
                quote! { #base.required() }
            } else {
                base
            }
        })
        .collect()
}

// `global` only makes sense on a named flag/option, never a positional.
fn validate_fields(
    plans: &[Plan],
    required_groups: &[String],
    has_flattened: bool,
) -> Result<(), String> {
    for p in plans {
        if p.global && !p.kind.is_named() {
            return Err(format!(
                "pound: #[pound(global)] needs a flag or option (short/long), not a positional \
                 (`{}`)",
                p.ident
            ));
        }
    }
    if !has_flattened
        && let Some(name) = required_groups
            .iter()
            .find(|name| !has_direct_member(plans, name))
    {
        return Err(format!("pound: required group `{name}` has no members"));
    }
    Ok(())
}

fn has_direct_member(plans: &[Plan], group: &str) -> bool {
    plans
        .iter()
        .any(|plan| plan.group.as_deref() == Some(group))
}

/// required groups whose members can only sit in flattened structs, checked
/// once the whole spec exists
fn group_asserts(
    plans: &[Plan],
    required_groups: &[String],
    spec: &proc_macro2::Ident,
) -> TokenStream2 {
    required_groups
        .iter()
        .filter(|name| !has_direct_member(plans, name))
        .map(|name| {
            let message = format!("pound: required group `{name}` has no members");
            quote! {
                const _: () = ::core::assert!(::pound::group_has_members(&#spec, #name), #message);
            }
        })
        .collect()
}

// resolve field-level requires names to index pairs. direction carries meaning
// here, so unlike conflicts these are not normalised.
fn require_pairs(plans: &[Plan]) -> Result<Vec<(usize, usize)>, String> {
    let index: HashMap<String, usize> = plans
        .iter()
        .enumerate()
        .map(|(i, p)| (p.ident.to_string(), i))
        .collect();
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for (i, p) in plans.iter().enumerate() {
        for name in &p.requires {
            let j = *index
                .get(name)
                .ok_or_else(|| format!("pound: requires: no field named `{name}`"))?;
            if i != j && !pairs.contains(&(i, j)) {
                pairs.push((i, j));
            }
        }
    }
    Ok(pairs)
}

// resolve field-level conflicts_with names to normalised, deduped index pairs.
fn conflict_pairs(plans: &[Plan]) -> Result<Vec<(usize, usize)>, String> {
    let index: HashMap<String, usize> = plans
        .iter()
        .enumerate()
        .map(|(i, p)| (p.ident.to_string(), i))
        .collect();
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for (i, p) in plans.iter().enumerate() {
        for name in &p.conflicts_with {
            let j = *index
                .get(name)
                .ok_or_else(|| format!("pound: conflicts_with: no field named `{name}`"))?;
            if i == j {
                continue;
            }
            let pair = if i < j { (i, j) } else { (j, i) };
            if !pairs.contains(&pair) {
                pairs.push(pair);
            }
        }
    }
    Ok(pairs)
}

// the `&[(usize, usize)]` token list for a set of index pairs.
fn index_pairs(pairs: &[(usize, usize)]) -> TokenStream2 {
    let items = pairs.iter().map(|(a, b)| quote! { (#a, #b) });
    quote! { &[ #(#items),* ] }
}

fn name_expr(item: &Pound) -> TokenStream2 {
    item.name.as_ref().map_or_else(
        || quote! { ::core::env!("CARGO_PKG_NAME") },
        |n| quote! { #n },
    )
}

fn version_expr(item: &Pound) -> TokenStream2 {
    item.version.as_ref().map_or_else(
        || quote! { ::core::env!("CARGO_PKG_VERSION") },
        |tokens| tokens.iter().cloned().collect::<TokenStream2>(),
    )
}

fn git_hash_call() -> TokenStream2 {
    git_hash().map_or_else(TokenStream2::new, |hash| quote! { .hash(#hash) })
}

fn git_hash() -> Option<String> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").ok()?;
    let output = Command::new("git")
        .args(["-C", &manifest_dir, "rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let hash = String::from_utf8(output.stdout).ok()?;
    let hash = hash.trim();
    (!hash.is_empty() && hash.chars().all(|ch| ch.is_ascii_hexdigit())).then(|| hash.to_owned())
}

// bake the help string only when the feature is on, otherwise emit "".
// the `.about()` / `.long_about()` calls for a doc comment. the long form is
// chained only when it says more than the summary already does.
fn about_calls(doc: &str) -> (TokenStream2, TokenStream2) {
    let summary = help_lit(attr::summary(doc));
    let about = quote! { .about(#summary) };
    if attr::summary(doc) == doc {
        return (about, quote!());
    }
    let full = help_lit(doc);
    (about, quote! { .long_about(#full) })
}

// bake the help string only when the feature is on, otherwise emit "".
fn help_lit(s: &str) -> TokenStream2 {
    #[cfg(feature = "help")]
    {
        quote! { #s }
    }
    #[cfg(not(feature = "help"))]
    {
        let _ = s;
        quote! { "" }
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

fn err(msg: &str) -> TokenStream {
    quote! { ::core::compile_error!(#msg); }.into()
}
