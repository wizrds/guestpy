use darling::{util::Flag, FromDeriveInput};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_quote, Data, DeriveInput, Fields, Generics, Ident, Path, Type};

use crate::path::CratePath;

#[derive(FromDeriveInput)]
#[darling(attributes(guestpy), supports(struct_any, enum_any))]
struct GuestDeriveInput {
    ident: Ident,
    generics: Generics,
    crate_path: Option<Path>,
    union: Flag,
    backend: Option<Ident>,
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

struct UnionVariant {
    ident: Ident,
    ty: Type,
}

impl UnionVariant {
    fn parse_all(data: &Data) -> Result<Vec<Self>, darling::Error> {
        let Data::Enum(data) = data else {
            return Err(darling::Error::custom("#[guestpy(union)] is only valid on an enum"));
        };

        if data.variants.is_empty() {
            return Err(darling::Error::custom(
                "a #[guestpy(union)] enum needs at least one variant",
            ));
        }

        data.variants
            .iter()
            .map(|variant| match &variant.fields {
                Fields::Unnamed(fields) if fields.unnamed.len() == 1 => Ok(Self {
                    ident: variant.ident.clone(),
                    ty: fields.unnamed[0].ty.clone(),
                }),
                _ => Err(darling::Error::custom(
                    "a #[guestpy(union)] variant must hold exactly one unnamed field",
                )
                .with_span(variant)),
            })
            .collect()
    }
}

enum DeriveShape {
    Serde,
    Union(Vec<UnionVariant>),
}

pub(crate) struct GuestDerive {
    input: GuestDeriveInput,
    shape: DeriveShape,
}

impl GuestDerive {
    pub(crate) fn new(input: &DeriveInput) -> Result<Self, darling::Error> {
        let derive_input = GuestDeriveInput::new(input)?;

        if derive_input.union.is_present() {
            Ok(Self {
                input: derive_input,
                shape: DeriveShape::Union(UnionVariant::parse_all(&input.data)?),
            })
        } else {
            Ok(Self {
                input: derive_input,
                shape: DeriveShape::Serde,
            })
        }
    }

    fn backend_generics(&self) -> (Ident, Generics) {
        let crate_path = self.input.crate_path();
        let mut implementation = self.input.generics();
        let backend = match &self.input.backend {
            Some(backend) => backend.clone(),
            None => {
                implementation
                    .params
                    .push(parse_quote!(B));

                format_ident!("B")
            }
        };

        implementation
            .make_where_clause()
            .predicates
            .push(parse_quote!(
                #backend: #crate_path::backend::Backend
                    + #crate_path::backend::BackendValues
            ));

        (backend, implementation)
    }

    fn serde_to_guest(&self) -> TokenStream {
        let crate_path = &self.input.crate_path();
        let ident = &self.input.ident();
        let generics = self.input.generics();
        let (backend, mut implementation) = self.backend_generics();

        implementation
            .make_where_clause()
            .predicates
            .push(parse_quote!(Self: ::serde::Serialize));

        let (impl_generics, _, where_clause) = implementation.split_for_impl();
        let (_, ty_generics, _) = generics.split_for_impl();

        quote! {
            impl #impl_generics #crate_path::marshal::ToGuest<#backend>
                for #ident #ty_generics #where_clause
            {
                fn to_guest<'py>(
                    self,
                    enter: &#crate_path::scope::Enter<'py, #backend>,
                ) -> ::core::result::Result<
                    <#backend as #crate_path::backend::Backend>::Value<'py>,
                    #crate_path::errors::Error,
                > {
                    enter.to_value(&self)
                }
            }
        }
    }

    fn serde_from_guest(&self) -> TokenStream {
        let crate_path = &self.input.crate_path();
        let ident = &self.input.ident();
        let generics = self.input.generics();
        let (backend, mut implementation) = self.backend_generics();

        implementation
            .make_where_clause()
            .predicates
            .push(parse_quote!(Self: ::serde::de::DeserializeOwned + 'static));

        let (impl_generics, _, where_clause) = implementation.split_for_impl();
        let (_, ty_generics, _) = generics.split_for_impl();

        quote! {
            impl #impl_generics #crate_path::marshal::FromGuest<#backend>
                for #ident #ty_generics #where_clause
            {
                type Owned = Self;

                fn from_guest<'py>(
                    enter: &#crate_path::scope::Enter<'py, #backend>,
                    value: <#backend as #crate_path::backend::Backend>::Value<'py>,
                ) -> ::core::result::Result<Self::Owned, #crate_path::errors::Error> {
                    enter.from_value(value)
                }
            }
        }
    }

    fn union_from_guest(&self, variants: &[UnionVariant]) -> TokenStream {
        let crate_path = &self.input.crate_path();
        let ident = &self.input.ident();
        let mut description = self.input.generics();
        let (backend, mut implementation) = self.backend_generics();
        let predicates = &mut implementation
            .make_where_clause()
            .predicates;

        predicates.push(parse_quote!(Self: 'static));

        for UnionVariant { ty, .. } in variants {
            predicates.push(parse_quote!(
                #ty: #crate_path::marshal::FromGuest<#backend, Owned = #ty>
                    + #crate_path::marshal::describe::Describe
            ));
            description
                .make_where_clause()
                .predicates
                .push(parse_quote!(
                    #ty: #crate_path::marshal::describe::Describe
                ));
        }

        let (impl_generics, _, where_clause) = implementation.split_for_impl();
        let (describe_generics, ty_generics, describe_where) = description.split_for_impl();
        let attempts = variants
            .iter()
            .map(|UnionVariant { ident: variant, ty }| {
                quote! {
                    match <#ty as #crate_path::marshal::FromGuest<#backend>>::from_guest(
                        enter,
                        ::core::clone::Clone::clone(&value),
                    ) {
                        ::core::result::Result::Ok(value) => {
                            return ::core::result::Result::Ok(Self::#variant(value));
                        }
                        ::core::result::Result::Err(error)
                            if !error.is_conversion() =>
                        {
                            return ::core::result::Result::Err(error);
                        }
                        ::core::result::Result::Err(_) => {}
                    }
                }
            });
        let names = variants
            .iter()
            .map(|UnionVariant { ty, .. }| {
                quote! {
                    <#ty as #crate_path::marshal::describe::Describe>::describe(expected);
                }
            });

        quote! {
            impl #impl_generics #crate_path::marshal::FromGuest<#backend>
                for #ident #ty_generics #where_clause
            {
                type Owned = Self;

                fn from_guest<'py>(
                    enter: &#crate_path::scope::Enter<'py, #backend>,
                    value: <#backend as #crate_path::backend::Backend>::Value<'py>,
                ) -> ::core::result::Result<Self::Owned, #crate_path::errors::Error> {
                    #(#attempts)*

                    ::core::result::Result::Err(
                        #crate_path::errors::Error::mismatch::<Self>(
                            &<#backend as #crate_path::backend::BackendValues>::type_name(
                                enter.token(),
                                &value,
                            ),
                        ),
                    )
                }
            }

            impl #describe_generics #crate_path::marshal::describe::Describe
                for #ident #ty_generics #describe_where
            {
                fn describe(
                    expected: &mut #crate_path::marshal::describe::Expected,
                ) {
                    #(#names)*
                }
            }
        }
    }

    fn union_to_guest(&self, variants: &[UnionVariant]) -> TokenStream {
        let crate_path = &self.input.crate_path();
        let ident = &self.input.ident();
        let generics = self.input.generics();
        let (backend, mut implementation) = self.backend_generics();
        let predicates = &mut implementation
            .make_where_clause()
            .predicates;

        for UnionVariant { ty, .. } in variants {
            predicates.push(parse_quote!(
                #ty: #crate_path::marshal::ToGuest<#backend>
            ));
        }

        let (impl_generics, _, where_clause) = implementation.split_for_impl();
        let (_, ty_generics, _) = generics.split_for_impl();
        let arms = variants
            .iter()
            .map(|UnionVariant { ident: variant, ty }| {
                quote! {
                    Self::#variant(value) => {
                        <#ty as #crate_path::marshal::ToGuest<#backend>>::to_guest(
                            value,
                            enter,
                        )
                    }
                }
            });

        quote! {
            impl #impl_generics #crate_path::marshal::ToGuest<#backend>
                for #ident #ty_generics #where_clause
            {
                fn to_guest<'py>(
                    self,
                    enter: &#crate_path::scope::Enter<'py, #backend>,
                ) -> ::core::result::Result<
                    <#backend as #crate_path::backend::Backend>::Value<'py>,
                    #crate_path::errors::Error,
                > {
                    match self {
                        #(#arms)*
                    }
                }
            }
        }
    }

    pub(crate) fn to_guest(&self) -> TokenStream {
        match &self.shape {
            DeriveShape::Serde => self.serde_to_guest(),
            DeriveShape::Union(variants) => self.union_to_guest(variants),
        }
    }

    pub(crate) fn from_guest(&self) -> TokenStream {
        match &self.shape {
            DeriveShape::Serde => self.serde_from_guest(),
            DeriveShape::Union(variants) => self.union_from_guest(variants),
        }
    }
}

#[cfg(test)]
mod tests {
    use syn::{parse_quote, ItemImpl};

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

    fn rejection(input: syn::DeriveInput) -> String {
        GuestDerive::new(&input)
            .err()
            .expect("union shape is rejected")
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

    #[test]
    fn union_from_guest_preserves_variant_order_and_checks_error_kind() {
        let rendered = GuestDerive::new(&parse_quote! {
            #[guestpy(union)]
            enum Value {
                Integer(i64),
                Text(String),
            }
        })
        .expect("failed to create union GuestDerive")
        .from_guest()
        .to_string();

        assert!(
            rendered
                .find("Self :: Integer")
                .expect("integer attempt is generated")
                < rendered
                    .find("Self :: Text")
                    .expect("text attempt is generated"),
        );
        assert!(rendered.contains("is_conversion"));
        assert!(rendered.contains("Describe"));
        assert!(rendered.contains("mismatch :: < Self >"));
    }

    #[test]
    fn union_to_guest_matches_every_variant() {
        let rendered = GuestDerive::new(&parse_quote! {
            #[guestpy(union)]
            enum Value {
                Integer(i64),
                Text(String),
            }
        })
        .expect("failed to create union GuestDerive")
        .to_guest()
        .to_string();

        assert!(rendered.contains("Self :: Integer"));
        assert!(rendered.contains("Self :: Text"));
        assert!(rendered.contains("ToGuest"));
    }

    #[test]
    fn union_can_reuse_a_declared_backend_parameter() {
        let rendered = GuestDerive::new(&parse_quote! {
            #[guestpy(union, backend = B)]
            enum Value<B> {
                Object(Object<B>),
            }
        })
        .expect("failed to create union GuestDerive")
        .from_guest()
        .to_string();

        assert!(rendered.contains("FromGuest < B >"));
        assert!(!rendered.contains("impl < B , B >"));
    }

    #[test]
    fn union_rejects_non_enum_empty_and_non_newtype_shapes() {
        assert!(rejection(parse_quote! {
            #[guestpy(union)]
            struct Value(i64);
        })
        .contains("#[guestpy(union)] is only valid on an enum"),);
        assert!(rejection(parse_quote! {
            #[guestpy(union)]
            enum Value {}
        })
        .contains("a #[guestpy(union)] enum needs at least one variant"),);

        for input in [
            parse_quote! {
                #[guestpy(union)]
                enum Value { Unit }
            },
            parse_quote! {
                #[guestpy(union)]
                enum Value { Named { value: i64 } }
            },
            parse_quote! {
                #[guestpy(union)]
                enum Value { Pair(i64, String) }
            },
        ] {
            assert!(rejection(input)
                .contains("a #[guestpy(union)] variant must hold exactly one unnamed field",));
        }
    }
}
