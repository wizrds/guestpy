use proc_macro2::TokenStream;
use syn::{
    Attribute, Ident, Visibility, braced,
    parse::{ParseStream, Parser},
};

use crate::guest::member::RawGuestMember;

#[derive(Clone, Copy)]
pub(crate) enum GuestFacadeKind {
    Class,
    Module,
}

impl GuestFacadeKind {
    fn keyword(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Module => "module",
        }
    }
}

#[derive(Debug)]
pub(crate) struct GuestFacadeDsl {
    pub(crate) attributes: Vec<Attribute>,
    pub(crate) visibility: Visibility,
    pub(crate) name: Ident,
    pub(crate) members: Vec<RawGuestMember>,
}

impl GuestFacadeDsl {
    pub(crate) fn parse(tokens: TokenStream, expected: GuestFacadeKind) -> syn::Result<Self> {
        (|input: ParseStream| Self::parse_expected(input, expected)).parse2(tokens)
    }

    fn parse_expected(input: ParseStream, expected: GuestFacadeKind) -> syn::Result<Self> {
        let attributes = input.call(Attribute::parse_outer)?;
        let visibility = input.parse::<Visibility>()?;
        let keyword = input.parse::<Ident>()?;

        if keyword != expected.keyword() {
            return Err(syn::Error::new(
                keyword.span(),
                format!("expected `{}`", expected.keyword()),
            ));
        }

        let name = input.parse::<Ident>()?;
        let content;

        braced!(content in input);

        let mut members = Vec::new();

        while !content.is_empty() {
            members.push(content.parse::<RawGuestMember>()?);
        }

        Ok(Self { attributes, visibility, name, members })
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::{GuestFacadeDsl, GuestFacadeKind};

    #[test]
    fn parses_a_class_facade() {
        let facade = GuestFacadeDsl::parse(
            quote! {
                pub class Client {
                    value status: i64;
                }
            },
            GuestFacadeKind::Class,
        )
        .unwrap();

        assert_eq!(facade.name.to_string(), "Client");
        assert_eq!(facade.members.len(), 1);
    }

    #[test]
    fn parses_a_module_facade() {
        let facade = GuestFacadeDsl::parse(
            quote! {
                pub module Client {
                    value status: i64;
                }
            },
            GuestFacadeKind::Module,
        )
        .unwrap();

        assert_eq!(facade.name.to_string(), "Client");
        assert_eq!(facade.members.len(), 1);
    }

    #[test]
    fn rejects_module_when_class_is_expected() {
        let error = GuestFacadeDsl::parse(
            quote! {
                pub module Client {}
            },
            GuestFacadeKind::Class,
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("expected `class`")
        );
    }

    #[test]
    fn rejects_class_when_module_is_expected() {
        let error = GuestFacadeDsl::parse(
            quote! {
                pub class Client {}
            },
            GuestFacadeKind::Module,
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("expected `module`")
        );
    }
}
