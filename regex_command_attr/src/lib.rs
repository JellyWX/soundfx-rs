#![deny(rust_2018_idioms)]
#![deny(broken_intra_doc_links)]

use proc_macro::TokenStream;
use proc_macro2::Ident;
use quote::quote;
use syn::{parse::Error, parse_macro_input, parse_quote, spanned::Spanned, Lit};

pub(crate) mod attributes;
pub(crate) mod consts;
pub(crate) mod structures;

#[macro_use]
pub(crate) mod util;

use attributes::*;
use consts::*;
use structures::*;
use util::*;

macro_rules! match_options {
    ($v:expr, $values:ident, $options:ident, $span:expr => [$($name:ident);*]) => {
        match $v {
            $(
                stringify!($name) => $options.$name = propagate_err!($crate::attributes::parse($values)),
            )*
            _ => {
                return Error::new($span, format_args!("invalid attribute: {:?}", $v))
                    .to_compile_error()
                    .into();
            },
        }
    };
}

#[proc_macro_attribute]
pub fn command(attr: TokenStream, input: TokenStream) -> TokenStream {
    let mut fun = parse_macro_input!(input as CommandFun);

    let _name = if !attr.is_empty() {
        parse_macro_input!(attr as Lit).to_str()
    } else {
        fun.name.to_string()
    };

    let mut options = Options::new();

    for attribute in &fun.attributes {
        let span = attribute.span();
        let values = propagate_err!(parse_values(attribute));

        let name = values.name.to_string();
        let name = &name[..];

        match name {
            "arg" => options
                .cmd_args
                .push(propagate_err!(attributes::parse(values))),
            "example" => {
                options
                    .examples
                    .push(propagate_err!(attributes::parse(values)));
            }
            "description" => {
                let line: String = propagate_err!(attributes::parse(values));
                util::append_line(&mut options.description, line);
            }
            _ => {
                match_options!(name, values, options, span => [
                    aliases;
                    group;
                    required_permissions;
                    kind
                ]);
            }
        }
    }

    let Options {
        aliases,
        description,
        group,
        examples,
        required_permissions,
        kind,
        mut cmd_args,
    } = options;

    propagate_err!(create_declaration_validations(&mut fun));

    let res = parse_quote!(serenity::framework::standard::CommandResult);
    create_return_type_validation(&mut fun, res);

    let visibility = fun.visibility;
    let name = fun.name.clone();
    let body = fun.body;
    let ret = fun.ret;

    let n = name.with_suffix(COMMAND);

    let cooked = fun.cooked.clone();

    let command_path = quote!(crate::framework::Command);
    let arg_path = quote!(crate::framework::Arg);

    populate_fut_lifetimes_on_refs(&mut fun.args);
    let args = fun.args;

    let arg_idents = cmd_args
        .iter()
        .map(|arg| {
            n.with_suffix(arg.name.replace(" ", "_").replace("-", "_").as_str())
                .with_suffix(ARG)
        })
        .collect::<Vec<Ident>>();

    let mut tokens = cmd_args
        .iter_mut()
        .map(|arg| {
            let Arg {
                name,
                description,
                kind,
                required,
            } = arg;

            let an = n.with_suffix(name.as_str()).with_suffix(ARG);

            quote! {
                #(#cooked)*
                #[allow(missing_docs)]
                pub static #an: #arg_path = #arg_path {
                    name: #name,
                    description: #description,
                    kind: #kind,
                    required: #required,
                };
            }
        })
        .fold(quote! {}, |mut a, b| {
            a.extend(b);
            a
        });

    tokens.extend(quote! {
        #(#cooked)*
        #[allow(missing_docs)]
        pub static #n: #command_path = #command_path {
            fun: #name,
            names: &[#_name, #(#aliases),*],
            desc: #description,
            group: #group,
            examples: &[#(#examples),*],
            required_permissions: #required_permissions,
            kind: #kind,
            args: &[#(&#arg_idents),*],
        };

        #(#cooked)*
        #[allow(missing_docs)]
        #visibility fn #name<'fut> (#(#args),*) -> ::serenity::futures::future::BoxFuture<'fut, #ret> {
            use ::serenity::futures::future::FutureExt;

            async move {
                let _output: #ret = { #(#body)* };
                #[allow(unreachable_code)]
                _output
            }.boxed()
        }
    });

    tokens.into()
}
