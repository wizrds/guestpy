use syn::{Ident, ItemImpl, Type, spanned::Spanned};

use crate::host::HostMacroError;

#[derive(Debug)]
pub(crate) struct HostTarget {
    ident: Ident,
}

impl HostTarget {
    pub(crate) fn from_impl(item: &ItemImpl, attribute: &str) -> Result<Self, HostMacroError> {
        if item.trait_.is_some() {
            return Err(syn::Error::new(
                item.impl_token.span(),
                format!("#[{attribute}] applies only to inherent impl blocks"),
            )
            .into());
        }

        let Type::Path(target) = item.self_ty.as_ref() else {
            return Err(syn::Error::new_spanned(
                item.self_ty.as_ref(),
                format!("#[{attribute}] requires a named type target"),
            )
            .into());
        };

        target
            .path
            .segments
            .last()
            .map(|segment| Self { ident: segment.ident.clone() })
            .ok_or_else(|| {
                syn::Error::new_spanned(
                    target,
                    format!("#[{attribute}] requires a named type target"),
                )
                .into()
            })
    }

    pub(crate) fn name(&self) -> String {
        self.ident.to_string()
    }
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use crate::host::HostMacroError;

    use super::HostTarget;

    #[test]
    fn accepts_a_named_inherent_impl() {
        let item = parse_quote! {
            impl Service {}
        };

        assert_eq!(
            HostTarget::from_impl(&item, "host_class")
                .unwrap()
                .name(),
            "Service",
        );
    }

    #[test]
    fn rejects_a_trait_impl() {
        let item = parse_quote! {
            impl Trait for Service {}
        };
        let HostMacroError::Syntax(error) = HostTarget::from_impl(&item, "host_class").unwrap_err()
        else {
            panic!("trait target returns a syntax error");
        };

        assert!(
            error
                .to_string()
                .contains("#[host_class]")
        );
    }

    #[test]
    fn rejects_an_unnamed_target() {
        let item = parse_quote! {
            impl (Service,) {}
        };
        let HostMacroError::Syntax(error) =
            HostTarget::from_impl(&item, "host_module").unwrap_err()
        else {
            panic!("unnamed target returns a syntax error");
        };

        assert!(
            error
                .to_string()
                .contains("#[host_module]")
        );
    }
}
