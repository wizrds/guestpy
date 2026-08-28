use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::Span;
use syn::{Ident, Path, parse_quote};

pub(crate) struct CratePath {
    explicit: Option<Path>,
}

impl CratePath {
    pub(crate) fn new(explicit: Option<Path>) -> Self {
        Self { explicit }
    }

    fn resolved() -> Path {
        match crate_name("guestpy") {
            Ok(FoundCrate::Itself) => parse_quote!(::guestpy),
            Ok(FoundCrate::Name(name)) => {
                let ident = Ident::new(&name.replace('-', "_"), Span::call_site());

                parse_quote!(::#ident)
            }
            Err(_) => parse_quote!(::guestpy),
        }
    }

    pub(crate) fn resolve(self) -> Path {
        self.explicit
            .unwrap_or_else(Self::resolved)
    }
}

#[cfg(test)]
mod tests {
    use quote::ToTokens;
    use syn::parse_quote;

    use super::CratePath;

    #[test]
    fn resolves_to_guestpy_by_default() {
        assert_eq!(
            CratePath::new(None)
                .resolve()
                .into_token_stream()
                .to_string(),
            ":: guestpy",
        );
    }

    #[test]
    fn preserves_an_explicit_path() {
        assert_eq!(
            CratePath::new(Some(parse_quote!(custom::guestpy)))
                .resolve()
                .into_token_stream()
                .to_string(),
            "custom :: guestpy",
        );
    }
}
