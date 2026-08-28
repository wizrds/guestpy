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
struct GuestModuleOptions {
    crate_path: Option<Path>,
}

pub(crate) struct GuestModuleMacro {
    definition: GuestModuleDefinition,
}

struct GuestModuleDefinition {
    visibility: Visibility,
    name: Ident,
    crate_path: Path,
    members: Vec<GuestMember>,
}

impl GuestModuleMacro {
    pub(crate) fn new(
        tokens: TokenStream,
    ) -> Result<Self, GuestMacroError> {
        let GuestFacadeDsl {
            mut attributes,
            visibility,
            name,
            members,
        } = GuestFacadeDsl::parse(
            tokens,
            GuestFacadeKind::Module,
        )?;
        let options = GuestModuleOptions::from_list(
            &HelperAttributes::take(&mut attributes)?,
        )?;

        Ok(Self {
            definition: GuestModuleDefinition {
                visibility,
                name,
                crate_path: CratePath::new(options.crate_path).resolve(),
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

impl GuestModuleDefinition {
    fn render(self) -> TokenStream {
        let Self {
            visibility,
            name,
            crate_path,
            members,
        } = self;
        let receiver = Ident::new("module", Span::call_site());
        let methods = members
            .iter()
            .map(|member| member.render(&crate_path, &receiver));

        quote! {
            #visibility struct #name<B: #crate_path::backend::Backend> {
                module: #crate_path::handle::Module<B>,
            }

            impl<B> #name<B>
            where
                B: #crate_path::backend::Backend + #crate_path::backend::BackendValues,
            {
                #(#methods)*
            }

            impl<B: #crate_path::backend::Backend>
                ::core::convert::From<#crate_path::handle::Module<B>> for #name<B>
            {
                fn from(module: #crate_path::handle::Module<B>) -> Self {
                    Self { module }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::GuestModuleMacro;

    fn expand(tokens: proc_macro2::TokenStream) -> String {
        GuestModuleMacro::new(tokens)
            .unwrap()
            .expand()
            .to_string()
    }

    #[test]
    fn generates_single_facade_over_call_and_get() {
        let output = expand(quote! {
            #[guestpy(crate_path = crate)]
            pub module Math {
                fn add(left: i64, right: i64) -> i64;

                #[guestpy(name = "makeHandler")]
                fn make_handler() -> Function<B>;

                fn spawn(value: i64) -> Coroutine<B, ()>;

                value answer: i64;

                value client: Class<B>;
            }
        });

        assert!(output.contains("pub struct Math < B"));
        assert!(output.contains("module : crate :: handle :: Module < B >"));
        assert!(output.contains(":: core :: convert :: From"));
        assert!(output.contains("for Math < B >"));
        assert!(output.contains("self . module . call"));
        assert!(output.contains("\"add\""));
        assert!(output.contains("(left , right ,)"));
        assert!(output.contains("\"makeHandler\""));
        assert!(output.contains("Function < B >"));
        assert!(output.contains("\"spawn\""));
        assert!(output.contains("Coroutine < B , () >"));
        assert!(output.contains("(value ,)"));
        assert!(output.contains("self . module . get"));
        assert!(output.contains("\"answer\""));
        assert!(output.contains("\"client\""));
        assert!(output.contains("Class < B >"));
    }

    #[test]
    fn honours_inherited_visibility_and_default_crate() {
        let output = expand(quote! {
            module Widgets {
                value count: i64;
            }
        });

        assert!(output.contains("struct Widgets < B"));
        assert!(output.contains("guestpy :: handle :: Module"));
    }

    #[test]
    fn rejects_an_unexpected_facade_keyword() {
        assert!(
            GuestModuleMacro::new(quote! {
                pub class Client {}
            })
            .is_err(),
        );
    }
}
