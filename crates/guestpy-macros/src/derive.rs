use darling::FromDeriveInput;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, Generics, Ident, Path, parse_quote};

use crate::path::CratePath;

#[derive(FromDeriveInput)]
#[darling(attributes(guestpy), supports(struct_any, enum_any))]
struct GuestDeriveInput {
    ident: Ident,
    generics: Generics,
    crate_path: Option<Path>,
}

impl GuestDeriveInput {
    fn new(input: &DeriveInput) -> Result<Self, darling::Error> {
        Self::from_derive_input(input)
    }

    fn ident(&self) -> Ident {
        self.ident.clone()
    }

    fn generics(&self) -> Generics {
        self.generics.clone()
    }

    fn crate_path(&self) -> Path {
        CratePath::new(self.crate_path.clone()).resolve()
    }
}

pub(crate) struct GuestDerive {
    input: GuestDeriveInput,
}

impl GuestDerive {
    pub(crate) fn new(input: &DeriveInput) -> Result<Self, darling::Error> {
        Ok(Self { input: GuestDeriveInput::new(input)? })
    }

    pub(crate) fn to_guest(&self) -> TokenStream {
        let crate_path = &self.input.crate_path();
        let ident = &self.input.ident();
        let generics = self.input.generics();
        let mut implementation = generics.clone();

        implementation
            .params
            .push(parse_quote!(B));

        let predicates = &mut implementation
            .make_where_clause()
            .predicates;
        predicates.push(parse_quote!(
            B: #crate_path::backend::Backend + #crate_path::backend::BackendValues
        ));
        predicates.push(parse_quote!(Self: ::serde::Serialize));

        let (impl_generics, _, where_clause) = implementation.split_for_impl();
        let (_, ty_generics, _) = generics.split_for_impl();

        quote! {
            impl #impl_generics #crate_path::marshal::ToGuest<B>
                for #ident #ty_generics #where_clause
            {
                fn to_guest<'py>(
                    self,
                    enter: &#crate_path::scope::Enter<'py, B>,
                ) -> ::core::result::Result<
                    <B as #crate_path::backend::Backend>::Value<'py>,
                    #crate_path::errors::Error,
                > {
                    enter.to_value(&self)
                }
            }
        }
    }

    pub(crate) fn from_guest(&self) -> TokenStream {
        let crate_path = &self.input.crate_path();
        let ident = &self.input.ident();
        let generics = self.input.generics();
        let mut implementation = generics.clone();

        implementation
            .params
            .push(parse_quote!(B));

        let predicates = &mut implementation
            .make_where_clause()
            .predicates;
        predicates.push(parse_quote!(
            B: #crate_path::backend::Backend + #crate_path::backend::BackendValues
        ));
        predicates.push(parse_quote!(Self: ::serde::de::DeserializeOwned + 'static));

        let (impl_generics, _, where_clause) = implementation.split_for_impl();
        let (_, ty_generics, _) = generics.split_for_impl();

        quote! {
            impl #impl_generics #crate_path::marshal::FromGuest<B>
                for #ident #ty_generics #where_clause
            {
                type Owned = Self;

                fn from_guest<'py>(
                    enter: &#crate_path::scope::Enter<'py, B>,
                    value: <B as #crate_path::backend::Backend>::Value<'py>,
                ) -> ::core::result::Result<Self::Owned, #crate_path::errors::Error> {
                    enter.from_value(value)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use syn::{ItemImpl, parse_quote};

    use super::GuestDerive;

    fn expand_to_guest(input: syn::DeriveInput) -> ItemImpl {
        syn::parse2(
            GuestDerive::new(&input)
                .expect("failed to create GuestDerive")
                .to_guest(),
        )
        .expect("generated ToGuest code parses as a single impl")
    }

    fn expand_from_guest(input: syn::DeriveInput) -> ItemImpl {
        syn::parse2(
            GuestDerive::new(&input)
                .expect("failed to create GuestDerive")
                .from_guest(),
        )
        .expect("generated FromGuest code parses as a single impl")
    }

    fn trait_name(expanded: &ItemImpl) -> String {
        expanded
            .trait_
            .as_ref()
            .expect("impl has a trait")
            .0
            .segments
            .last()
            .expect("trait path has a segment")
            .ident
            .to_string()
    }

    #[test]
    fn to_guest_emits_a_single_method_delegating_impl() {
        let expanded = expand_to_guest(parse_quote! {
            struct Request {
                user_id: u64,
            }
        });

        assert_eq!(trait_name(&expanded), "ToGuest");
        assert_eq!(expanded.generics.params.len(), 1);
        assert_eq!(expanded.items.len(), 1);

        let rendered = quote::quote!(#expanded).to_string();

        assert!(rendered.contains("to_value"));
    }

    #[test]
    fn from_guest_emits_owned_self_and_delegates() {
        let expanded = expand_from_guest(parse_quote! {
            struct Request {
                user_id: u64,
            }
        });

        assert_eq!(trait_name(&expanded), "FromGuest");

        let rendered = quote::quote!(#expanded).to_string();

        assert!(rendered.contains("type Owned = Self"));
        assert!(rendered.contains("from_value"));
    }

    #[test]
    fn preserves_the_type_own_generics() {
        let expanded = expand_to_guest(parse_quote! {
            struct Wrapper<T> {
                value: T,
            }
        });

        assert_eq!(expanded.generics.params.len(), 2);
    }

    #[test]
    fn resolves_to_guestpy_by_default() {
        let rendered = GuestDerive::new(&parse_quote! {
            struct Request;
        })
        .expect("failed to create GuestDerive")
        .to_guest()
        .to_string();

        assert!(rendered.contains("guestpy"));
    }
}
