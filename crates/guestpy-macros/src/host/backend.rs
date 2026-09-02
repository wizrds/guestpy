use darling::FromMeta;
use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::{
    GenericParam,
    Generics,
    Ident,
    ItemImpl,
    Meta,
    MetaList,
    Token,
    Type,
    TypeParam,
    parse::{ParseStream, Parser},
    parse_quote,
    spanned::Spanned,
};

use crate::host::HostMacroError;

pub(crate) enum BackendOption {
    Named(Ident),
    Pinned(Type),
}

impl BackendOption {
    fn pinned(list: &MetaList) -> darling::Result<Type> {
        Parser::parse2(
            |input: ParseStream| {
                let key = input.parse::<Ident>()?;

                if key != "pin" {
                    return Err(syn::Error::new(
                        key.span(),
                        "`backend(...)` accepts only `pin = <type>`",
                    ));
                }

                input.parse::<Token![=]>()?;

                let pinned = input.parse::<Type>()?;

                if !input.is_empty() {
                    return Err(input.error("`backend(...)` accepts only `pin = <type>`"));
                }

                Ok(pinned)
            },
            list.tokens.clone(),
        )
        .map_err(|error| darling::Error::from(error).with_span(list))
    }
}

impl FromMeta for BackendOption {
    fn from_meta(item: &Meta) -> darling::Result<Self> {
        match item {
            Meta::NameValue(pair) => syn::parse2::<Ident>(pair.value.to_token_stream())
                .map(Self::Named)
                .map_err(|_| {
                    darling::Error::custom(
                        "`backend = <name>` names the backend type parameter; pin a concrete \
                         backend with `backend(pin = <type>)`",
                    )
                    .with_span(&pair.value)
                }),
            Meta::List(list) => Self::pinned(list).map(Self::Pinned),
            Meta::Path(_) => Err(darling::Error::unsupported_format("word").with_span(item)),
        }
    }
}

pub(crate) enum BackendParameter {
    Synthesized(Ident),
    Declared(Ident),
    Introduced(Ident),
    Concrete(Type),
}

impl BackendParameter {
    pub(crate) fn resolve(
        declared: Option<BackendOption>,
        item: &ItemImpl,
        attribute: &str,
    ) -> Result<Self, HostMacroError> {
        match declared {
            Some(BackendOption::Named(ident)) => {
                return Ok(match Self::declares_parameter(&item.generics, &ident.to_string()) {
                    true => Self::Declared(ident),
                    false => Self::Introduced(ident),
                });
            }
            Some(BackendOption::Pinned(pinned)) => return Ok(Self::Concrete(pinned)),
            None => {}
        }

        if let Some(ident) = Self::backend_bounded_parameter(&item.generics) {
            return Err(syn::Error::new(
                item.self_ty.span(),
                format!(
                    "#[{attribute}] requires `backend = {ident}` because `{ident}` is \
                     bounded by `Backend`",
                ),
            )
            .into());
        }

        Ok(Self::Synthesized(
            match Self::declares_parameter(&item.generics, "B") {
                true => parse_quote!(__GuestpyBackend),
                false => parse_quote!(B),
            },
        ))
    }

    fn parameters(generics: &Generics) -> impl Iterator<Item = &TypeParam> {
        generics
            .params
            .iter()
            .filter_map(|parameter| match parameter {
                GenericParam::Type(parameter) => Some(parameter),
                _ => None,
            })
    }

    fn declares_parameter(generics: &Generics, name: &str) -> bool {
        Self::parameters(generics).any(|parameter| parameter.ident == name)
    }

    fn backend_bounded_parameter(generics: &Generics) -> Option<Ident> {
        Self::parameters(generics)
            .find(|parameter| {
                parameter.bounds.iter().any(|bound| match bound {
                    syn::TypeParamBound::Trait(bound) => bound
                        .path
                        .segments
                        .last()
                        .is_some_and(|segment| segment.ident == "Backend"),
                    _ => false,
                })
            })
            .map(|parameter| parameter.ident.clone())
    }

    pub(crate) fn ty(&self) -> Type {
        match self {
            Self::Synthesized(ident) | Self::Declared(ident) | Self::Introduced(ident) => {
                parse_quote!(#ident)
            }
            Self::Concrete(pinned) => pinned.clone(),
        }
    }

    pub(crate) fn introduced(&self) -> Option<&Ident> {
        match self {
            Self::Introduced(ident) => Some(ident),
            _ => None,
        }
    }

    pub(crate) fn turbofish(&self) -> TokenStream {
        match self.introduced() {
            Some(ident) => quote!(::<#ident>),
            None => quote!(),
        }
    }

    pub(crate) fn method_generics(&self) -> TokenStream {
        match self {
            Self::Synthesized(ident) | Self::Introduced(ident) => quote!(<#ident>),
            _ => quote!(),
        }
    }

    pub(crate) fn capability_predicate(
        &self,
        capabilities: &[TokenStream],
    ) -> Option<TokenStream> {
        match self {
            Self::Synthesized(ident) | Self::Declared(ident) | Self::Introduced(ident) => {
                Some(quote!(#ident: #(#capabilities)+*))
            }
            Self::Concrete(_) => None,
        }
    }

    pub(crate) fn definition_generics(
        &self,
        generics: &Generics,
        capabilities: &[TokenStream],
    ) -> Generics {
        let mut definition = generics.clone();

        if let Self::Synthesized(ident) | Self::Introduced(ident) = self {
            definition.params.push(parse_quote!(#ident));
        }

        if let Some(predicate) = self.capability_predicate(capabilities) {
            definition
                .make_where_clause()
                .predicates
                .push(parse_quote!(#predicate));
        }

        definition
    }
}
