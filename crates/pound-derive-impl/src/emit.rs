// SPDX-License-Identifier: EUPL-1.2

//! the tokens for a checked model: the static `CommandSpec`, its const
//! asserts, and the `from_matches` reader

use proc_macro2::{
    Ident,
    Span,
    TokenStream,
};
use quote::quote;

use crate::{
    Options,
    attr::{
        NumArgs,
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
        ExternalField,
        Header,
        Prose,
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
        let lead = options.root.starts_with("::").then(|| quote!(::));
        let segments = options
            .root
            .split("::")
            .filter(|segment| !segment.is_empty())
            .map(|segment| Ident::new(segment, Span::call_site()));

        Self {
            root: quote! { #lead #(#segments)::* },
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
        let (matches, sp) = (local("m"), local("spec"));
        let readers = self.readers(fields, &matches, &sp);
        let updates = self.updates(command, &matches, &sp, |field| quote! { self.#field });
        let named = named_fields(command).map(|field| quote! { let _ = &self.#field; });
        let (update_m, update_spec) = (matches.clone(), sp.clone());

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
                matches
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

                fn update_from_matches(
                    &mut self,
                    #update_spec: &'static #root::CommandSpec,
                    #update_m: &#root::Matches,
                ) -> ::core::result::Result<(), #root::Error> {
                    let _ = (#update_spec, #update_m);
                    #(#named)*
                    #updates
                    ::core::result::Result::Ok(())
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

        let sub_consts = built.iter().map(|variant| &variant.consts);
        let sub_specs = built.iter().map(|variant| &variant.spec);
        let arms = built.iter().map(|variant| &variant.arm);
        let update_arms = built.iter().map(|variant| &variant.update_arm);
        let picked = local("picked");

        let reading = built
            .iter()
            .enumerate()
            .filter(|(_, variant)| variant.reads_spec)
            .map(|(idx, _)| idx)
            .collect::<Vec<_>>();

        // `spec` is only read when some variant has its own args
        let spec_param = if reading.is_empty() {
            local("_spec")
        } else {
            spec_ident.clone()
        };

        let spec_assert = match reading[..] {
            [_, .., last] => quote! { ::core::assert!(#spec_ident.subs.len() > #last); },
            _ => quote! {},
        };

        let unique_message = format!("pound: two commands of `{name}` share a name or alias");
        let external_message =
            format!("pound: `{name}` has more than one #[pound(external)] variant");

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
                    const _: () = ::core::assert!(
                        #root::checks::externals(#subs_id) <= 1,
                        #external_message
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

                fn update_from_matches(
                    &mut self,
                    #spec_ident: &'static #root::CommandSpec,
                    #m_ident: &#root::Matches,
                ) -> ::core::result::Result<(), #root::Error> {
                    #spec_assert
                    let ::core::option::Option::Some((#picked, #sm)) = #root::Matches::sub(#m_ident) else {
                        return ::core::result::Result::Ok(());
                    };
                    let _ = #sm;
                    match (#picked, &mut *self) {
                        #(#update_arms)*
                        _ => {
                            #root::Matches::demand_sub(#m_ident, #spec_ident)?;
                            *self = <Self as #root::Parse>::from_matches(#spec_ident, #m_ident)?;
                        },
                    }
                    ::core::result::Result::Ok(())
                }
            }
        }
    }

    fn variant(&self, idx: usize, variant: &Variant, sm: &Ident, spec: &Ident) -> VariantTokens {
        let root = &self.root;
        let vname = &variant.ident;

        let named = match &variant.kind {
            VariantKind::Command(named) => named,
            VariantKind::External(ExternalField::List) => {
                let sp = local("s");
                return VariantTokens {
                    consts:     TokenStream::new(),
                    spec:       quote! { #root::SubSpec::external() },
                    update_arm: TokenStream::new(),
                    arm:        quote! {
                        ::core::option::Option::Some((#idx, #sm)) => {
                            let #sp = #spec.subs[#idx].spec;
                            ::core::result::Result::Ok(Self::#vname(#sm.many(#sp, 0)?))
                        },
                    },
                    reads_spec: true,
                };
            },
            VariantKind::External(ExternalField::Payload) => {
                return VariantTokens {
                    consts:     TokenStream::new(),
                    spec:       quote! { #root::SubSpec::external() },
                    update_arm: TokenStream::new(),
                    arm:        quote! {
                        ::core::option::Option::Some((#idx, #sm)) => {
                            ::core::result::Result::Ok(Self::#vname(#sm.external()))
                        },
                    },
                    reads_spec: false,
                };
            },
            VariantKind::Flatten(ty) => {
                let inner = local("inner");
                return VariantTokens {
                    consts:     TokenStream::new(),
                    spec:       quote! {
                        #root::SubSpec::flatten(#root::checks::subcommand_spec::<#ty>())
                    },
                    update_arm: quote! {
                        (#idx, Self::#vname(#inner)) => <#ty as #root::Parse>::update_from_matches(
                            #inner,
                            <#ty as #root::Parse>::SPEC,
                            #sm,
                        )?,
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

        let (name, aliases, fields) = (&named.name, &named.aliases, &named.command.fields);
        let ck = generated(&format!("CMD{idx}"));
        let prose_calls = self.prose_calls(&named.prose);

        let empty_call = named.help_if_empty.then(|| quote!(.help_if_empty()));
        let head = quote! {
            #root::CommandSpec::new(#name)
                #prose_calls
                #empty_call
        };

        let about = self.help_lit(short_help(&named.prose.doc));
        let hidden_call = named.hidden.then(|| quote!(.hidden()));
        let order_call = named.display_order.map(|n| quote! { .display_order(#n) });
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
            update_arm: self.update_arm(idx, vname, &named.command, sm, spec),
            consts: self.command_consts(&named.command, &head, &idx.to_string(), &ck),
            spec: quote! {
                #root::SubSpec::new(#name, &#ck)
                    .aliases(&[ #(#aliases),* ])
                    .about(#about)
                    #hidden_call
                    #order_call
            },
            arm,
            reads_spec,
        }
    }

    fn update_arm(
        &self,
        idx: usize,
        vname: &Ident,
        command: &Command,
        sm: &Ident,
        spec: &Ident,
    ) -> TokenStream {
        let fields = &command.fields;
        let sp = local("s");
        let bind = (!fields.args.is_empty()).then(|| quote! { let #sp = #spec.subs[#idx].spec; });
        let bindings = fields
            .args
            .iter()
            .map(|arg| &arg.ident)
            .chain(fields.flattened.iter().map(|flat| &flat.ident))
            .chain(fields.sub.iter().map(|sub| &sub.ident))
            .map(|field| {
                let bound = bound(field);
                quote! { #field: #bound }
            });
        let updates = self.updates(command, sm, &sp, |field| {
            let bound = bound(field);
            quote! { *#bound }
        });

        let this = local("this");
        let named = named_fields(command).map(|field| {
            quote! {
                let _ = |#this: &Self| match #this {
                    Self::#vname { #field: _, .. } => {},
                    #[allow(unreachable_patterns)]
                    _ => {},
                };
            }
        });

        quote! {
            (#idx, Self::#vname { #(#bindings,)* .. }) => {
                #(#named)*
                #bind
                #updates
            },
        }
    }

    /// a reference to `pound::attr::<key>` for each key written, under the
    /// key's own span
    pub fn key_refs(&self, keys: &[Ident]) -> TokenStream {
        let root = &self.root;

        if keys.is_empty() {
            return TokenStream::new();
        }

        // a value path, so rustc reports an unknown key at the key itself, the
        // one token here carrying the user's span
        let uses = keys.iter().map(|key| quote! { let _ = #root::attr::#key; });

        quote! {
            const _: () = {
                #(#uses)*
            };
        }
    }

    pub fn value_enum(&self, value_enum: &ValueEnum) -> TokenStream {
        let root = &self.root;
        let name = &value_enum.ident;

        let ignore_case = value_enum.ignore_case.then(|| quote!(.ignore_case()));
        let choices = value_enum.variants.iter().map(|variant| {
            let (label, aliases) = (&variant.label, &variant.aliases);
            let help = self.help_lit(&variant.help);
            let hidden = variant.hidden.then(|| quote!(.hidden()));
            quote! {
                #root::Choice::new(#label).aliases(&[ #(#aliases),* ]).help(#help) #hidden #ignore_case
            }
        });

        let arms = value_enum.variants.iter().enumerate().map(|(i, variant)| {
            let vname = &variant.ident;
            quote! { ::core::option::Option::Some(#i) => ::core::result::Result::Ok(Self::#vname), }
        });

        let spellings = value_enum.variants.iter().map(|variant| {
            let (vname, label) = (&variant.ident, &variant.label);
            quote! { Self::#vname => #label, }
        });

        let variants = value_enum.variants.iter().map(|variant| {
            let vname = &variant.ident;
            quote! { Self::#vname }
        });

        let (raw, listed, choice) = (local("s"), local("listed"), local("choice"));

        quote! {
            impl #root::FromArg for #name {
                const POSSIBLE: ::core::option::Option<&'static [#root::Choice]> =
                    ::core::option::Option::Some(&[ #(#choices),* ]);

                fn from_arg(#raw: &str) -> ::core::result::Result<Self, #root::ValueError> {
                    let #listed = <Self as #root::FromArg>::POSSIBLE.unwrap_or_default();
                    match #listed.iter().position(|#choice| #choice.answers(#raw)) {
                        #(#arms)*
                        _ => ::core::result::Result::Err(
                            #root::ValueError::new(#raw, "unrecognized value")
                        ),
                    }
                }
            }

            impl #name {
                #[doc(hidden)]
                pub const fn __pound_label(&self) -> &'static str {
                    match *self {
                        #(#spellings)*
                    }
                }
            }

            impl #root::ArgValue for #name {
                const ALL: &'static [Self] = &[ #(#variants),* ];

                fn as_str(&self) -> &'static str {
                    self.__pound_label()
                }
            }
        }
    }

    // without a `name`, a command shows the name the program runs as when it
    // parses argv, and the package name otherwise
    fn header(&self, header: &Header) -> TokenStream {
        let root = &self.root;

        let new = header.name.as_ref().map_or_else(
            || quote! { #root::CommandSpec::new(::core::env!("CARGO_PKG_NAME")).invoked_name() },
            |name| quote! { #root::CommandSpec::new(#name) },
        );

        let version = header.version.as_ref().map_or_else(
            || quote! { ::core::env!("CARGO_PKG_VERSION") },
            |tokens| tokens.iter().cloned().collect::<TokenStream>(),
        );

        let long_version = header.long_version.as_ref().map(|tokens| {
            let text = tokens.iter().cloned().collect::<TokenStream>();
            quote! { .long_version(#text) }
        });
        let prose = self.prose_calls(&header.prose);

        let empty = header.help_if_empty.then(|| quote!(.help_if_empty()));

        quote! {
            #new
                .version(#version)
                #long_version
                .hash_opt(::core::option_env!("POUND_GIT_HASH"))
                #prose
                #empty
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

        let (subs, sub_optional_call) = fields.sub.as_ref().map_or_else(
            || (quote! { &[] }, quote!()),
            |sub| {
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
        );

        let unique_message = format!("pound: two args of `{owner}` answer to the same spelling");

        let group_links = group_links(command);

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

        // an exclusive arg lifts the requirements of flattened fields too,
        // which only the spec can see
        let exclusive_assert = if fields.flattened.is_empty() {
            TokenStream::new()
        } else {
            let message = format!(
                "pound: `{owner}` has an exclusive arg beside a required field, which fails to \
                 read when the exclusive arg is given alone"
            );

            quote! {
                const _: () = ::core::assert!(#root::checks::exclusive_readable(&#cmd_id), #message);
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
            #group_links
            #selector_assert
            #exclusive_assert
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

    fn readers(&self, fields: &CommandFields, matches: &Ident, spec: &Ident) -> TokenStream {
        let root = &self.root;

        let args = fields.args.iter().enumerate().map(|(i, arg)| {
            let (field, value) = (&arg.ident, self.read(arg, i, matches, spec));
            quote! { #field: #value }
        });

        let flattened = fields.flattened.iter().enumerate().map(|(i, field)| {
            let (ident, ty) = (&field.ident, &field.ty);

            quote! {
                #ident: <#ty as #root::Parse>::from_matches(
                    <#ty as #root::Parse>::SPEC,
                    #root::Matches::flattened(#matches, #i),
                )?
            }
        });

        let sub = fields.sub.as_ref().map(|sub| {
            let (ident, ty) = (&sub.ident, &sub.ty);

            let build =
                quote! { <#ty as #root::Parse>::from_matches(<#ty as #root::Parse>::SPEC, #matches)? };

            if sub.optional {
                quote! {
                    #ident: if #root::Matches::sub(#matches).is_some() {
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

    // a const assertion that a declared default is one of its type's values.
    // only a `FromArg` conversion exposes a value list, so a custom parser
    // gets none.
    fn default_assert(&self, arg: &Arg) -> Option<TokenStream> {
        let root = &self.root;
        let default = arg.default.as_ref()?;

        if let Text::Value(path) = default {
            let (inner, value_tokens) =
                (&arg.ty.inner, path.iter().cloned().collect::<TokenStream>());
            return Some(quote! { const _: #inner = #value_tokens; });
        }

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
            Text::Expr(_) | Text::Value(_) => {
                format!(
                    "pound: the default is not one of the possible values for `{}`",
                    arg.ident
                )
            },
        };

        let delimiter = arg.delimiter.map_or_else(
            || quote!(::core::option::Option::None),
            |sep| quote!(::core::option::Option::Some(#sep)),
        );

        Some(quote! {
            const _: () = ::core::assert!(
                #root::checks::default_allowed(
                    #tokens,
                    #delimiter,
                    <#inner as #root::FromArg>::POSSIBLE,
                ),
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

        let spellings = Self::spelling_calls(arg);
        let counts = self.count_calls(arg);
        let mut spec = quote! { #root::ArgSpec::new(#kind) #spellings #counts };

        if arg.required {
            spec = quote! { #spec.required() };
        }

        if let Some(group) = &arg.group {
            spec = quote! { #spec.group(#group) };
        }

        if let Some(default) = &arg.default {
            let tokens = default.tokens();
            spec = quote! { #spec.default(#tokens) };
        }

        if let Some(dm) = &arg.default_missing {
            spec = quote! { #spec.default_missing(#dm) };
        }

        if let Some(ev) = &arg.env {
            spec = quote! { #spec.env(#ev) };
        }

        if let Some(heading) = arg.heading.as_ref().filter(|_| self.help) {
            spec = quote! { #spec.heading(#heading) };
        }

        if let Some(n) = arg.display_order {
            spec = quote! { #spec.display_order(#n) };
        }

        let vn = &arg.value_name;
        spec = quote! { #spec.value_name(#vn) };

        if arg
            .conversion
            .as_ref()
            .is_some_and(Conversion::reads_from_arg)
        {
            let inner = &arg.ty.inner;
            spec = quote! {
                #spec.possible_opt(<#inner as #root::FromArg>::POSSIBLE)
                    .hint(<#inner as #root::FromArg>::HINT)
            };
        }

        let hide = [
            (arg.hidden, quote! { .hidden() }),
            (arg.hide_possible, quote! { .hide_possible() }),
            (arg.hide_default, quote! { .hide_default() }),
            (arg.hide_env, quote! { .hide_env() }),
            (arg.hyphen_values, quote! { .hyphen_values() }),
        ]
        .into_iter()
        .filter_map(|(set, call)| set.then_some(call));
        spec = quote! { #spec #(#hide)* };

        if arg.global {
            spec = quote! { #spec.global() };
        }

        if arg.exclusive {
            spec = quote! { #spec.exclusive() };
        }

        if let Some(complete) = &arg.complete {
            spec = quote! { #spec.complete(#complete) };
        }

        if let Some(hint) = &arg.hint {
            let hint_tokens = match hint {
                ValueHint::Path => quote! { #root::Hint::Path },
                ValueHint::Variant(variant) => quote! { #root::Hint::#variant },
            };
            spec = quote! { #spec.hint(#hint_tokens) };
        }

        if let Some(long_help) = &arg.long_help {
            let long_help_text = self.help_lit(long_help);
            spec = quote! { #spec.long_help(#long_help_text) };
        }

        let help = self.help_lit(&arg.help);
        quote! { #spec.help(#help) }
    }

    fn spelling_calls(arg: &Arg) -> TokenStream {
        let long = arg.long.as_ref().map(|name| quote! { .long(#name) });
        let short = arg.short.map(|name| quote! { .short(#name) });
        let negate = arg.negate.as_ref().map(|name| quote! { .negate(#name) });
        let (longs, shorts) = (&arg.aliases, &arg.short_aliases);
        let aliases = (!longs.is_empty()).then(|| quote! { .aliases(&[ #(#longs),* ]) });
        let short_aliases =
            (!shorts.is_empty()).then(|| quote! { .short_aliases(&[ #(#shorts),* ]) });

        quote! { #long #short #negate #aliases #short_aliases }
    }

    fn count_calls(&self, arg: &Arg) -> TokenStream {
        let root = &self.root;
        let multi = arg.ty.shape.is_many().then(|| quote! { .multi() });
        let optional = (arg.ty.shape == Shape::OptOpt).then(|| quote! { .optional_value() });
        let min_values = arg.min_values.map(|n| quote! { .min_values(#n) });
        let max_values = arg.max_values.map(|n| quote! { .max_values(#n) });
        let delimiter = arg.delimiter.map(|sep| quote! { .delimiter(#sep) });
        let per_use = arg.num_args.map(|NumArgs { min, max }| {
            let arity = match max {
                Some(limit) if limit == min => quote! { #root::Arity::exactly(#min) },
                Some(limit) => quote! { #root::Arity::between(#min, #limit) },
                None => quote! { #root::Arity::at_least(#min) },
            };
            quote! { .per_use(#arity) }
        });

        quote! { #multi #optional #min_values #max_values #delimiter #per_use }
    }

    fn read(&self, arg: &Arg, i: usize, matches: &Ident, spec: &Ident) -> TokenStream {
        let inner = &arg.ty.inner;

        match (arg.kind, &arg.conversion) {
            (ArgKind::Flag, _) if arg.ty.sourced => quote! { #matches.switch_sourced(#spec, #i)? },
            (ArgKind::Flag, _) => quote! { #matches.switch(#spec, #i)? },
            (ArgKind::Count, _) => {
                let ty = &arg.ty.full;
                quote! { #matches.count(#i) as #ty }
            },
            (_, Some(conversion)) => {
                let convert = self.conversion_closure(conversion, inner);
                match arg.ty.shape {
                    Shape::One if arg.ty.sourced => {
                        quote! { #matches.required_sourced_map::<#inner>(#spec, #i, #convert)? }
                    },
                    Shape::Opt if arg.ty.sourced => {
                        quote! { #matches.optional_sourced_map::<#inner>(#spec, #i, #convert)? }
                    },
                    Shape::Bool | Shape::One => {
                        quote! { #matches.required_map::<#inner>(#spec, #i, #convert)? }
                    },
                    Shape::Opt => quote! { #matches.optional_map::<#inner>(#spec, #i, #convert)? },
                    Shape::Many => quote! { #matches.many_map::<#inner>(#spec, #i, #convert)? },
                    Shape::OptMany => {
                        quote! { #matches.optional_many_map::<#inner>(#spec, #i, #convert)? }
                    },
                    Shape::OptOpt => {
                        quote! { #matches.optional_value_map::<#inner>(#spec, #i, #convert)? }
                    },
                }
            },
            (_, None) => {
                match arg.ty.shape {
                    Shape::Bool | Shape::One => quote! { #matches.required::<#inner>(#spec, #i)? },
                    Shape::Opt => quote! { #matches.optional::<#inner>(#spec, #i)? },
                    Shape::Many => quote! { #matches.many::<#inner>(#spec, #i)? },
                    Shape::OptMany => quote! { #matches.optional_many::<#inner>(#spec, #i)? },
                    Shape::OptOpt => {
                        let root = &self.root;
                        let from_arg = quote! { <#inner as #root::FromArg>::from_arg };
                        quote! { #matches.optional_value_map::<#inner>(#spec, #i, #from_arg)? }
                    },
                }
            },
        }
    }

    fn updates(
        &self,
        command: &Command,
        matches: &Ident,
        spec: &Ident,
        place: impl Fn(&Ident) -> TokenStream,
    ) -> TokenStream {
        let root = &self.root;
        let fields = &command.fields;

        // an arg given on the line clears the others it shares a group or a
        // conflict with, which read back as if never given
        let args = fields.args.iter().enumerate().map(|(i, arg)| {
            let (field, value) = (place(&arg.ident), self.read(arg, i, matches, spec));
            let rivals = fields
                .args
                .iter()
                .enumerate()
                .filter(|&(j, other)| {
                    let grouped = arg.group.is_some() && other.group == arg.group;
                    let conflicting = command.conflicts.contains(&(i.min(j), i.max(j)));
                    j != i && (grouped || conflicting)
                })
                .map(|(j, _)| j);
            quote! {
                if #root::Matches::given(#matches, #i) #(|| #root::Matches::given(#matches, #rivals))* {
                    #field = #value;
                }
            }
        });

        let flattened = fields.flattened.iter().enumerate().map(|(i, flat)| {
            let (field, ty) = (place(&flat.ident), &flat.ty);
            quote! {
                <#ty as #root::Parse>::update_from_matches(
                    &mut #field,
                    <#ty as #root::Parse>::SPEC,
                    #root::Matches::flattened(#matches, #i),
                )?;
            }
        });

        let sub = fields.sub.as_ref().map(|sub| {
            let (field, ty) = (place(&sub.ident), &sub.ty);
            let existing = local("existing");
            let update = quote! {
                <#ty as #root::Parse>::update_from_matches(#existing, <#ty as #root::Parse>::SPEC, #matches)?
            };
            let build =
                quote! { <#ty as #root::Parse>::from_matches(<#ty as #root::Parse>::SPEC, #matches)? };

            if sub.optional {
                quote! {
                    if #root::Matches::sub(#matches).is_some() {
                        if let ::core::option::Option::Some(#existing) = &mut #field {
                            #update;
                        } else {
                            #root::Matches::demand_sub(#matches, <#ty as #root::Parse>::SPEC)?;
                            #field = ::core::option::Option::Some(#build);
                        }
                    }
                }
            } else {
                quote! {
                    let #existing = &mut #field;
                    #update;
                }
            }
        });

        quote! { #(#args)* #(#flattened)* #sub }
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
                    max_len.as_ref().map(|bound| self.max_len_check(bound)),
                    min.as_deref()
                        .map(|bound| self.bound_check(inner, "min", bound)),
                    max.as_deref()
                        .map(|bound| self.bound_check(inner, "max", bound)),
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

        let validation_tokens = validate.map(|check| {
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
                #validation_tokens
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

    // the long form is chained only when it says more than the summary
    // already does
    fn prose_calls(&self, prose: &Prose) -> TokenStream {
        let call = |name: &str, text: &str| {
            let method = Ident::new(name, Span::call_site());
            let lit = self.help_lit(text);
            quote! { .#method(#lit) }
        };

        let doc = prose.doc.as_str();
        let about = call("about", short_help(doc));
        let long = (summary(doc) != doc).then(|| call("long_about", doc));
        let before = prose
            .before_help
            .as_deref()
            .map(|text| call("before_help", text));
        let after = prose
            .after_help
            .as_deref()
            .map(|text| call("after_help", text));

        quote! { #about #long #before #after }
    }

    // bake the help string only when the wrapping crate asks for help text,
    // otherwise emit "".
    fn help_lit(&self, help: &str) -> TokenStream {
        if self.help {
            quote! { #help }
        } else {
            quote! { "" }
        }
    }
}

// the `&[(usize, usize)]` token list for a set of index pairs.
fn index_pairs(pairs: &[(usize, usize)]) -> TokenStream {
    let items = pairs.iter().map(|(left, right)| quote! { (#left, #right) });
    quote! { &[ #(#items),* ] }
}

// mixed-site spans still let a user's unit struct or const capture a binding
// pattern or an item reference, so generated names also carry a prefix.
fn local(name: &str) -> Ident {
    Ident::new(&format!("__pound_{name}"), Span::mixed_site())
}

// the fields a `requires` or `conflicts_with` names, read back under the
// user's own spans so the compiler and rust-analyzer resolve each one
fn named_fields(command: &Command) -> impl Iterator<Item = &Ident> {
    command
        .fields
        .args
        .iter()
        .flat_map(|arg| arg.requires.iter().chain(&arg.conflicts_with))
}

// each bare group name bound once as a local under its first mention's span and
// read under the others', so rename and go-to-definition reach every mention
fn group_links(command: &Command) -> TokenStream {
    let mentions = command
        .required_refs
        .iter()
        .chain(command.fields.args.iter().flat_map(|arg| &arg.group_refs))
        .collect::<Vec<_>>();

    let links = mentions.iter().enumerate().filter_map(|(at, first)| {
        if mentions[..at].iter().any(|earlier| earlier == first) {
            return None;
        }

        let rest = mentions[at + 1..].iter().filter(|later| *later == first);

        Some(quote! {
            const _: () = {
                #[allow(non_snake_case, unused_variables)]
                let #first = ();
                #(let _ = #rest;)*
            };
        })
    });

    quote! { #(#links)* }
}

// a variant's field rebound under a name the user's own closures can't see or
// shadow
fn bound(field: &Ident) -> Ident {
    let name = field.to_string();
    local(&format!("field_{}", name.trim_start_matches("r#")))
}

fn generated(name: &str) -> Ident {
    Ident::new(&format!("__POUND_{name}"), Span::mixed_site())
}

struct VariantTokens {
    consts:     TokenStream,
    spec:       TokenStream,
    arm:        TokenStream,
    /// empty when naming the variant again replaces it
    update_arm: TokenStream,
    /// the arm reads its args against `spec.subs[idx].spec`
    reads_spec: bool,
}
