// SPDX-License-Identifier: EUPL-1.2

//! the tokens for a checked model: the static `CommandSpec`, its const
//! asserts, and the `from_matches` reader

use std::str::FromStr;

use proc_macro2::{
    Ident,
    Span,
    TokenStream,
};
use quote::quote;

use crate::{
    Options,
    attr::{
        Text,
        ValueHint,
        short_help,
        summary,
    },
    model::{
        Arg,
        ArgKind,
        Command,
        CommandFields,
        Conversion,
        Derive,
        Header,
        Shape,
        Slot,
        ValueEnum,
        Variant,
        VariantKind,
    },
};

pub struct Emitter {
    root: TokenStream,
    help: bool,
}

impl Emitter {
    pub fn new(options: &Options) -> Self {
        Self {
            root: TokenStream::from_str(options.root).expect("Options::root is a path"),
            help: options.help,
        }
    }

    pub fn derive(&self, derive: &Derive) -> TokenStream {
        match derive {
            Derive::Struct {
                ident,
                header,
                command,
            } => self.parse_struct(ident, header, command),
            Derive::Enum {
                ident,
                header,
                variants,
            } => self.parse_enum(ident, header, variants),
        }
    }

    fn parse_struct(&self, name: &Ident, header: &Header, command: &Command) -> TokenStream {
        let root = &self.root;
        let fields = &command.fields;
        let head = self.header(header);
        let cmd_id = generated("CMD");
        let consts = self.command_consts(command, &head, "", &cmd_id);
        let (m, sp) = (local("m"), local("spec"));
        let readers = self.readers(fields, &m, &sp);

        // avoid unused-param warnings when a command carries only a subcommand.
        let spec_param = if fields.args.is_empty() {
            local("_spec")
        } else {
            sp
        };

        let m_param =
            if fields.args.is_empty() && fields.flattened.is_empty() && fields.sub.is_none() {
                local("_m")
            } else {
                m
            };

        quote! {
            impl #root::Parse for #name {
                const SPEC: &'static #root::CommandSpec = {
                    #consts
                    &#cmd_id
                };

                fn from_matches(#spec_param: &'static #root::CommandSpec, #m_param: &#root::Matches)
                    -> ::core::result::Result<Self, #root::Error>
                {
                    ::core::result::Result::Ok(Self { #readers })
                }
            }
        }
    }

    fn parse_enum(&self, name: &Ident, header: &Header, variants: &[Variant]) -> TokenStream {
        let root = &self.root;
        let head = self.header(header);
        let (sm, spec_ident, m_ident) = (local("sm"), local("spec"), local("m"));
        let (subs_id, root_id) = (generated("SUBS"), generated("ROOT"));

        let built = variants
            .iter()
            .enumerate()
            .map(|(idx, variant)| self.variant(idx, variant, &sm, &spec_ident))
            .collect::<Vec<_>>();

        let sub_consts = built.iter().map(|v| &v.consts);
        let sub_specs = built.iter().map(|v| &v.spec);
        let arms = built.iter().map(|v| &v.arm);

        let reading = built
            .iter()
            .enumerate()
            .filter(|(_, v)| v.reads_spec)
            .map(|(idx, _)| idx)
            .collect::<Vec<_>>();

        // `spec` is only read when some variant has its own args
        let spec_param = if reading.is_empty() {
            local("_spec")
        } else {
            spec_ident.clone()
        };

        let spec_assert = match reading[..] {
            [_, .., last] => quote! { assert!(#spec_ident.subs.len() > #last); },
            _ => quote! {},
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
                    const #root_id: #root::CommandSpec = #head
                        .subs(#subs_id);
                    &#root_id
                };

                fn from_matches(#spec_param: &'static #root::CommandSpec, #m_ident: &#root::Matches)
                    -> ::core::result::Result<Self, #root::Error>
                {
                    #spec_assert
                    match #root::Matches::sub(#m_ident) {
                        #(#arms)*
                        _ => ::core::result::Result::Err(#root::ErrorKind::MissingSubcommand { help: ::core::option::Option::None }.into()),
                    }
                }
            }
        }
    }

    fn variant(&self, idx: usize, variant: &Variant, sm: &Ident, spec: &Ident) -> VariantTokens {
        let root = &self.root;
        let vname = &variant.ident;

        let (name, aliases, hidden, doc, command) = match &variant.kind {
            VariantKind::Command {
                name,
                aliases,
                hidden,
                doc,
                command,
            } => (name, aliases, *hidden, doc, command),
            VariantKind::Flatten(ty) => {
                return VariantTokens {
                    consts:     TokenStream::new(),
                    spec:       quote! {
                        #root::SubSpec::flatten(#root::checks::subcommand_spec::<#ty>())
                    },
                    arm:        quote! {
                        ::core::option::Option::Some((#idx, #sm)) => ::core::result::Result::Ok(
                            Self::#vname(<#ty as #root::Parse>::from_matches(
                                <#ty as #root::Parse>::SPEC,
                                #sm,
                            )?),
                        ),
                    },
                    reads_spec: false,
                };
            },
        };

        let fields = &command.fields;
        let ck = generated(&format!("CMD{idx}"));
        let (about_call, long_about_call) = self.about_calls(doc);

        let head = quote! {
            #root::CommandSpec::new(#name)
                #about_call
                #long_about_call
        };

        let about = self.help_lit(short_help(doc));
        let hidden_call = if hidden { quote!(.hidden()) } else { quote!() };
        let reads_spec = !fields.args.is_empty();

        let arm = if fields.args.is_empty() && fields.flattened.is_empty() && fields.sub.is_none() {
            quote! { ::core::option::Option::Some((#idx, _)) => ::core::result::Result::Ok(Self::#vname), }
        } else {
            let sp = local("s");
            let readers = self.readers(fields, sm, &sp);
            let bind = reads_spec.then(|| quote! { let #sp = #spec.subs[#idx].spec; });

            quote! {
                ::core::option::Option::Some((#idx, #sm)) => {
                    #bind
                    ::core::result::Result::Ok(Self::#vname { #readers })
                },
            }
        };

        VariantTokens {
            consts: self.command_consts(command, &head, &idx.to_string(), &ck),
            spec: quote! {
                #root::SubSpec::new(#name, &#ck)
                    .aliases(&[ #(#aliases),* ])
                    .about(#about)
                    #hidden_call
            },
            arm,
            reads_spec,
        }
    }

    pub fn value_enum(&self, value_enum: &ValueEnum) -> TokenStream {
        let root = &self.root;
        let name = &value_enum.ident;
        let labels = value_enum.variants.iter().map(|(_, label)| label);

        let arms = value_enum
            .variants
            .iter()
            .map(|(vname, label)| quote! { #label => ::core::result::Result::Ok(Self::#vname), });

        let spellings = value_enum
            .variants
            .iter()
            .map(|(vname, label)| quote! { Self::#vname => #label, });

        let variants = value_enum
            .variants
            .iter()
            .map(|(vname, _)| quote! { Self::#vname });

        let (raw, other) = (local("s"), local("other"));

        quote! {
            impl #root::FromArg for #name {
                const POSSIBLE: ::core::option::Option<&'static [&'static str]> =
                    ::core::option::Option::Some(&[ #(#labels),* ]);

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

    // without a `name`, a command shows the name the program runs as when it
    // parses argv, and the package name otherwise
    fn header(&self, header: &Header) -> TokenStream {
        let root = &self.root;

        let new = match &header.name {
            Some(name) => quote! { #root::CommandSpec::new(#name) },
            None => {
                quote! { #root::CommandSpec::new(::core::env!("CARGO_PKG_NAME")).invoked_name() }
            },
        };

        let version = header.version.as_ref().map_or_else(
            || quote! { ::core::env!("CARGO_PKG_VERSION") },
            |tokens| tokens.iter().cloned().collect::<TokenStream>(),
        );

        let (about, long_about) = self.about_calls(&header.doc);

        quote! {
            #new
                .version(#version)
                .hash_opt(::core::option_env!("POUND_GIT_HASH"))
                #about
                #long_about
        }
    }

    fn command_consts(
        &self,
        command: &Command,
        head: &TokenStream,
        suffix: &str,
        cmd_id: &Ident,
    ) -> TokenStream {
        let root = &self.root;
        let fields = &command.fields;
        let owner = &command.owner;

        let [args_id, groups_id, conflicts_id, requires_id] =
            ["ARGS", "GROUPS", "CONFLICTS", "REQUIRES"]
                .map(|id| generated(&format!("{id}{suffix}")));

        let args = fields.args.iter().map(|arg| self.arg(arg));

        let default_asserts = fields
            .args
            .iter()
            .filter_map(|arg| self.default_assert(arg));

        let groups = command.groups.iter().map(|group| {
            let name = &group.name;

            if group.required {
                quote! { #root::GroupSpec::new(#name).required() }
            } else {
                quote! { #root::GroupSpec::new(#name) }
            }
        });

        let conflicts = index_pairs(&command.conflicts);
        let requires = index_pairs(&command.requires);
        let flatten_calls = self.flatten_calls(fields);

        let (subs, sub_optional_call) = match &fields.sub {
            Some(sub) => {
                let ty = &sub.ty;
                let optional = if sub.optional {
                    quote!(.sub_optional())
                } else {
                    quote!()
                };
                (
                    quote! { #root::checks::subcommand_spec::<#ty>().subs },
                    optional,
                )
            },
            None => (quote! { &[] }, quote!()),
        };

        let unique_message = format!("pound: two args of `{owner}` answer to the same spelling");

        let group_asserts = command
            .groups
            .iter()
            .filter(|group| group.required && !group.direct)
            .map(|group| {
                let name = &group.name;
                let message = format!("pound: required group `{name}` has no members");

                quote! {
                    const _: () = ::core::assert!(
                        #root::checks::group_has_members(&#cmd_id, #name),
                        #message
                    );
                }
            });

        // a flattened struct may bring its own subcommand field, and only one
        // of those can be selected at a level
        let selector_assert = if fields.flattened.is_empty() {
            TokenStream::new()
        } else {
            let message = format!("pound: `{owner}` has more than one subcommand field");

            quote! {
                const _: () = ::core::assert!(#root::checks::selector_count(&#cmd_id) <= 1, #message);
            }
        };

        let positional_message = format!(
            "pound: `{owner}` has a positional or subcommand after a variadic or trailing \
             positional"
        );

        quote! {
            #(#default_asserts)*
            const #args_id: &[#root::ArgSpec] = &[ #(#args),* ];
            const #groups_id: &[#root::GroupSpec] = &[ #(#groups),* ];
            const #conflicts_id: &[(usize, usize)] = #conflicts;
            const #requires_id: &[(usize, usize)] = #requires;
            const #cmd_id: #root::CommandSpec = #head
                .args(#args_id)
                #flatten_calls
                .groups(#groups_id)
                .conflicts(#conflicts_id)
                .requires(#requires_id)
                .subs(#subs)
                #sub_optional_call;
            const _: () = ::core::assert!(#root::checks::names_unique(&#cmd_id), #unique_message);
            #(#group_asserts)*
            #selector_assert
            const _: () = ::core::assert!(
                #root::checks::positionals_reachable(&#cmd_id),
                #positional_message
            );
        }
    }

    // the builder calls that embed the flattened fields, absent when there are
    // none so plain commands keep the default order
    fn flatten_calls(&self, fields: &CommandFields) -> TokenStream {
        let root = &self.root;

        if fields.flattened.is_empty() {
            return TokenStream::new();
        }

        let specs = fields.flattened.iter().map(|field| {
            let ty = &field.ty;
            quote! { <#ty as #root::Parse>::SPEC }
        });

        let order = fields.order.iter().map(|entry| {
            match entry {
                Slot::Direct(i) => quote! { #root::ArgumentOrder::Direct(#i) },
                Slot::Flattened(i) => quote! { #root::ArgumentOrder::Flattened(#i) },
            }
        });

        quote! {
            .flattened(&[ #(#specs),* ])
            .argument_order(&[ #(#order),* ])
        }
    }

    fn readers(&self, fields: &CommandFields, m: &Ident, spec: &Ident) -> TokenStream {
        let root = &self.root;

        let args = fields
            .args
            .iter()
            .enumerate()
            .map(|(i, arg)| self.reader(arg, i, m, spec));

        let flattened = fields.flattened.iter().enumerate().map(|(i, field)| {
            let (ident, ty) = (&field.ident, &field.ty);

            quote! {
                #ident: <#ty as #root::Parse>::from_matches(
                    <#ty as #root::Parse>::SPEC,
                    #root::Matches::flattened(#m, #i),
                )?
            }
        });

        let sub = fields.sub.as_ref().map(|sub| {
            let (ident, ty) = (&sub.ident, &sub.ty);

            let build =
                quote! { <#ty as #root::Parse>::from_matches(<#ty as #root::Parse>::SPEC, #m)? };

            if sub.optional {
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
        });

        quote! { #(#args,)* #(#flattened,)* #sub }
    }

    // a const assertion that a declared default is one of its type's values. only
    // a `FromArg` conversion exposes a value list, so a custom parser gets none.
    fn default_assert(&self, arg: &Arg) -> Option<TokenStream> {
        let root = &self.root;
        let default = arg.default.as_ref()?;

        if !arg
            .conversion
            .as_ref()
            .is_some_and(Conversion::reads_from_arg)
        {
            return None;
        }

        let tokens = default.tokens();
        let inner = &arg.ty.inner;

        let message = match default {
            Text::Lit(text) => {
                format!(
                    "pound: default \"{text}\" is not one of the possible values for `{}`",
                    arg.ident
                )
            },
            Text::Expr(_) => {
                format!(
                    "pound: the default is not one of the possible values for `{}`",
                    arg.ident
                )
            },
        };

        Some(quote! {
            const _: () = ::core::assert!(
                #root::checks::default_allowed(#tokens, <#inner as #root::FromArg>::POSSIBLE),
                #message
            );
        })
    }

    fn arg(&self, arg: &Arg) -> TokenStream {
        let root = &self.root;

        let kind = match arg.kind {
            ArgKind::Flag => quote! { #root::Kind::Flag },
            ArgKind::Count => quote! { #root::Kind::Count },
            ArgKind::Opt => quote! { #root::Kind::Opt },
            ArgKind::Positional => quote! { #root::Kind::Positional },
            ArgKind::Trailing => quote! { #root::Kind::Trailing },
        };

        let mut e = quote! { #root::ArgSpec::new(#kind) };

        if let Some(l) = &arg.long {
            e = quote! { #e.long(#l) };
        }

        if let Some(c) = arg.short {
            e = quote! { #e.short(#c) };
        }

        if arg.required {
            e = quote! { #e.required() };
        }

        if arg.ty.shape == Shape::Many {
            e = quote! { #e.multi() };
        }

        if let Some(n) = arg.min_values {
            e = quote! { #e.min_values(#n) };
        }

        if let Some(n) = arg.max_values {
            e = quote! { #e.max_values(#n) };
        }

        if let Some(g) = &arg.group {
            e = quote! { #e.group(#g) };
        }

        if let Some(d) = &arg.default {
            let d = d.tokens();
            e = quote! { #e.default(#d) };
        }

        if let Some(dm) = &arg.default_missing {
            e = quote! { #e.default_missing(#dm) };
        }

        if let Some(ev) = &arg.env {
            e = quote! { #e.env(#ev) };
        }

        if let Some(n) = &arg.negate {
            e = quote! { #e.negate(#n) };
        }

        if !arg.aliases.is_empty() {
            let al = &arg.aliases;
            e = quote! { #e.aliases(&[ #(#al),* ]) };
        }

        if let Some(h) = arg.heading.as_ref().filter(|_| self.help) {
            e = quote! { #e.heading(#h) };
        }

        let vn = &arg.value_name;
        e = quote! { #e.value_name(#vn) };

        if arg
            .conversion
            .as_ref()
            .is_some_and(Conversion::reads_from_arg)
        {
            let inner = &arg.ty.inner;
            e = quote! {
                #e.possible_opt(<#inner as #root::FromArg>::POSSIBLE)
                    .hint(<#inner as #root::FromArg>::HINT)
            };
        }

        if arg.hidden {
            e = quote! { #e.hidden() };
        }

        if arg.global {
            e = quote! { #e.global() };
        }

        if let Some(complete) = &arg.complete {
            e = quote! { #e.complete(#complete) };
        }

        if let Some(hint) = arg.hint {
            let hint = match hint {
                ValueHint::Path => quote! { #root::Hint::Path },
            };
            e = quote! { #e.hint(#hint) };
        }

        if let Some(lh) = &arg.long_help {
            let lh = self.help_lit(lh);
            e = quote! { #e.long_help(#lh) };
        }

        let help = self.help_lit(&arg.help);
        quote! { #e.help(#help) }
    }

    fn reader(&self, arg: &Arg, i: usize, m: &Ident, spec: &Ident) -> TokenStream {
        let fname = &arg.ident;
        let inner = &arg.ty.inner;

        let body = match (arg.kind, &arg.conversion) {
            (ArgKind::Flag, _) => quote! { #m.switch(#spec, #i)? },
            (ArgKind::Count, _) => {
                let ty = &arg.ty.full;
                quote! { #m.count(#i) as #ty }
            },
            (_, Some(conversion)) => {
                let convert = self.conversion_closure(conversion, inner);
                match arg.ty.shape {
                    Shape::Bool | Shape::One => {
                        quote! { #m.required_map::<#inner>(#spec, #i, #convert)? }
                    },
                    Shape::Opt => quote! { #m.optional_map::<#inner>(#spec, #i, #convert)? },
                    Shape::Many => quote! { #m.many_map::<#inner>(#spec, #i, #convert)? },
                }
            },
            (_, None) => {
                match arg.ty.shape {
                    Shape::Bool | Shape::One => quote! { #m.required::<#inner>(#spec, #i)? },
                    Shape::Opt => quote! { #m.optional::<#inner>(#spec, #i)? },
                    Shape::Many => quote! { #m.many::<#inner>(#spec, #i)? },
                }
            },
        };

        quote! { #fname: #body }
    }

    fn conversion_closure(&self, conversion: &Conversion, inner: &TokenStream) -> TokenStream {
        let root = &self.root;
        let (sv, val, fail) = (local("s"), local("value"), local("msg"));

        let (parse, max_len, min, max, validate) = match conversion {
            Conversion::FromArg => {
                (
                    quote! { <#inner as #root::FromArg>::from_arg(#sv)? },
                    None,
                    None,
                    None,
                    None,
                )
            },
            Conversion::CheckedFromArg {
                min,
                max,
                max_len,
                validate,
            } => {
                (
                    quote! { <#inner as #root::FromArg>::from_arg(#sv)? },
                    max_len.as_ref().map(|max| self.max_len_check(max)),
                    min.as_deref().map(|v| self.bound_check(inner, "min", v)),
                    max.as_deref().map(|v| self.bound_check(inner, "max", v)),
                    validate.as_ref(),
                )
            },
            Conversion::CustomParse { parse, validate } => {
                (
                    quote! {
                        (#parse)(#sv).map_err(|#fail| #root::ValueError::new(#sv, #fail))?
                    },
                    None,
                    None,
                    None,
                    validate.as_ref(),
                )
            },
        };

        let validate = validate.map(|check| {
            quote! {
                if let ::core::result::Result::Err(#fail) = (#check)(&#val) {
                    return ::core::result::Result::Err(#root::ValueError::new(#sv, #fail));
                }
            }
        });

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

    fn max_len_check(&self, max: &TokenStream) -> TokenStream {
        let root = &self.root;
        let sv = local("s");

        quote! {
            if #sv.chars().count() > (#max) {
                return ::core::result::Result::Err(#root::ValueError::new(
                    #sv,
                    ::core::concat!("must be at most ", ::core::stringify!(#max), " chars"),
                ));
            }
        }
    }

    fn bound_check(&self, inner: &TokenStream, kind: &str, value: &str) -> TokenStream {
        let root = &self.root;
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

    // the `.about()` / `.long_about()` calls for a doc comment. the long form is
    // chained only when it says more than the summary already does.
    fn about_calls(&self, doc: &str) -> (TokenStream, TokenStream) {
        let short = self.help_lit(short_help(doc));
        let about = quote! { .about(#short) };

        if summary(doc) == doc {
            return (about, quote!());
        }

        let full = self.help_lit(doc);
        (about, quote! { .long_about(#full) })
    }

    // bake the help string only when the wrapping crate asks for help text,
    // otherwise emit "".
    fn help_lit(&self, s: &str) -> TokenStream {
        if self.help {
            quote! { #s }
        } else {
            quote! { "" }
        }
    }
}

// the `&[(usize, usize)]` token list for a set of index pairs.
fn index_pairs(pairs: &[(usize, usize)]) -> TokenStream {
    let items = pairs.iter().map(|(a, b)| quote! { (#a, #b) });
    quote! { &[ #(#items),* ] }
}

// mixed-site spans still let a user's unit struct or const capture a binding
// pattern or an item reference, so generated names also carry a prefix.
fn local(name: &str) -> Ident {
    Ident::new(&format!("__pound_{name}"), Span::mixed_site())
}

fn generated(name: &str) -> Ident {
    Ident::new(&format!("__POUND_{name}"), Span::mixed_site())
}

struct VariantTokens {
    consts:     TokenStream,
    spec:       TokenStream,
    arm:        TokenStream,
    /// the arm reads its args against `spec.subs[idx].spec`
    reads_spec: bool,
}
