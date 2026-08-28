use darling::FromMeta;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Attribute, Ident, Path, Token, Type, parenthesized,
    parse::{Parse, ParseStream},
};

use crate::{attributes::HelperAttributes, guest::GuestMacroError, naming::Naming};

mod kw {
    syn::custom_keyword!(value);
}

#[derive(Default, FromMeta)]
#[darling(default)]
struct MemberOptions {
    name: Option<String>,
}

#[derive(Debug)]
pub(crate) struct GuestParameter {
    ident: Ident,
    ty: Type,
}

impl Parse for GuestParameter {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let ident = input.parse::<Ident>()?;

        input.parse::<Token![:]>()?;

        Ok(Self { ident, ty: input.parse::<Type>()? })
    }
}

#[derive(Debug)]
pub(crate) enum RawGuestMember {
    Function {
        attributes: Vec<Attribute>,
        ident: Ident,
        parameters: Vec<GuestParameter>,
        descriptor: Type,
    },
    Value {
        attributes: Vec<Attribute>,
        ident: Ident,
        descriptor: Type,
    },
}

impl Parse for RawGuestMember {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let attributes = input.call(Attribute::parse_outer)?;

        if input.peek(Token![fn]) {
            input.parse::<Token![fn]>()?;

            let ident = input.parse::<Ident>()?;
            let content;

            parenthesized!(content in input);

            let parameters = content
                .parse_terminated(GuestParameter::parse, Token![,])?
                .into_iter()
                .collect();

            input.parse::<Token![->]>()?;

            let descriptor = input.parse::<Type>()?;

            input.parse::<Token![;]>()?;

            Ok(Self::Function {
                attributes,
                ident,
                parameters,
                descriptor,
            })
        } else if input.peek(kw::value) {
            input.parse::<kw::value>()?;

            let ident = input.parse::<Ident>()?;

            input.parse::<Token![:]>()?;

            let descriptor = input.parse::<Type>()?;

            input.parse::<Token![;]>()?;

            Ok(Self::Value { attributes, ident, descriptor })
        } else {
            Err(input.error("expected `fn` or `value` in a guest declaration member"))
        }
    }
}

pub(crate) enum GuestMember {
    Function {
        ident: Ident,
        guest_name: String,
        parameters: Vec<GuestParameter>,
        descriptor: Type,
    },
    Value {
        ident: Ident,
        guest_name: String,
        descriptor: Type,
    },
}

impl GuestMember {
    pub(crate) fn resolve(raw: RawGuestMember) -> Result<Self, GuestMacroError> {
        match raw {
            RawGuestMember::Function {
                mut attributes,
                ident,
                parameters,
                descriptor,
            } => Ok(Self::Function {
                guest_name: Naming::member(
                    &ident,
                    MemberOptions::from_list(&HelperAttributes::take(&mut attributes)?)?.name,
                    None,
                ),
                ident,
                parameters,
                descriptor,
            }),
            RawGuestMember::Value { mut attributes, ident, descriptor } => Ok(Self::Value {
                guest_name: Naming::member(
                    &ident,
                    MemberOptions::from_list(&HelperAttributes::take(&mut attributes)?)?.name,
                    None,
                ),
                ident,
                descriptor,
            }),
        }
    }

    pub(crate) fn render(&self, krate: &Path, receiver: &Ident) -> TokenStream {
        match self {
            Self::Function {
                ident,
                guest_name,
                parameters,
                descriptor,
            } => {
                let names = parameters
                    .iter()
                    .map(|parameter| &parameter.ident);
                let types = parameters
                    .iter()
                    .map(|parameter| &parameter.ty)
                    .collect::<Vec<_>>();
                let arguments = parameters
                    .iter()
                    .map(|parameter| &parameter.ident);

                quote! {
                    pub fn #ident(&self, #(#names: #types),*)
                        -> ::core::result::Result<
                            <#descriptor as #krate::marshal::FromGuest<B>>::Owned,
                            #krate::errors::Error,
                        >
                    where
                        (#(#types,)*) : #krate::marshal::args::ToGuestArgs<B>,
                        #descriptor: #krate::marshal::FromGuest<B>,
                    {
                        self.#receiver.call::<_, #descriptor>(
                            #guest_name,
                            (#(#arguments,)*),
                        )
                    }
                }
            }
            Self::Value { ident, guest_name, descriptor } => quote! {
                pub fn #ident(&self)
                    -> ::core::result::Result<
                        <#descriptor as #krate::marshal::FromGuest<B>>::Owned,
                        #krate::errors::Error,
                    >
                where
                    #descriptor: #krate::marshal::FromGuest<B>,
                {
                    self.#receiver.get::<#descriptor>(#guest_name)
                }
            },
        }
    }
}
