use darling::FromMeta;
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Ident, Path, Visibility};

use crate::{
    attributes::HelperAttributes,
    guest::{
        GuestMacroError,
        facade::{GuestFacadeDsl, GuestFacadeKind},
        member::GuestMember,
    },
    path::CratePath,
};

#[derive(Default, FromMeta)]
#[darling(default)]
struct GuestClassOptions {
    crate_path: Option<Path>,
    payload: Option<Path>,
}

pub(crate) struct GuestClassMacro {
    definition: GuestClassDefinition,
}

struct GuestClassDefinition {
    visibility: Visibility,
    name: Ident,
    crate_path: Path,
    payload: Option<Path>,
    members: Vec<GuestMember>,
}

impl GuestClassMacro {
    pub(crate) fn new(tokens: TokenStream) -> Result<Self, GuestMacroError> {
        let GuestFacadeDsl {
            mut attributes,
            visibility,
            name,
            members,
        } = GuestFacadeDsl::parse(tokens, GuestFacadeKind::Class)?;
        let options = GuestClassOptions::from_list(&HelperAttributes::take(&mut attributes)?)?;

        Ok(Self {
            definition: GuestClassDefinition {
                visibility,
                name,
                crate_path: CratePath::new(options.crate_path).resolve(),
                payload: options.payload,
                members: members
                    .into_iter()
                    .map(GuestMember::resolve)
                    .collect::<Result<Vec<_>, _>>()?,
            },
        })
    }

    pub(crate) fn expand(self) -> TokenStream {
        self.definition.render()
    }
}

impl GuestClassDefinition {
    fn render(self) -> TokenStream {
        let Self {
            visibility,
            name,
            crate_path,
            payload,
            members,
        } = self;
        let receiver = Ident::new("instance", Span::call_site());
        let methods = members
            .iter()
            .map(|member| member.render(&crate_path, &receiver));
        let instance_type = payload
            .as_ref()
            .map(|payload| quote!(#crate_path::handle::Instance<B, #payload>))
            .unwrap_or_else(|| quote!(#crate_path::handle::Instance<B>));
        let from_guest_bounds = payload
            .as_ref()
            .map(|payload| {
                quote! {
                    B: #crate_path::backend::Backend
                        + #crate_path::backend::BackendValues
                        + #crate_path::backend::BackendClasses,
                    #payload: #crate_path::host::class::HostClass,
                }
            })
            .unwrap_or_else(|| {
                quote! {
                    B: #crate_path::backend::Backend,
                }
            });

        quote! {
            #visibility struct #name<B: #crate_path::backend::Backend> {
                instance: #instance_type,
            }

            impl<B: #crate_path::backend::Backend> ::core::clone::Clone for #name<B> {
                fn clone(&self) -> Self {
                    Self {
                        instance: self.instance.clone(),
                    }
                }
            }

            impl<B: #crate_path::backend::Backend> #name<B> {
                fn new(instance: #instance_type) -> Self {
                    Self { instance }
                }

                pub fn instance(&self) -> &#instance_type {
                    &self.instance
                }

                pub fn into_instance(self) -> #instance_type {
                    self.instance
                }
            }

            impl<B> #name<B>
            where
                B: #crate_path::backend::Backend + #crate_path::backend::BackendValues,
            {
                #(#methods)*
            }

            impl<B: #crate_path::backend::Backend>
                ::core::convert::AsRef<#instance_type> for #name<B>
            {
                fn as_ref(&self) -> &#instance_type {
                    self.instance()
                }
            }

            impl<B: #crate_path::backend::Backend>
                ::core::convert::From<#instance_type> for #name<B>
            {
                fn from(instance: #instance_type) -> Self {
                    Self::new(instance)
                }
            }

            impl<B: #crate_path::backend::Backend>
                ::core::convert::Into<#instance_type> for #name<B>
            {
                fn into(self) -> #instance_type {
                    self.into_instance()
                }
            }

            impl<B> #crate_path::marshal::FromGuest<B> for #name<B>
            where
                #from_guest_bounds
            {
                type Owned = Self;

                fn from_guest<'py>(
                    enter: &#crate_path::scope::Enter<'py, B>,
                    value: <B as #crate_path::backend::Backend>::Value<'py>,
                ) -> ::core::result::Result<
                    Self::Owned,
                    #crate_path::errors::Error,
                > {
                    <#instance_type as #crate_path::marshal::FromGuest<B>>::from_guest(
                        enter,
                        value,
                    )
                    .map(Self::new)
                }
            }

            impl<B> #crate_path::marshal::ToGuest<B> for #name<B>
            where
                B: #crate_path::backend::Backend,
            {
                fn to_guest<'py>(
                    self,
                    enter: &#crate_path::scope::Enter<'py, B>,
                ) -> ::core::result::Result<
                    <B as #crate_path::backend::Backend>::Value<'py>,
                    #crate_path::errors::Error,
                > {
                    #crate_path::marshal::ToGuest::to_guest(
                        self.into_instance(),
                        enter,
                    )
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::GuestClassMacro;

    fn expand(tokens: proc_macro2::TokenStream) -> String {
        GuestClassMacro::new(tokens)
            .unwrap()
            .expand()
            .to_string()
    }

    #[test]
    fn generates_dynamic_facade_methods_values_and_conversions() {
        let output = expand(quote! {
            #[guestpy(crate_path = crate)]
            pub class Client {
                fn get(path: String) -> Coroutine<B, Response<B>>;

                #[guestpy(name = "baseUrl")]
                value base_url: String;
            }
        });

        assert!(output.contains("pub struct Client < B"));
        assert!(output.contains("crate :: handle :: Instance < B >"));
        assert!(output.contains("instance : self . instance . clone"));
        assert!(output.contains("crate :: handle :: ObjectProtocol :: call_method",));
        assert!(output.contains("\"get\""));
        assert!(output.contains("(path ,)"));
        assert!(output.contains("Coroutine < B , Response < B > >"));
        assert!(output.contains("crate :: handle :: ObjectProtocol :: get",));
        assert!(output.contains("\"baseUrl\""));
        assert!(output.contains("fn instance"));
        assert!(output.contains("fn into_instance"));
        assert!(output.contains(":: core :: convert :: AsRef"));
        assert!(output.contains(":: core :: convert :: From"));
        assert!(output.contains(":: core :: convert :: Into"));
        assert!(output.contains("marshal :: FromGuest"));
        assert!(output.contains("marshal :: ToGuest"));
    }

    #[test]
    fn generates_payload_typed_facade() {
        let output = expand(quote! {
            #[guestpy(crate_path = crate, payload = HostResponse)]
            pub class Response {
                fn status() -> u16;
            }
        });

        assert!(output.contains("crate :: handle :: Instance < B , HostResponse >",));
        assert!(output.contains("HostResponse : crate :: host :: class :: HostClass"));
        assert!(output.contains("marshal :: FromGuest"));
        assert!(output.contains("marshal :: ToGuest"));
    }

    #[test]
    fn honours_inherited_visibility_and_default_crate() {
        let output = expand(quote! {
            class Widget {
                value count: i64;
            }
        });

        assert!(output.contains("struct Widget < B"));
        assert!(output.contains("guestpy :: handle :: Instance"));
    }

    #[test]
    fn rejects_an_unexpected_facade_keyword() {
        assert!(
            GuestClassMacro::new(quote! {
                pub Client {
                    value count: i64;
                }
            })
            .is_err(),
        );
    }

    #[test]
    fn rejects_unknown_class_option() {
        assert!(
            GuestClassMacro::new(quote! {
                #[guestpy(unknown)]
                pub class Client {}
            })
            .is_err(),
        );
    }
}
