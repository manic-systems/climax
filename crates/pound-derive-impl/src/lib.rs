// SPDX-License-Identifier: EUPL-1.2

#![warn(missing_docs)]

//! expansion behind pound's derive macros, as a plain library so more than one
//! proc-macro crate can root the generated code at its own path to the pound
//! runtime. `derive_parse` turns a struct into a flat command and an enum into
//! a subcommand tree, `derive_value_enum` wires a unit enum up as a `FromArg`
//! choice type. all of it just emits the static `CommandSpec` plus a
//! `from_matches` reader, the runtime does the work.

mod attr;

use std::{
    collections::HashMap,
    str::FromStr,
};

use proc_macro2::{
    Ident,
    Span,
    TokenStream as TokenStream2,
    TokenTree,
};
use quote::quote;
use venial::{
    Fields,
    Item,
    NamedField,
    TypeExpr,
    parse_item,
};

use crate::attr::{Pound, Text};

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

/// How the expansion refers to the runtime crate and whether it bakes help
/// text, decided by the proc-macro crate that wraps this library.
#[non_exhaustive]
pub struct Options {
    /// Path to the `pound` runtime as seen from the deriving crate, such as
    /// `::pound`.
    pub root: &'static str,
    /// Bake doc comments and `#[pound(heading)]` into the expansion.
    pub help: bool,
}

impl Options {
    /// options rooting the expansion at `root`, with doc comments baked in
    /// when `help` is set
    #[must_use]
    pub const fn new(root: &'static str, help: bool) -> Self {
        Self { root, help }
    }
}

struct Ctx {
    root: TokenStream2,
    help: bool,
}

impl Ctx {
    fn new(options: &Options) -> Self {
        Self {
            root: TokenStream2::from_str(options.root).expect("Options::root is a path"),
            help: options.help,
        }
    }
}

/// expands `#[derive(Parse)]` for the struct or enum in `input`, or yields a
/// `compile_error!` naming the problem. the attributes it reads are the ones
/// documented on `pound_derive::Parse`.
///
/// a crate that re-exports pound wraps it in its own proc-macro, so the
/// generated code names that crate's path to the runtime.
///
/// ```
/// use pound_derive_impl::{derive_parse, Options};
///
/// // the body of `#[proc_macro_derive(Parse, attributes(pound))]` in a crate
/// // that re-exports pound as `climax::pound`
/// const OPTIONS: Options = Options::new("::climax::pound", true);
///
/// let input = "struct Args { #[pound(long)] verbose: bool }".parse().unwrap();
/// let output = derive_parse(input, &OPTIONS).to_string();
/// assert!(output.contains(":: climax :: pound :: Parse for Args"));
/// ```
pub fn derive_parse(input: TokenStream2, options: &Options) -> TokenStream2 {
    let cx = &Ctx::new(options);
    match parse_item(input) {
        Ok(Item::Struct(s)) => parse_struct(cx, &s),
        Ok(Item::Enum(e)) => parse_enum(cx, &e),
        Ok(_) => err("pound: Parse can only derive on a struct or enum"),
        Err(e) => e.to_compile_error(),
    }
}

/// expands `#[derive(ValueEnum)]` for the unit-variant enum in `input`, or
/// yields a `compile_error!` naming the problem. see [`derive_parse`] for how a
/// re-exporting crate wraps it.
///
/// ```
/// use pound_derive_impl::{derive_value_enum, Options};
///
/// let input = "enum Format { Json, PlainText }".parse().unwrap();
/// let output = derive_value_enum(input, &Options::new("::pound", true)).to_string();
/// assert!(output.contains("\"plain-text\""));
/// ```
pub fn derive_value_enum(input: TokenStream2, options: &Options) -> TokenStream2 {
    let cx = &Ctx::new(options);
    match parse_item(input) {
        Ok(Item::Enum(e)) => value_enum(cx, &e),
        Ok(_) => err("pound: ValueEnum can only derive on an enum"),
        Err(e) => e.to_compile_error(),
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
        validate: Option<TokenStream2>,
    },
    CustomParse {
        parse:    TokenStream2,
        validate: Option<TokenStream2>,
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
    default:         Option<Text>,
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
    fn reader(&self, cx: &Ctx, i: usize, m: &Ident) -> TokenStream2 {
        let root = &cx.root;
        let Self { ident, ty } = self;
        quote! {
            #ident: <#ty as #root::Parse>::from_matches(
                <#ty as #root::Parse>::SPEC,
                #root::Matches::flattened(#m, #i),
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
    /// a flattened struct may bring its own subcommand field, and only one of
    /// those can be selected at a level
    fn selector_assert(&self, cx: &Ctx, owner: &str, spec: &proc_macro2::Ident) -> TokenStream2 {
        let root = &cx.root;
        if self.flattened.is_empty() {
            return TokenStream2::new();
        }
        let message = format!("pound: `{owner}` has more than one subcommand field");
        quote! {
            const _: () = ::core::assert!(#root::checks::selector_count(&#spec) <= 1, #message);
        }
    }

    /// the builder calls that embed the flattened fields, absent when there
    /// are none so plain commands keep the default order
    fn flatten_calls(&self, cx: &Ctx) -> TokenStream2 {
        let root = &cx.root;
        if self.flattened.is_empty() {
            return TokenStream2::new();
        }
        let specs = self.flattened.iter().map(|field| {
            let ty = &field.ty;
            quote! { <#ty as #root::Parse>::SPEC }
        });
        let order = self.order.iter().map(|entry| {
            match entry {
                FieldOrder::Direct(i) => quote! { #root::ArgumentOrder::Direct(#i) },
                FieldOrder::Flattened(i) => quote! { #root::ArgumentOrder::Flattened(#i) },
            }
        });
        quote! {
            .flattened(&[ #(#specs),* ])
            .argument_order(&[ #(#order),* ])
        }
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one cohesive codegen pass reads best whole"
)]
fn parse_struct(cx: &Ctx, s: &venial::Struct) -> TokenStream2 {
    let root = &cx.root;
    let fields = match analyze(&s.fields) {
        Ok(v) => v,
        Err(e) => return err(&e),
    };
    let item = match command_attributes(&s.attributes, &s.name) {
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
    let args = plans.iter().map(|p| arg_expr(cx, p));
    let default_asserts = plans.iter().filter_map(|p| default_assert(cx, p));
    let groups = group_exprs(cx, plans, &item.required_groups);
    let conflicts = index_pairs(&conflicts);
    let (subs, sub_optional) = sub_parts(cx, sub.as_ref());
    let name_expr = name_expr(&item);
    let version_expr = version_expr(&item);
    let hash_call = git_hash_call();
    let (about, long_about) = about_calls(cx, &attr::doc(&s.attributes));

    let m = local("m");
    let sp = local("spec");
    let (args_id, groups_id, conflicts_id, requires_id, cmd_id) = (
        generated("ARGS"),
        generated("GROUPS"),
        generated("CONFLICTS"),
        generated("REQUIRES"),
        generated("CMD"),
    );
    let readers = plans.iter().enumerate().map(|(i, p)| reader(cx, p, i, &m, &sp));
    let flattened_readers = fields
        .flattened
        .iter()
        .enumerate()
        .map(|(i, f)| f.reader(cx, i, &m));
    let sub_reader = sub.as_ref().map(|sf| sub_reader(cx, sf, &m));
    let flatten_calls = fields.flatten_calls(cx);
    let unique_message = format!("pound: two args of `{name}` answer to the same spelling");
    let group_asserts = group_asserts(cx, plans, &item.required_groups, &cmd_id);
    let selector_assert = fields.selector_assert(cx, &name.to_string(), &cmd_id);
    let positional_assert = positional_assert(cx, &name.to_string(), &cmd_id);

    // avoid unused-param warnings when a command carries only a subcommand.
    let spec_param = if plans.is_empty() {
        local("_spec")
    } else {
        sp.clone()
    };
    let m_param = if plans.is_empty() && fields.flattened.is_empty() && sub.is_none() {
        local("_m")
    } else {
        m.clone()
    };

    // parameterless builder, so only chain it when the flag is actually set.
    let sub_optional_call = if sub_optional {
        quote!(.sub_optional())
    } else {
        quote!()
    };

    quote! {
        impl #root::Parse for #name {
            const SPEC: &'static #root::CommandSpec = {
                #(#default_asserts)*
                const #args_id: &[#root::ArgSpec] = &[ #(#args),* ];
                const #groups_id: &[#root::GroupSpec] = &[ #(#groups),* ];
                const #conflicts_id: &[(usize, usize)] = #conflicts;
                const #requires_id: &[(usize, usize)] = #requires;
                const #cmd_id: #root::CommandSpec = #root::CommandSpec::new(#name_expr)
                    .version(#version_expr)
                    #hash_call
                    #about
                    #long_about
                    .args(#args_id)
                    #flatten_calls
                    .groups(#groups_id)
                    .conflicts(#conflicts_id)
                    .requires(#requires_id)
                    .subs(#subs)
                    #sub_optional_call;
                const _: () = ::core::assert!(#root::checks::names_unique(&#cmd_id), #unique_message);
                #group_asserts
                #selector_assert
                #positional_assert
                &#cmd_id
            };

            fn from_matches(#spec_param: &'static #root::CommandSpec, #m_param: &#root::Matches)
                -> ::core::result::Result<Self, #root::Error>
            {
                ::core::result::Result::Ok(Self {
                    #(#readers,)* #(#flattened_readers,)* #sub_reader
                })
            }
        }
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one cohesive codegen pass reads best whole"
)]
fn parse_enum(cx: &Ctx, e: &venial::Enum) -> TokenStream2 {
    let root = &cx.root;
    if e.variants.is_empty() {
        return err("pound: a command enum needs at least one variant");
    }
    let item = match command_attributes(&e.attributes, &e.name) {
        Ok(item) => item,
        Err(e) => return err(&e),
    };
    let name = &e.name;
    let name_expr = name_expr(&item);
    let version_expr = version_expr(&item);
    let hash_call = git_hash_call();
    let (about, long_about) = about_calls(cx, &attr::doc(&e.attributes));

    let (sm, spec_ident, m_ident) = (local("sm"), local("spec"), local("m"));
    let (subs_id, root_id) = (generated("SUBS"), generated("ROOT"));
    let mut sub_consts = Vec::new();
    let mut sub_specs = Vec::new();
    let mut arms = Vec::new();
    let mut uses_spec = false;
    let mut indexed_variants = 0;
    let mut last_spec_index = 0;

    for (idx, variant) in e.variants.items().enumerate() {
        let vattr = match attr::pound(&variant.attributes) {
            Ok(attributes) => attributes,
            Err(e) => return err(&e),
        };
        let vname = &variant.name;
        if vattr.flatten {
            let ty = match vattr
                .allow_only(&["flatten"], vname)
                .and_then(|()| flattened_variant(variant))
            {
                Ok(ty) => ty,
                Err(e) => return err(&e),
            };
            sub_specs.push(quote! {
                #root::SubSpec::flatten(#root::checks::subcommand_spec::<#ty>())
            });
            arms.push(quote! {
                ::core::option::Option::Some((#idx, #sm)) => ::core::result::Result::Ok(
                    Self::#vname(<#ty as #root::Parse>::from_matches(
                        <#ty as #root::Parse>::SPEC,
                        #sm,
                    )?),
                ),
            });
            continue;
        }

        let fields = match analyze(&variant.fields) {
            Ok(v) => v,
            Err(msg) => return err(&msg),
        };
        let plans = &fields.args;
        let sub = &fields.sub;
        let sub_name = vattr
            .name
            .clone()
            .unwrap_or_else(|| camel_to_kebab(&vname.to_string()));
        let variant_doc = attr::doc(&variant.attributes);
        let sub_about = help_lit(cx, attr::short_help(&variant_doc));
        let (about_call, long_about_call) = about_calls(cx, &variant_doc);
        let hidden = vattr.hidden;

        if let Err(e) = vattr.allow_only(VARIANT_ATTRIBUTES, vname) {
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
        let args = plans.iter().map(|p| arg_expr(cx, p));
        let default_asserts = plans.iter().filter_map(|p| default_assert(cx, p));
        let groups = group_exprs(cx, plans, &vattr.required_groups);
        let conflicts = index_pairs(&conflicts);
        let (subs, sub_optional) = sub_parts(cx, sub.as_ref());
        let ak = generated(&format!("ARGS{idx}"));
        let gk = generated(&format!("GROUPS{idx}"));
        let xk = generated(&format!("CONFLICTS{idx}"));
        let rk = generated(&format!("REQUIRES{idx}"));
        let ck = generated(&format!("CMD{idx}"));
        let flatten_calls = fields.flatten_calls(cx);
        let unique_message = format!(
            "pound: two args of `{name}::{}` answer to the same spelling",
            variant.name
        );
        let group_asserts = group_asserts(cx, plans, &vattr.required_groups, &ck);
        let selector_assert = fields.selector_assert(cx, &format!("{name}::{vname}"), &ck);
        let positional_assert = positional_assert(cx, &format!("{name}::{vname}"), &ck);
        // parameterless builders, so only chain them when the flag is set.
        let sub_optional_call = if sub_optional {
            quote!(.sub_optional())
        } else {
            quote!()
        };
        let hidden_call = if hidden { quote!(.hidden()) } else { quote!() };
        sub_consts.push(quote! {
            #(#default_asserts)*
            const #ak: &[#root::ArgSpec] = &[ #(#args),* ];
            const #gk: &[#root::GroupSpec] = &[ #(#groups),* ];
            const #xk: &[(usize, usize)] = #conflicts;
            const #rk: &[(usize, usize)] = #requires;
            const #ck: #root::CommandSpec = #root::CommandSpec::new(#sub_name)
                #about_call
                #long_about_call
                .args(#ak)
                #flatten_calls
                .groups(#gk)
                .conflicts(#xk)
                .requires(#rk)
                .subs(#subs)
                #sub_optional_call;
            const _: () = ::core::assert!(#root::checks::names_unique(&#ck), #unique_message);
            #group_asserts
            #selector_assert
            #positional_assert
        });
        let valias = &vattr.aliases;
        sub_specs.push(quote! {
            #root::SubSpec::new(#sub_name, &#ck)
                .aliases(&[ #(#valias),* ])
                .about(#sub_about)
                #hidden_call
        });

        let m = sm.clone();
        let sp = local("s");
        arms.push(if plans.is_empty() && fields.flattened.is_empty() && sub.is_none() {
            quote! { ::core::option::Option::Some((#idx, _)) => ::core::result::Result::Ok(Self::#vname), }
        } else {
            let readers = plans.iter().enumerate().map(|(i, p)| reader(cx, p, i, &m, &sp));
            let flattened_readers =
                fields.flattened.iter().enumerate().map(|(i, f)| f.reader(cx, i, &m));
            let sub_r = sub.as_ref().map(|sf| sub_reader(cx, sf, &m));
            let bind = if plans.is_empty() {
                quote! {}
            } else {
                uses_spec = true;
                indexed_variants += 1;
                last_spec_index = idx;
                quote! { let #sp = #spec_ident.subs[#idx].spec; }
            };
            quote! {
                ::core::option::Option::Some((#idx, #sm)) => {
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
        spec_ident.clone()
    } else {
        local("_spec")
    };
    let spec_assert = if indexed_variants > 1 {
        quote! { assert!(#spec_ident.subs.len() > #last_spec_index); }
    } else {
        quote! {}
    };

    let unique_message = format!("pound: two commands of `{name}` share a name or alias");
    quote! {
        impl #root::Subcommands for #name {}

        impl #root::Parse for #name {
            const SPEC: &'static #root::CommandSpec = {
                #(#sub_consts)*
                const #subs_id: &[#root::SubSpec] = &[ #(#sub_specs),* ];
                const _: () = ::core::assert!(
                    #root::checks::commands_unique(#subs_id),
                    #unique_message
                );
                const #root_id: #root::CommandSpec = #root::CommandSpec::new(#name_expr)
                    .version(#version_expr)
                    #hash_call
                    #about
                    #long_about
                    .subs(#subs_id);
                &#root_id
            };

            fn from_matches(#spec_param: &'static #root::CommandSpec, #m_ident: &#root::Matches)
                -> ::core::result::Result<Self, #root::Error>
            {
                #spec_assert
                match #root::Matches::sub(#m_ident) {
                    #(#arms)*
                    _ => ::core::result::Result::Err(#root::ErrorKind::MissingSubcommand.into()),
                }
            }
        }
    }
}

fn value_enum(cx: &Ctx, e: &venial::Enum) -> TokenStream2 {
    let root = &cx.root;
    let item = match attr::pound(&e.attributes) {
        Ok(item) => item,
        Err(e) => return err(&e),
    };
    let name = &e.name;
    if let Err(e) = item.allow_only(&[], name) {
        return err(&e);
    }
    let mut names = Vec::new();
    let mut arms = Vec::new();
    let mut spellings = Vec::new();
    let mut variants = Vec::new();
    let mut variant_names = Vec::new();
    for (variant, _) in &e.variants.inner {
        if !matches!(variant.fields, Fields::Unit) {
            return err("pound: ValueEnum needs unit variants only");
        }
        let vname = &variant.name;
        let vattr = match attr::pound(&variant.attributes) {
            Ok(attributes) => attributes,
            Err(e) => return err(&e),
        };
        if let Err(e) = vattr.allow_only(VALUE_VARIANT_ATTRIBUTES, vname) {
            return err(&e);
        }
        let label = vattr
            .name
            .unwrap_or_else(|| camel_to_kebab(&vname.to_string()));
        if let Some(first) = names.iter().position(|n| *n == label) {
            return err(&format!(
                "pound: ValueEnum variants `{}` and `{vname}` both spell `{label}`, so `{vname}` could never be parsed",
                variant_names[first]
            ));
        }
        variant_names.push(vname.to_string());
        arms.push(quote! { #label => ::core::result::Result::Ok(Self::#vname), });
        spellings.push(quote! { Self::#vname => #label, });
        variants.push(quote! { Self::#vname });
        names.push(label);
    }

    let (raw, other) = (local("s"), local("other"));
    quote! {
        impl #root::FromArg for #name {
            const POSSIBLE: ::core::option::Option<&'static [&'static str]> =
                ::core::option::Option::Some(&[ #(#names),* ]);

            fn from_arg(#raw: &str) -> ::core::result::Result<Self, #root::ValueError> {
                match #raw {
                    #(#arms)*
                    #other => ::core::result::Result::Err(
                        #root::ValueError::new(#other, "unrecognized value")
                    ),
                }
            }
        }

        impl #root::ArgValue for #name {
            const ALL: &'static [Self] = &[ #(#variants),* ];

            fn as_str(&self) -> &'static str {
                match *self {
                    #(#spellings)*
                }
            }
        }
    }
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
            attributes.allow_only(&["subcommand"], &field.name)?;
            if plan.sub.is_some() {
                return Err("pound: only one #[pound(subcommand)] field is allowed".into());
            }
            plan.sub = Some(sub_field(field)?);
        } else if attributes.flatten {
            attributes.allow_only(&["flatten"], &field.name)?;
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
            attributes.allow_only(FIELD_ATTRIBUTES, &field.name)?;
            plan.order.push(FieldOrder::Direct(plan.args.len()));
            plan.args.push(plan_field(field, attributes)?);
        }
    }
    Ok(plan)
}

fn command_attributes(
    attributes: &[venial::Attribute],
    owner: &proc_macro2::Ident,
) -> Result<Pound, String> {
    let item = attr::pound(attributes)?;
    item.allow_only(ITEM_ATTRIBUTES, owner)?;
    Ok(item)
}

fn positional_assert(cx: &Ctx, owner: &str, spec: &proc_macro2::Ident) -> TokenStream2 {
    let root = &cx.root;
    let message = format!(
        "pound: `{owner}` has a positional or subcommand after a variadic or trailing positional"
    );
    quote! {
        const _: () = ::core::assert!(#root::checks::positionals_reachable(&#spec), #message);
    }
}

// the one tuple field of a `#[pound(flatten)]` enum variant
fn flattened_variant(variant: &venial::EnumVariant) -> Result<TokenStream2, String> {
    let message = format!(
        "pound: #[pound(flatten)] needs exactly one unattributed tuple field (`{}`)",
        variant.name
    );
    let Fields::Tuple(tuple) = &variant.fields else {
        return Err(message);
    };
    let mut fields = tuple.fields.items();
    match (fields.next(), fields.next()) {
        (Some(field), None) if field.attributes.is_empty() => {
            Ok(field.ty.tokens.iter().cloned().collect())
        },
        _ => Err(message),
    }
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
fn sub_parts(cx: &Ctx, sub: Option<&SubField>) -> (TokenStream2, bool) {
    let root = &cx.root;
    match sub {
        Some(sf) => {
            let ty = &sf.ty;
            (
                quote! { #root::checks::subcommand_spec::<#ty>().subs },
                sf.optional,
            )
        },
        None => (quote! { &[] }, false),
    }
}

// the `field: <built subcommand>` reader for a subcommand field.
fn sub_reader(cx: &Ctx, sf: &SubField, m: &Ident) -> TokenStream2 {
    let root = &cx.root;
    let ident = &sf.ident;
    let ty = &sf.ty;
    let build =
        quote! { <#ty as #root::Parse>::from_matches(<#ty as #root::Parse>::SPEC, #m)? };
    if sf.optional {
        quote! {
            #ident: if #root::Matches::sub(#m).is_some() {
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
    let help = if a.help.is_some() {
        attr::summary(&doc)
    } else {
        attr::short_help(&doc)
    }
    .to_owned();
    let long_help = a
        .long_help
        .clone()
        .or_else(|| (attr::summary(&doc) != doc).then(|| doc.clone()));

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
    if let Some(inner) = strip_wrapper(toks, "Option", "option", &["core", "std"]) {
        return (false, Card::Opt, inner);
    }
    if let Some(inner) = strip_wrapper(toks, "Vec", "vec", &["alloc", "std"]) {
        return (false, Card::Many, inner);
    }
    (false, Card::One, toks.iter().cloned().collect())
}

// `Wrapper < Inner >` -> the `Inner` tokens, where `Wrapper` may also be written
// `module::Wrapper` or `krate::module::Wrapper` with an optional leading `::`.
fn strip_wrapper(
    toks: &[TokenTree],
    wrapper: &str,
    module: &str,
    crates: &[&str],
) -> Option<TokenStream2> {
    let is_colon = |tok: Option<&TokenTree>| matches!(tok, Some(TokenTree::Punct(p)) if p.as_char() == ':');
    let mut at = if is_colon(toks.first()) && is_colon(toks.get(1)) { 2 } else { 0 };
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
        [home, name] => home == module && name == wrapper,
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

// --- emit helpers

// a const assertion that a declared default is one of its type's values. only
// a `FromArg` conversion exposes a value list, so a custom parser gets none.
fn default_assert(cx: &Ctx, p: &Plan) -> Option<TokenStream2> {
    let root = &cx.root;
    let default = p.default.as_ref()?;
    let tokens = default.tokens();
    if !matches!(
        p.conversion,
        Some(Conversion::FromArg | Conversion::CheckedFromArg { .. })
    ) {
        return None;
    }
    let inner = &p.inner_ty;
    let message = match default {
        Text::Lit(text) => format!(
            "pound: default \"{text}\" is not one of the possible values for `{}`",
            p.ident
        ),
        Text::Expr(_) => format!(
            "pound: the default is not one of the possible values for `{}`",
            p.ident
        ),
    };
    Some(quote! {
        const _: () = ::core::assert!(
            #root::checks::default_allowed(#tokens, <#inner as #root::FromArg>::POSSIBLE),
            #message
        );
    })
}

fn arg_expr(cx: &Ctx, p: &Plan) -> TokenStream2 {
    let root = &cx.root;
    let kind = match p.kind {
        ArgKind::Flag => quote! { #root::Kind::Flag },
        ArgKind::Count => quote! { #root::Kind::Count },
        ArgKind::Opt => quote! { #root::Kind::Opt },
        ArgKind::Positional => quote! { #root::Kind::Positional },
        ArgKind::Trailing => quote! { #root::Kind::Trailing },
    };
    let mut e = quote! { #root::ArgSpec::new(#kind) };
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
        let d = d.tokens();
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
    if let Some(h) = p.heading.as_ref().filter(|_| cx.help) {
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
        e = quote! { #e.possible_opt(<#inner as #root::FromArg>::POSSIBLE) };
    }
    if p.hidden {
        e = quote! { #e.hidden() };
    }
    if p.global {
        e = quote! { #e.global() };
    }
    if let Some(lh) = &p.long_help {
        let lh = help_lit(cx, lh);
        e = quote! { #e.long_help(#lh) };
    }
    let help = help_lit(cx, &p.help);
    quote! { #e.help(#help) }
}

fn reader(cx: &Ctx, p: &Plan, i: usize, m: &Ident, spec: &Ident) -> TokenStream2 {
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
                let convert = conversion_closure(cx, conversion, inner);
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

fn conversion_closure(cx: &Ctx, conversion: &Conversion, inner: &TokenStream2) -> TokenStream2 {
    let root = &cx.root;
    let parse = parse_value_expr(cx, conversion, inner);
    let max_len = match conversion {
        Conversion::CheckedFromArg { max_len, .. } => max_len.as_deref().map(|value| max_len_check(cx, value)),
        Conversion::FromArg | Conversion::CustomParse { .. } => None,
    };
    let min = match conversion {
        Conversion::CheckedFromArg { min, .. } => {
            min.as_deref().map(|v| bound_check(cx, inner, "min", v))
        },
        Conversion::FromArg | Conversion::CustomParse { .. } => None,
    };
    let max = match conversion {
        Conversion::CheckedFromArg { max, .. } => {
            max.as_deref().map(|v| bound_check(cx, inner, "max", v))
        },
        Conversion::FromArg | Conversion::CustomParse { .. } => None,
    };
    let validate = match conversion {
        Conversion::CheckedFromArg { validate, .. } | Conversion::CustomParse { validate, .. } => {
            validate.as_ref().map(|value| validate_check(cx, value))
        },
        Conversion::FromArg => None,
    };
    let (sv, val) = (local("s"), local("value"));
    quote! {
        |#sv: &str| -> ::core::result::Result<#inner, #root::ValueError> {
            #max_len
            let #val = #parse;
            #min
            #max
            #validate
            ::core::result::Result::Ok(#val)
        }
    }
}

fn parse_value_expr(cx: &Ctx, conversion: &Conversion, inner: &TokenStream2) -> TokenStream2 {
    let root = &cx.root;
    let (sv, fail) = (local("s"), local("msg"));
    match conversion {
        Conversion::FromArg | Conversion::CheckedFromArg { .. } => {
            quote! { <#inner as #root::FromArg>::from_arg(#sv)? }
        },
        Conversion::CustomParse { parse, .. } => {
            quote! {
                (#parse)(#sv).map_err(|#fail| #root::ValueError::new(#sv, #fail))?
            }
        },
    }
}

fn max_len_check(cx: &Ctx, value: &str) -> TokenStream2 {
    let root = &cx.root;
    let sv = local("s");
    if let Ok(max) = TokenStream2::from_str(value) {
        quote! {
            if #sv.chars().count() > (#max) {
                return ::core::result::Result::Err(#root::ValueError::new(
                    #sv,
                    ::core::concat!("must be at most ", ::core::stringify!(#max), " chars"),
                ));
            }
        }
    } else {
        quote! { ::core::compile_error!("pound: invalid max_len bound") }
    }
}

fn bound_check(cx: &Ctx, inner: &TokenStream2, kind: &str, value: &str) -> TokenStream2 {
    let root = &cx.root;
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
    let (sv, val, bnd) = (local("s"), local("value"), local("bound"));
    quote! {
        let #bnd = <#inner as #root::FromArg>::from_arg(#value)
            .map_err(|_| #root::ValueError::new(#sv, #invalid))?;
        if #val #op #bnd {
            return ::core::result::Result::Err(#root::ValueError::new(#sv, #msg));
        }
    }
}

fn validate_check(cx: &Ctx, check: &TokenStream2) -> TokenStream2 {
    let root = &cx.root;
    let (sv, val, fail) = (local("s"), local("value"), local("msg"));
    quote! {
        if let ::core::result::Result::Err(#fail) = (#check)(&#val) {
            return ::core::result::Result::Err(#root::ValueError::new(#sv, #fail));
        }
    }
}

// distinct group names in first-seen order, marked required where listed.
fn group_exprs(cx: &Ctx, plans: &[Plan], required: &[String]) -> Vec<TokenStream2> {
    let root = &cx.root;
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
            let base = quote! { #root::GroupSpec::new(#g) };
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
fn group_asserts(cx: &Ctx, plans: &[Plan],
    required_groups: &[String],
    spec: &proc_macro2::Ident,
) -> TokenStream2 {
    let root = &cx.root;
    required_groups
        .iter()
        .filter(|name| !has_direct_member(plans, name))
        .map(|name| {
            let message = format!("pound: required group `{name}` has no members");
            quote! {
                const _: () = ::core::assert!(
                    #root::checks::group_has_members(&#spec, #name),
                    #message
                );
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
    quote! { .hash_opt(::core::option_env!("POUND_GIT_HASH")) }
}

// bake the help string only when the feature is on, otherwise emit "".
// the `.about()` / `.long_about()` calls for a doc comment. the long form is
// chained only when it says more than the summary already does.
fn about_calls(cx: &Ctx, doc: &str) -> (TokenStream2, TokenStream2) {
    let summary = help_lit(cx, attr::short_help(doc));
    let about = quote! { .about(#summary) };
    if attr::summary(doc) == doc {
        return (about, quote!());
    }
    let full = help_lit(cx, doc);
    (about, quote! { .long_about(#full) })
}

// bake the help string only when the wrapping crate asks for help text,
// otherwise emit "".
fn help_lit(cx: &Ctx, s: &str) -> TokenStream2 {
    if cx.help { quote! { #s } } else { quote! { "" } }
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

// mixed-site spans still let a user's unit struct or const capture a binding
// pattern or an item reference, so generated names also carry a prefix.
fn local(name: &str) -> Ident {
    Ident::new(&format!("__pound_{name}"), Span::mixed_site())
}

fn generated(name: &str) -> Ident {
    Ident::new(&format!("__POUND_{name}"), Span::mixed_site())
}

fn err(msg: &str) -> TokenStream2 {
    quote! { ::core::compile_error!(#msg); }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expand(help: bool) -> String {
        let input: TokenStream2 = r#"struct Args { #[pound(long, heading = "Output")] verbose: bool }"#
            .parse()
            .unwrap();
        derive_parse(input, &Options::new("::pound", help)).to_string()
    }

    fn expand_field(attr: &str) -> String {
        let input: TokenStream2 = format!("struct Args {{ #[pound({attr})] x: u8 }}")
            .parse()
            .unwrap();
        derive_parse(input, &Options::new("::pound", true)).to_string()
    }

    #[test]
    fn a_turbofish_comma_stays_inside_the_expression() {
        let out = expand_field("long, parse = pair::<u8, u16>, default = { DEFAULT }");
        assert!(out.contains("(pair ::< u8 , u16 >) (__pound_s)"), "{out}");
        assert!(out.contains("default (DEFAULT)"), "{out}");
    }

    fn expand_typed(ty: &str) -> String {
        let input: TokenStream2 = format!("struct Args {{ #[pound(long)] x: {ty} }}")
            .parse()
            .unwrap();
        derive_parse(input, &Options::new("::pound", true)).to_string()
    }

    #[test]
    fn qualified_option_and_vec_paths_are_recognised() {
        for ty in [
            "::core::option::Option<u8>",
            "core::option::Option<u8>",
            "std::option::Option<u8>",
            "::std::option::Option<u8>",
            "option::Option<u8>",
            "Option<u8>",
        ] {
            assert!(expand_typed(ty).contains("optional_map :: < u8 >"), "{ty}");
        }
        for ty in [
            "::alloc::vec::Vec<u8>",
            "alloc::vec::Vec<u8>",
            "std::vec::Vec<u8>",
            "::std::vec::Vec<u8>",
            "vec::Vec<u8>",
            "Vec<u8>",
        ] {
            assert!(expand_typed(ty).contains("many_map :: < u8 >"), "{ty}");
        }
    }

    #[test]
    fn foreign_paths_named_option_or_vec_stay_scalar() {
        for ty in ["my::Option<u8>", "other::option::Option<u8>", "core::vec::Vec<u8>"] {
            assert!(expand_typed(ty).contains("required_map :: < "), "{ty}");
        }
    }

    #[test]
    fn a_string_that_is_no_path_is_rejected() {
        let out = expand_field("long, parse = \"not a path (\"");
        assert!(out.contains("is not a valid path"), "{out}");
    }

    #[test]
    fn a_summary_loses_one_trailing_period_and_long_help_keeps_it() {
        let input: TokenStream2 = r#"
            /// runs the thing.
            ///
            /// more detail here.
            struct Args {
                /// the target.
                ///
                /// extra about the target.
                #[pound(long)]
                target: String,
                /// waits...
                #[pound(long)]
                wait: bool,
                #[pound(long, help = "explicit.")]
                kept: bool,
            }
        "#
        .parse()
        .unwrap();
        let out = derive_parse(input, &Options::new("::pound", true)).to_string();
        assert!(out.contains(". about (\"runs the thing\")"), "{out}");
        assert!(out.contains("runs the thing.\\n\\nmore detail here."), "{out}");
        assert!(out.contains(". long_help ("), "{out}");
        assert!(out.contains(". help (\"the target\")"), "{out}");
        assert!(out.contains(". help (\"waits...\")"), "{out}");
        assert!(out.contains(". help (\"explicit.\")"), "{out}");
    }

    fn expand_enum(body: &str) -> String {
        let input: TokenStream2 = format!("enum Mode {{ {body} }}").parse().unwrap();
        derive_value_enum(input, &Options::new("::pound", true)).to_string()
    }

    #[test]
    fn two_variants_with_one_spelling_are_rejected() {
        let renamed = expand_enum(r#"Fast, #[pound(name = "fast")] Quick"#);
        assert!(
            renamed.contains("`Fast` and `Quick` both spell `fast`"),
            "{renamed}"
        );
        let explicit = expand_enum(r#"#[pound(name = "x")] A, #[pound(name = "x")] B"#);
        assert!(explicit.contains("`A` and `B` both spell `x`"), "{explicit}");
        let kebab = expand_enum(r#"#[pound(name = "my-mode")] Plain, MyMode"#);
        assert!(kebab.contains("`Plain` and `MyMode` both spell `my-mode`"), "{kebab}");
    }

    #[test]
    fn distinct_spellings_expand() {
        let out = expand_enum(r#"Fast, #[pound(name = "slow")] Quick"#);
        assert!(!out.contains("compile_error"), "{out}");
    }

    #[test]
    fn headings_are_baked_only_when_help_is_on() {
        assert!(expand(true).contains("heading"));
        assert!(!expand(false).contains("heading"));
    }
}
