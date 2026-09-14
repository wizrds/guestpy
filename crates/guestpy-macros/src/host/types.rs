use core::ops::Deref;

use darling::FromMeta;
use syn::{
    Ident, LitStr, Meta, Token, Type,
    parse::{Parse, ParseStream, Parser},
    punctuated::Punctuated,
};

#[derive(Default)]
pub(crate) struct TypeList(Vec<Type>);

impl Deref for TypeList {
    type Target = Vec<Type>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromMeta for TypeList {
    fn from_meta(item: &Meta) -> darling::Result<Self> {
        let Meta::List(list) = item else {
            return Err(darling::Error::unsupported_format("word").with_span(item));
        };

        Punctuated::<Type, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .map(|types| Self(types.into_iter().collect()))
            .map_err(|error| darling::Error::from(error).with_span(item))
    }
}

pub(crate) enum BaseItem {
    Host(Type),
    Imported {
        module: LitStr,
        qualname: LitStr,
    },
}

impl Parse for BaseItem {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        if !input.peek(LitStr) {
            return input.parse().map(Self::Host);
        }

        let literal = input.parse::<LitStr>()?;
        let value = literal.value();
        let Some((module, qualname)) = value.split_once(':') else {
            return Err(Self::invalid(&literal));
        };

        if qualname.contains(':') || !Self::path(module) || !Self::path(qualname) {
            return Err(Self::invalid(&literal));
        }

        Ok(Self::Imported {
            module: LitStr::new(module, literal.span()),
            qualname: LitStr::new(qualname, literal.span()),
        })
    }
}

impl BaseItem {
    fn path(value: &str) -> bool {
        !value.is_empty()
            && value
                .split('.')
                .all(|part| !part.is_empty() && syn::parse_str::<Ident>(part).is_ok())
    }

    fn invalid(literal: &LitStr) -> syn::Error {
        syn::Error::new(
            literal.span(),
            "expected \"module:qualname\" such as \"collections.abc:Mapping\"",
        )
    }
}

#[derive(Default)]
pub(crate) struct BaseList(Vec<BaseItem>);

impl Deref for BaseList {
    type Target = Vec<BaseItem>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FromMeta for BaseList {
    fn from_meta(item: &Meta) -> darling::Result<Self> {
        let Meta::List(list) = item else {
            return Err(darling::Error::unsupported_format("word").with_span(item));
        };

        Punctuated::<BaseItem, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .map(|bases| Self(bases.into_iter().collect()))
            .map_err(|error| darling::Error::from(error).with_span(item))
    }
}

#[cfg(test)]
mod tests {
    use darling::FromMeta;
    use quote::ToTokens;
    use syn::{parse_quote, Meta};

    use super::{BaseItem, BaseList, TypeList};

    #[test]
    fn parses_plain_and_generic_entries() {
        assert_eq!(
            TypeList::from_meta(&parse_quote!(classes(Vector2, Envelope<B>)))
                .unwrap()
                .iter()
                .map(|entry| entry.into_token_stream().to_string())
                .collect::<Vec<_>>(),
            vec!["Vector2".to_string(), "Envelope < B >".to_string()],
        );
    }

    #[test]
    fn rejects_a_bare_word() {
        assert!(TypeList::from_meta(&parse_quote!(classes)).is_err());
    }

    #[test]
    fn parses_mixed_host_and_imported_bases() {
        let bases = BaseList::from_meta(
            &parse_quote!(extends(Headers, "collections.abc:Mapping")),
        )
        .unwrap();

        assert!(matches!(&bases[0], BaseItem::Host(_)));
        assert!(matches!(
            &bases[1],
            BaseItem::Imported { module, qualname }
                if module.value() == "collections.abc" && qualname.value() == "Mapping"
        ));
    }

    #[test]
    fn rejects_invalid_imported_bases() {
        for value in [
            "collections.abc.Mapping",
            ":Mapping",
            "collections.abc:",
            "a:b:c",
            "collections.1abc:Mapping",
        ] {
            let meta = syn::parse_str::<Meta>(&format!("extends(\"{value}\")")).unwrap();

            assert!(BaseList::from_meta(&meta).is_err());
        }
    }
}
