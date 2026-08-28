use darling::ast::NestedMeta;
use syn::{Attribute, Meta};

pub(crate) struct HelperAttributes;

impl HelperAttributes {
    pub(crate) fn take(attributes: &mut Vec<Attribute>) -> Result<Vec<NestedMeta>, darling::Error> {
        let mut helpers = Vec::new();
        let mut retained = Vec::with_capacity(attributes.len());

        for attribute in attributes.drain(..) {
            if !attribute.path().is_ident("guestpy") {
                retained.push(attribute);

                continue;
            }

            match attribute.meta {
                Meta::List(list) => helpers.extend(NestedMeta::parse_meta_list(list.tokens)?),
                meta => {
                    return Err(syn::Error::new_spanned(meta, "expected #[guestpy(...)]").into());
                }
            }
        }

        *attributes = retained;

        Ok(helpers)
    }
}

#[cfg(test)]
mod tests {
    use quote::ToTokens;
    use syn::{ImplItemFn, parse_quote};

    use super::HelperAttributes;

    #[test]
    fn consumes_guestpy_and_preserves_other_attributes() {
        let mut method: ImplItemFn = parse_quote! {
            #[allow(dead_code)]
            #[guestpy(method, name = "read")]
            fn read(&self) -> Result<i32, Error> {
                Ok(1)
            }
        };

        assert_eq!(
            HelperAttributes::take(&mut method.attrs)
                .unwrap()
                .len(),
            2,
        );
        assert_eq!(method.attrs.len(), 1);
        assert!(
            method.attrs[0]
                .to_token_stream()
                .to_string()
                .contains("allow"),
        );
    }
}
