use core::ops::Deref;

use darling::FromMeta;
use syn::{Meta, Token, Type, parse::Parser, punctuated::Punctuated};

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

#[cfg(test)]
mod tests {
    use darling::FromMeta;
    use quote::ToTokens;
    use syn::parse_quote;

    use super::TypeList;

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
}
