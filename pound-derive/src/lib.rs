// SPDX-License-Identifier: EUPL-1.2

//! derive macros for pound. `#[derive(Parse)]` turns a struct into a flat
//! command and an enum into a subcommand tree, `#[derive(ValueEnum)]` wires a
//! unit enum up as a `FromArg` choice type. all of it just emits the static
//! `CommandSpec` plus a `from_matches` reader, the runtime does the work.

mod attr;

use std::collections::HashMap;

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

#[proc_macro_derive(Parse, attributes(pound))]
pub fn derive_parse(input: TokenStream) -> TokenStream {
    match parse_item(input.into()) {
        Ok(Item::Struct(s)) => parse_struct(&s),
        Ok(Item::Enum(e)) => parse_enum(&e),
        Ok(_) => err("pound: Parse can only derive on a struct or enum"),
        Err(e) => TokenStream::from(e.to_compile_error()),
    }
}

#[proc_macro_derive(ValueEnum)]
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

// the resolved plan for one field.
struct Plan {
    ident:      proc_macro2::Ident,
    kind:       &'static str,
    long:       Option<String>,
    short:      Option<char>,
    required:   bool,
    multi:      bool,
    group:      Option<String>,
    default:    Option<String>,
    env:        Option<String>,
    value_name: String,
    help:       String,
    aliases:    Vec<String>,
    conflicts_with: Vec<String>,
    hidden:     bool,
    card:       Card,
    inner_ty:   TokenStream2,
    full_ty:    TokenStream2,
}

// a field that delegates to its type's subcommand tree.
struct SubField {
    ident:    proc_macro2::Ident,
    ty:       TokenStream2,
    optional: bool,
}

fn parse_struct(s: &venial::Struct) -> TokenStream {
    let (plans, sub) = match analyze(&s.fields) {
        Ok(v) => v,
        Err(e) => return err(&e),
    };
    let item = attr::pound(&s.attributes);
    let name = &s.name;

    let conflicts = match conflict_pairs(&plans) {
        Ok(c) => c,
        Err(e) => return err(&e),
    };
    let args = plans.iter().map(arg_expr);
    let groups = group_exprs(&plans, &item.required_groups);
    let conflicts = conflict_tokens(&conflicts);
    let (subs, sub_optional) = sub_parts(sub.as_ref());
    let name_expr = name_expr(&item);
    let version_expr = version_expr(&item);
    let about = help_lit(&attr::doc(&s.attributes));

    let m = quote!(m);
    let sp = quote!(spec);
    let readers = plans.iter().enumerate().map(|(i, p)| reader(p, i, &m, &sp));
    let sub_reader = sub.as_ref().map(|sf| sub_reader(sf, &m));

    // avoid unused-param warnings when a command carries only a subcommand.
    let spec_param = if plans.is_empty() { quote!(_spec) } else { quote!(spec) };
    let m_param = if plans.is_empty() && sub.is_none() { quote!(_m) } else { quote!(m) };

    quote! {
        impl ::pound::Parse for #name {
            const SPEC: &'static ::pound::CommandSpec = {
                const ARGS: &[::pound::ArgSpec] = &[ #(#args),* ];
                const GROUPS: &[::pound::GroupSpec] = &[ #(#groups),* ];
                const CONFLICTS: &[(usize, usize)] = #conflicts;
                const CMD: ::pound::CommandSpec = ::pound::CommandSpec {
                    name:         #name_expr,
                    version:      #version_expr,
                    about:        #about,
                    args:         ARGS,
                    groups:       GROUPS,
                    conflicts:    CONFLICTS,
                    subs:         #subs,
                    sub_optional: #sub_optional,
                };
                &CMD
            };

            fn from_matches(#spec_param: &'static ::pound::CommandSpec, #m_param: &::pound::Matches)
                -> ::core::result::Result<Self, ::pound::Error>
            {
                ::core::result::Result::Ok(Self { #(#readers,)* #sub_reader })
            }
        }
    }
    .into()
}

#[allow(clippy::too_many_lines, reason = "one cohesive codegen pass reads best whole")]
fn parse_enum(e: &venial::Enum) -> TokenStream {
    let item = attr::pound(&e.attributes);
    let name = &e.name;
    let name_expr = name_expr(&item);
    let version_expr = version_expr(&item);
    let about = help_lit(&attr::doc(&e.attributes));

    let mut sub_consts = Vec::new();
    let mut sub_specs = Vec::new();
    let mut arms = Vec::new();
    let mut uses_spec = false;

    for (idx, variant) in e.variants.items().enumerate() {
        let (plans, sub) = match analyze(&variant.fields) {
            Ok(v) => v,
            Err(msg) => return err(&msg),
        };
        let vattr = attr::pound(&variant.attributes);
        let vname = &variant.name;
        let sub_name = vattr.name.clone().unwrap_or_else(|| camel_to_kebab(&vname.to_string()));
        let sub_about = help_lit(&attr::doc(&variant.attributes));
        let hidden = vattr.hidden;

        let conflicts = match conflict_pairs(&plans) {
            Ok(c) => c,
            Err(msg) => return err(&msg),
        };
        let args = plans.iter().map(arg_expr);
        let groups = group_exprs(&plans, &vattr.required_groups);
        let conflicts = conflict_tokens(&conflicts);
        let (subs, sub_optional) = sub_parts(sub.as_ref());
        let ak = format_ident!("ARGS{}", idx);
        let gk = format_ident!("GROUPS{}", idx);
        let xk = format_ident!("CONFLICTS{}", idx);
        let ck = format_ident!("CMD{}", idx);
        sub_consts.push(quote! {
            const #ak: &[::pound::ArgSpec] = &[ #(#args),* ];
            const #gk: &[::pound::GroupSpec] = &[ #(#groups),* ];
            const #xk: &[(usize, usize)] = #conflicts;
            const #ck: ::pound::CommandSpec = ::pound::CommandSpec {
                name:         #sub_name,
                version:      "",
                about:        #sub_about,
                args:         #ak,
                groups:       #gk,
                conflicts:    #xk,
                subs:         #subs,
                sub_optional: #sub_optional,
            };
        });
        let valias = &vattr.aliases;
        sub_specs.push(quote! {
            ::pound::SubSpec {
                name:    #sub_name,
                aliases: &[ #(#valias),* ],
                about:   #sub_about,
                spec:    &#ck,
                hidden:  #hidden,
            }
        });

        let m = quote!(__sm);
        let sp = quote!(__s);
        arms.push(if plans.is_empty() && sub.is_none() {
            quote! { ::core::option::Option::Some((#idx, _)) => ::core::result::Result::Ok(Self::#vname), }
        } else {
            let readers = plans.iter().enumerate().map(|(i, p)| reader(p, i, &m, &sp));
            let sub_r = sub.as_ref().map(|sf| sub_reader(sf, &m));
            let bind = if plans.is_empty() {
                quote! {}
            } else {
                uses_spec = true;
                quote! { let __s = spec.subs[#idx].spec; }
            };
            quote! {
                ::core::option::Option::Some((#idx, __sm)) => {
                    #bind
                    ::core::result::Result::Ok(Self::#vname { #(#readers,)* #sub_r })
                },
            }
        });
    }

    // `spec` is only read when some variant has its own args
    let spec_param = if uses_spec { quote!(spec) } else { quote!(_spec) };

    quote! {
        impl ::pound::Parse for #name {
            const SPEC: &'static ::pound::CommandSpec = {
                #(#sub_consts)*
                const SUBS: &[::pound::SubSpec] = &[ #(#sub_specs),* ];
                const ROOT: ::pound::CommandSpec = ::pound::CommandSpec {
                    name:         #name_expr,
                    version:      #version_expr,
                    about:        #about,
                    args:         &[],
                    groups:       &[],
                    conflicts:    &[],
                    subs:         SUBS,
                    sub_optional: false,
                };
                &ROOT
            };

            fn from_matches(#spec_param: &'static ::pound::CommandSpec, m: &::pound::Matches)
                -> ::core::result::Result<Self, ::pound::Error>
            {
                match ::pound::Matches::sub(m) {
                    #(#arms)*
                    _ => ::core::result::Result::Err(::pound::Error::MissingSubcommand),
                }
            }
        }
    }
    .into()
}

fn value_enum(e: &venial::Enum) -> TokenStream {
    let name = &e.name;
    let mut names = Vec::new();
    let mut arms = Vec::new();
    for (variant, _) in &e.variants.inner {
        if !matches!(variant.fields, Fields::Unit) {
            return err("pound: ValueEnum needs unit variants only");
        }
        let vname = &variant.name;
        let vattr = attr::pound(&variant.attributes);
        let label = vattr.name.unwrap_or_else(|| camel_to_kebab(&vname.to_string()));
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
                        ::pound::ValueError::new(other, "unrecognised value")
                    ),
                }
            }
        }
    }
    .into()
}

// --- field planning

// split a struct/variant's fields into regular args and an optional single
// `#[pound(subcommand)]` field.
fn analyze(fields: &Fields) -> Result<(Vec<Plan>, Option<SubField>), String> {
    let named = match fields {
        Fields::Unit => return Ok((Vec::new(), None)),
        Fields::Tuple(_) => {
            return Err("pound: tuple fields are not supported, use named fields".into());
        },
        Fields::Named(named) => named,
    };
    let mut args = Vec::new();
    let mut sub = None;
    for field in named.fields.items() {
        if attr::pound(&field.attributes).subcommand {
            if sub.is_some() {
                return Err("pound: only one #[pound(subcommand)] field is allowed".into());
            }
            sub = Some(sub_field(field)?);
        } else {
            args.push(plan_field(field));
        }
    }
    Ok((args, sub))
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

fn plan_field(field: &NamedField) -> Plan {
    let a = attr::pound(&field.attributes);
    let (is_bool, card, inner_ty) = classify(&field.ty);
    let full_ty: TokenStream2 = field.ty.tokens.iter().cloned().collect();
    let fname = field.name.to_string();

    let kind = if is_bool {
        "Flag"
    } else if a.count {
        "Count"
    } else if a.trailing {
        "Trailing"
    } else if a.is_named() {
        "Opt"
    } else {
        "Positional"
    };

    // long/short only for named kinds, defaulting a long name when neither given.
    let (mut long, mut short) = (None, None);
    if matches!(kind, "Flag" | "Count" | "Opt") {
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

    let required = matches!(kind, "Opt" | "Positional" | "Trailing")
        && card == Card::One
        && a.default.is_none();

    Plan {
        ident: field.name.clone(),
        kind,
        long,
        short,
        required,
        multi: card == Card::Many,
        group: a.group,
        default: a.default,
        env: a.env,
        value_name: a.value_name.unwrap_or(fname),
        help: a.help.unwrap_or_else(|| attr::doc(&field.attributes)),
        aliases: a.aliases,
        conflicts_with: a.conflicts_with,
        hidden: a.hidden,
        card,
        inner_ty,
        full_ty,
    }
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

fn arg_expr(p: &Plan) -> TokenStream2 {
    let kind = format_ident!("{}", p.kind);
    let mut e = quote! { ::pound::ArgSpec::new(::pound::Kind::#kind) };
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
    if let Some(g) = &p.group {
        e = quote! { #e.group(#g) };
    }
    if let Some(d) = &p.default {
        e = quote! { #e.default(#d) };
    }
    if let Some(ev) = &p.env {
        e = quote! { #e.env(#ev) };
    }
    if !p.aliases.is_empty() {
        let al = &p.aliases;
        e = quote! { #e.aliases(&[ #(#al),* ]) };
    }
    let vn = &p.value_name;
    e = quote! { #e.value_name(#vn) };
    // valued kinds pull a possible-value list from a value enum (None otherwise)
    if matches!(p.kind, "Opt" | "Positional" | "Trailing") {
        let inner = &p.inner_ty;
        e = quote! { #e.possible_opt(<#inner as ::pound::FromArg>::POSSIBLE) };
    }
    if p.hidden {
        e = quote! { #e.hidden() };
    }
    let help = help_lit(&p.help);
    quote! { #e.help(#help) }
}



fn reader(p: &Plan, i: usize, m: &TokenStream2, spec: &TokenStream2) -> TokenStream2 {
    let fname = &p.ident;
    let body = match p.kind {
        "Flag" => quote! { #m.flag(#i) },
        "Count" => {
            let ty = &p.full_ty;
            quote! { #m.count(#i) as #ty }
        },
        _ => {
            let inner = &p.inner_ty;
            match p.card {
                Card::One => quote! { #m.required::<#inner>(#spec, #i)? },
                Card::Opt => quote! { #m.optional::<#inner>(#spec, #i)? },
                Card::Many => quote! { #m.many::<#inner>(#spec, #i)? },
            }
        },
    };
    quote! { #fname: #body }
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

// resolve field-level conflicts_with names to normalised, deduped index pairs.
fn conflict_pairs(plans: &[Plan]) -> Result<Vec<(usize, usize)>, String> {
    let index: HashMap<String, usize> =
        plans.iter().enumerate().map(|(i, p)| (p.ident.to_string(), i)).collect();
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

// the `&[(usize, usize)]` token list for a conflict-pair set.
fn conflict_tokens(pairs: &[(usize, usize)]) -> TokenStream2 {
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
        |v| quote! { #v },
    )
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
