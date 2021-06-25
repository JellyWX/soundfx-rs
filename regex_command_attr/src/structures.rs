use proc_macro2::TokenStream as TokenStream2;
use quote::{quote, ToTokens};
use syn::{
    braced,
    parse::{Error, Parse, ParseStream, Result},
    spanned::Spanned,
    Attribute, Block, FnArg, Ident, Pat, ReturnType, Stmt, Token, Type, Visibility,
};

use crate::util::{self, Argument, Parenthesised};

fn parse_argument(arg: FnArg) -> Result<Argument> {
    match arg {
        FnArg::Typed(typed) => {
            let pat = typed.pat;
            let kind = typed.ty;

            match *pat {
                Pat::Ident(id) => {
                    let name = id.ident;
                    let mutable = id.mutability;

                    Ok(Argument {
                        mutable,
                        name,
                        kind: *kind,
                    })
                }
                Pat::Wild(wild) => {
                    let token = wild.underscore_token;

                    let name = Ident::new("_", token.spans[0]);

                    Ok(Argument {
                        mutable: None,
                        name,
                        kind: *kind,
                    })
                }
                _ => Err(Error::new(
                    pat.span(),
                    format_args!("unsupported pattern: {:?}", pat),
                )),
            }
        }
        FnArg::Receiver(_) => Err(Error::new(
            arg.span(),
            format_args!("`self` arguments are prohibited: {:?}", arg),
        )),
    }
}

/// Test if the attribute is cooked.
fn is_cooked(attr: &Attribute) -> bool {
    const COOKED_ATTRIBUTE_NAMES: &[&str] = &[
        "cfg", "cfg_attr", "derive", "inline", "allow", "warn", "deny", "forbid",
    ];

    COOKED_ATTRIBUTE_NAMES.iter().any(|n| attr.path.is_ident(n))
}

/// Removes cooked attributes from a vector of attributes. Uncooked attributes are left in the vector.
///
/// # Return
///
/// Returns a vector of cooked attributes that have been removed from the input vector.
fn remove_cooked(attrs: &mut Vec<Attribute>) -> Vec<Attribute> {
    let mut cooked = Vec::new();

    // FIXME: Replace with `Vec::drain_filter` once it is stable.
    let mut i = 0;
    while i < attrs.len() {
        if !is_cooked(&attrs[i]) {
            i += 1;
            continue;
        }

        cooked.push(attrs.remove(i));
    }

    cooked
}

#[derive(Debug)]
pub struct CommandFun {
    /// `#[...]`-style attributes.
    pub attributes: Vec<Attribute>,
    /// Populated cooked attributes. These are attributes outside of the realm of this crate's procedural macros
    /// and will appear in generated output.
    pub cooked: Vec<Attribute>,
    pub visibility: Visibility,
    pub name: Ident,
    pub args: Vec<Argument>,
    pub ret: Type,
    pub body: Vec<Stmt>,
}

impl Parse for CommandFun {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut attributes = input.call(Attribute::parse_outer)?;

        // Rename documentation comment attributes (`#[doc = "..."]`) to `#[description = "..."]`.
        util::rename_attributes(&mut attributes, "doc", "description");

        let cooked = remove_cooked(&mut attributes);

        let visibility = input.parse::<Visibility>()?;

        input.parse::<Token![async]>()?;

        input.parse::<Token![fn]>()?;
        let name = input.parse()?;

        // (...)
        let Parenthesised(args) = input.parse::<Parenthesised<FnArg>>()?;

        let ret = match input.parse::<ReturnType>()? {
            ReturnType::Type(_, t) => (*t).clone(),
            ReturnType::Default => {
                return Err(input
                    .error("expected a result type of either `CommandResult` or `CheckResult`"))
            }
        };

        // { ... }
        let bcont;
        braced!(bcont in input);
        let body = bcont.call(Block::parse_within)?;

        let args = args
            .into_iter()
            .map(parse_argument)
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            attributes,
            cooked,
            visibility,
            name,
            args,
            ret,
            body,
        })
    }
}

impl ToTokens for CommandFun {
    fn to_tokens(&self, stream: &mut TokenStream2) {
        let Self {
            attributes: _,
            cooked,
            visibility,
            name,
            args,
            ret,
            body,
        } = self;

        stream.extend(quote! {
            #(#cooked)*
            #visibility async fn #name (#(#args),*) -> #ret {
                #(#body)*
            }
        });
    }
}

#[derive(Debug)]
pub enum PermissionLevel {
    Unrestricted,
    Managed,
    Restricted,
}

impl Default for PermissionLevel {
    fn default() -> Self {
        Self::Unrestricted
    }
}

impl PermissionLevel {
    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s.to_uppercase().as_str() {
            "UNRESTRICTED" => Self::Unrestricted,
            "MANAGED" => Self::Managed,
            "RESTRICTED" => Self::Restricted,
            _ => return None,
        })
    }
}

impl ToTokens for PermissionLevel {
    fn to_tokens(&self, stream: &mut TokenStream2) {
        let path = quote!(crate::framework::PermissionLevel);
        let variant;

        match self {
            Self::Unrestricted => {
                variant = quote!(Unrestricted);
            }

            Self::Managed => {
                variant = quote!(Managed);
            }

            Self::Restricted => {
                variant = quote!(Restricted);
            }
        }

        stream.extend(quote! {
            #path::#variant
        });
    }
}

#[derive(Debug)]
pub(crate) enum ApplicationCommandOptionType {
    SubCommand,
    SubCommandGroup,
    String,
    Integer,
    Boolean,
    User,
    Channel,
    Role,
    Mentionable,
    Unknown,
}

impl ApplicationCommandOptionType {
    pub fn from_str(s: String) -> Self {
        match s.as_str() {
            "SubCommand" => Self::SubCommand,
            "SubCommandGroup" => Self::SubCommandGroup,
            "String" => Self::String,
            "Integer" => Self::Integer,
            "Boolean" => Self::Boolean,
            "User" => Self::User,
            "Channel" => Self::Channel,
            "Role" => Self::Role,
            "Mentionable" => Self::Mentionable,
            _ => Self::Unknown,
        }
    }
}

impl ToTokens for ApplicationCommandOptionType {
    fn to_tokens(&self, stream: &mut TokenStream2) {
        let path = quote!(serenity::model::interactions::ApplicationCommandOptionType);
        let variant = match self {
            ApplicationCommandOptionType::SubCommand => quote!(SubCommand),
            ApplicationCommandOptionType::SubCommandGroup => quote!(SubCommandGroup),
            ApplicationCommandOptionType::String => quote!(String),
            ApplicationCommandOptionType::Integer => quote!(Integer),
            ApplicationCommandOptionType::Boolean => quote!(Boolean),
            ApplicationCommandOptionType::User => quote!(User),
            ApplicationCommandOptionType::Channel => quote!(Channel),
            ApplicationCommandOptionType::Role => quote!(Role),
            ApplicationCommandOptionType::Mentionable => quote!(Mentionable),
            ApplicationCommandOptionType::Unknown => quote!(Unknown),
        };

        stream.extend(quote! {
            #path::#variant
        });
    }
}

#[derive(Debug)]
pub(crate) struct Arg {
    pub name: String,
    pub description: String,
    pub kind: ApplicationCommandOptionType,
    pub required: bool,
}

impl Default for Arg {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            kind: ApplicationCommandOptionType::String,
            required: false,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct Options {
    pub aliases: Vec<String>,
    pub description: String,
    pub group: String,
    pub examples: Vec<String>,
    pub required_permissions: PermissionLevel,
    pub allow_slash: bool,
    pub cmd_args: Vec<Arg>,
}

impl Options {
    #[inline]
    pub fn new() -> Self {
        Self {
            allow_slash: true,
            group: "Other".to_string(),
            ..Default::default()
        }
    }
}
