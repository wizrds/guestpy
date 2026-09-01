use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    GenericParam, Generics, Ident, ItemImpl, Type, TypeParam, parse_quote, spanned::Spanned,
};

use crate::host::HostMacroError;

pub(crate) enum BackendParameter {
    Synthesized(Ident),
    Declared(Ident),
    Concrete(Type),
}

impl BackendParameter {
    pub(crate) fn resolve(
        declared: Option<Type>,
        item: &ItemImpl,
        attribute: &str,
    ) -> Result<Self, HostMacroError> {
        if let Some(ty) = declared {
            return Ok(match Self::names_parameter(&ty, &item.generics) {
                Some(ident) => Self::Declared(ident),
                None => Self::Concrete(ty),
            });
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

        Ok(Self::Synthesized(match Self::declares_parameter(&item.generics, "B") {
            true => parse_quote!(__GuestpyBackend),
            false => parse_quote!(B),
        }))
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

    fn names_parameter(ty: &Type, generics: &Generics) -> Option<Ident> {
        let Type::Path(path) = ty else {
            return None;
        };

        if path.qself.is_some() || path.path.segments.len() != 1 {
            return None;
        }

        let segment = path.path.segments.first()?;

        if !segment.arguments.is_none() {
            return None;
        }

        Self::parameters(generics)
            .find(|parameter| parameter.ident == segment.ident)
            .map(|parameter| parameter.ident.clone())
    }

    fn backend_bounded_parameter(generics: &Generics) -> Option<Ident> {
        Self::parameters(generics)
            .find(|parameter| {
                parameter
                    .bounds
                    .iter()
                    .any(|bound| match bound {
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
            Self::Synthesized(ident) | Self::Declared(ident) => parse_quote!(#ident),
            Self::Concrete(ty) => ty.clone(),
        }
    }

    pub(crate) fn method_generics(&self) -> TokenStream {
        match self {
            Self::Synthesized(ident) => quote!(<#ident>),
            _ => quote!(),
        }
    }

    pub(crate) fn capability_predicate(&self, capabilities: &[TokenStream]) -> Option<TokenStream> {
        match self {
            Self::Synthesized(ident) | Self::Declared(ident) => {
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

        if let Self::Synthesized(ident) = self {
            definition
                .params
                .push(parse_quote!(#ident));
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
