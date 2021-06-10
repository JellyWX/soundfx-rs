#![deny(rust_2018_idioms)]
#![deny(broken_intra_doc_links)]

use proc_macro::TokenStream;
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

/// The heart of the attribute-based framework.
///
/// This is a function attribute macro. Using this on other Rust constructs won't work.
///
/// ## Options
///
/// To alter how the framework will interpret the command,
/// you can provide options as attributes following this `#[command]` macro.
///
/// Each option has its own kind of data to stock and manipulate with.
/// They're given to the option either with the `#[option(...)]` or `#[option = ...]` syntaxes.
/// If an option doesn't require for any data to be supplied, then it's simply an empty `#[option]`.
///
/// If the input to the option is malformed, the macro will give you can error, describing
/// the correct method for passing data, and what it should be.
///
/// The list of available options, is, as follows:
///
/// | Syntax                                                                       | Description                                                                                              | Argument explanation                                                                                                                                                                                                             |
/// | ---------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
/// | `#[checks(identifiers)]`                                                     | Preconditions that must met before the command's execution.                                              | `identifiers` is a comma separated list of identifiers referencing functions marked by the `#[check]` macro                                                                                                                      |
/// | `#[aliases(names)]`                                                          | Alternative names to refer to this command.                                                              | `names` is a comma separated list of desired aliases.                                                                                                                                                                             |
/// | `#[description(desc)]` </br> `#[description = desc]`                         | The command's description or summary.                                                                    | `desc` is a string describing the command.                                                                                                                                                                                       |
/// | `#[usage(use)]` </br> `#[usage = use]`                                       | The command's intended usage.                                                                            | `use` is a string stating the schema for the command's usage.                                                                                                                                                                    |
/// | `#[example(ex)]` </br> `#[example = ex]`                                     | An example of the command's usage. May be called multiple times to add many examples at once.            | `ex` is a string                                                                                                                                                                                                                 |
/// | `#[delimiters(delims)]`                                                      | Argument delimiters specific to this command. Overrides the global list of delimiters in the framework.  | `delims` is a comma separated list of strings |
/// | `#[min_args(min)]` </br> `#[max_args(max)]` </br> `#[num_args(min_and_max)]` | The expected length of arguments that the command must receive in order to function correctly.           | `min`, `max` and `min_and_max` are 16-bit, unsigned integers.                                                                                                                                                                    |
/// | `#[required_permissions(perms)]`                                             | Set of permissions the user must possess.                                                                | `perms` is a comma separated list of permission names.</br> These can be found at [Discord's official documentation](https://discord.com/developers/docs/topics/permissions).                                                 |
/// | `#[allowed_roles(roles)]`                                                    | Set of roles the user must possess.                                                                      | `roles` is a comma separated list of role names.                                                                                                                                                                                 |
/// | `#[help_available]` </br> `#[help_available(b)]`                             | If the command should be displayed in the help message.                                                  | `b` is a boolean. If no boolean is provided, the value is assumed to be `true`.                                                                                                                                                  |
/// | `#[only_in(ctx)]`                                                            | Which environment the command can be executed in.                                                        | `ctx` is a string with the accepted values `guild`/`guilds` and `dm`/`dms` (Direct Message).                                                                                                                                     |
/// | `#[bucket(name)]` </br> `#[bucket = name]`                                   | What bucket will impact this command.                                                                    | `name` is a string containing the bucket's name.</br> Refer to [the bucket example in the standard framework](https://docs.rs/serenity/*/serenity/framework/standard/struct.StandardFramework.html#method.bucket) for its usage. |
/// | `#[owners_only]` </br> `#[owners_only(b)]`                                   | If this command is exclusive to owners.                                                                  | `b` is a boolean. If no boolean is provided, the value is assumed to be `true`.                                                                                                                                                  |
/// | `#[owner_privilege]` </br> `#[owner_privilege(b)]`                           | If owners can bypass certain options.                                                                    | `b` is a boolean. If no boolean is provided, the value is assumed to be `true`.                                                                                                                                                  |
/// | `#[sub_commands(commands)]`                                                  | The sub or children commands of this command. They are executed in the form: `this-command sub-command`. | `commands` is a comma separated list of identifiers referencing functions marked by the `#[command]` macro.                                                                                                                      |
///
/// Documentation comments (`///`) applied onto the function are interpreted as sugar for the
/// `#[description]` option. When more than one application of the option is performed,
/// the text is delimited by newlines. This mimics the behaviour of regular doc-comments,
/// which are sugar for the `#[doc = "..."]` attribute. If you wish to join lines together,
/// however, you have to end the previous lines with `\$`.
///
/// # Notes
/// The name of the command is parsed from the applied function,
/// or may be specified inside the `#[command]` attribute, a lá `#[command("foobar")]`.
///
/// This macro attribute generates static instances of `Command` and `CommandOptions`,
/// conserving the provided options.
///
/// The names of the instances are all uppercased names of the command name.
/// For example, with a name of "foo":
/// ```rust,ignore
/// pub static FOO_COMMAND_OPTIONS: CommandOptions = CommandOptions { ... };
/// pub static FOO_COMMAND: Command = Command { options: FOO_COMMAND_OPTIONS, ... };
/// ```
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
                    usage;
                    required_permissions;
                    allow_slash
                ]);
            }
        }
    }

    let Options {
        aliases,
        description,
        usage,
        examples,
        required_permissions,
        allow_slash,
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

    populate_fut_lifetimes_on_refs(&mut fun.args);
    let args = fun.args;

    (quote! {
        #(#cooked)*
        #[allow(missing_docs)]
        pub static #n: #command_path = #command_path {
            fun: #name,
            names: &[#_name, #(#aliases),*],
            desc: #description,
            usage: #usage,
            examples: &[#(#examples),*],
            required_permissions: #required_permissions,
            allow_slash: #allow_slash,
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
    })
    .into()
}

/// A macro that transforms `async` functions (and closures) into plain functions, whose
/// return type is a boxed [`Future`].
///
/// # Transformation
///
/// The macro transforms an `async` function, which may look like this:
///
/// ```rust,no_run
/// async fn foo(n: i32) -> i32 {
///     n + 4
/// }
/// ```
///
/// into this (some details omitted):
///
/// ```rust,no_run
/// use std::future::Future;
/// use std::pin::Pin;
///
/// fn foo(n: i32) -> Pin<Box<dyn std::future::Future<Output = i32>>> {
///     Box::pin(async move {
///         n + 4
///     })
/// }
/// ```
///
/// This transformation also applies to closures, which are converted more simply. For instance,
/// this closure:
///
/// ```rust,no_run
/// # #![feature(async_closure)]
/// #
/// async move |x: i32| {
///     x * 2 + 4
/// }
/// # ;
/// ```
///
/// is changed to:
///
/// ```rust,no_run
/// |x: i32| {
///     Box::pin(async move {
///         x * 2 + 4
///     })
/// }
/// # ;
/// ```
///
/// ## How references are handled
///
/// When a function contains references, their lifetimes are constrained to the returned
/// [`Future`]. If the above `foo` function had `&i32` as a parameter, the transformation would be
/// instead this:
///
/// ```rust,no_run
/// use std::future::Future;
/// use std::pin::Pin;
///
/// fn foo<'fut>(n: &'fut i32) -> Pin<Box<dyn std::future::Future<Output = i32> + 'fut>> {
///     Box::pin(async move {
///         *n + 4
///     })
/// }
/// ```
///
/// Explicitly specifying lifetimes (in the parameters or in the return type) or complex usage of
/// lifetimes (e.g. `'a: 'b`) is not supported.
///
/// # Necessity for the macro
///
/// The macro performs the transformation to permit the framework to store and invoke the functions.
///
/// Functions marked with the `async` keyword will wrap their return type with the [`Future`] trait,
/// which a state-machine generated by the compiler for the function will implement. This complicates
/// matters for the framework, as [`Future`] is a trait. Depending on a type that implements a trait
/// is done with two methods in Rust:
///
/// 1. static dispatch - generics
/// 2. dynamic dispatch - trait objects
///
/// First method is infeasible for the framework. Typically, the framework will contain a plethora
/// of different commands that will be stored in a single list. And due to the nature of generics,
/// generic types can only resolve to a single concrete type. If commands had a generic type for
/// their function's return type, the framework would be unable to store commands, as only a single
/// [`Future`] type from one of the commands would get resolved, preventing other commands from being
/// stored.
///
/// Second method involves heap allocations, but is the only working solution. If a trait is
/// object-safe (which [`Future`] is), the compiler can generate a table of function pointers
/// (a vtable) that correspond to certain implementations of the trait. This allows to decide
/// which implementation to use at runtime. Thus, we can use the interface for the [`Future`] trait,
/// and avoid depending on the underlying value (such as its size). To opt-in to dynamic dispatch,
/// trait objects must be used with a pointer, like references (`&` and `&mut`) or `Box`. The
/// latter is what's used by the macro, as the ownership of the value (the state-machine) must be
/// given to the caller, the framework in this case.
///
/// The macro exists to retain the normal syntax of `async` functions (and closures), while
/// granting the user the ability to pass those functions to the framework, like command functions
/// and hooks (`before`, `after`, `on_dispatch_error`, etc.).
///
/// # Notes
///
/// If applying the macro on an `async` closure, you will need to enable the `async_closure`
/// feature. Inputs to procedural macro attributes must be valid Rust code, and `async`
/// closures are not stable yet.
///
/// [`Future`]: std::future::Future
#[proc_macro_attribute]
pub fn hook(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let hook = parse_macro_input!(input as Hook);

    match hook {
        Hook::Function(mut fun) => {
            let cooked = fun.cooked;
            let visibility = fun.visibility;
            let fun_name = fun.name;
            let body = fun.body;
            let ret = fun.ret;

            populate_fut_lifetimes_on_refs(&mut fun.args);
            let args = fun.args;

            (quote! {
                #(#cooked)*
                #[allow(missing_docs)]
                #visibility fn #fun_name<'fut>(#(#args),*) -> ::serenity::futures::future::BoxFuture<'fut, #ret> {
                    use ::serenity::futures::future::FutureExt;

                    async move {
                        let _output: #ret = { #(#body)* };
                        #[allow(unreachable_code)]
                        _output
                    }.boxed()
                }
            })
                .into()
        }
        Hook::Closure(closure) => {
            let cooked = closure.cooked;
            let args = closure.args;
            let ret = closure.ret;
            let body = closure.body;

            (quote! {
                #(#cooked)*
                |#args| #ret {
                    use ::serenity::futures::future::FutureExt;

                    async move { #body }.boxed()
                }
            })
            .into()
        }
    }
}
