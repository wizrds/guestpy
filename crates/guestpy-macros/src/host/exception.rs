use darling::{FromDeriveInput, FromField, ast::Data, util::Flag};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, Generics, Ident, Path, Type};

use crate::{host::HostMacroError, path::CratePath};

#[derive(FromField)]
#[darling(attributes(guestpy))]
struct ExceptionFieldInput {
    ident: Option<Ident>,
    ty: Type,
    arg: Flag,
    name: Option<String>,
}

#[derive(FromDeriveInput)]
#[darling(attributes(guestpy), supports(struct_named, struct_unit))]
struct ExceptionDeriveInput {
    ident: Ident,
    generics: Generics,
    data: Data<(), ExceptionFieldInput>,
    name: Option<String>,
    base: Option<Path>,
    builtin: Option<String>,
    crate_path: Option<Path>,
}

enum Base {
    Default,
    Typed(Path),
    Builtin(String),
}

impl Base {
    fn tokens(&self, crate_path: &Path) -> TokenStream {
        match self {
            Self::Default => quote!(
                #crate_path::host::exception::ExceptionClass::exception()
            ),
            Self::Typed(base) => quote!(
                <#base as #crate_path::host::exception::HostException>::class()
            ),
            Self::Builtin(base) => quote!(
                #crate_path::host::exception::ExceptionClass::builtin(#base)
            ),
        }
    }
}

enum ExceptionFieldKind {
    Argument(usize),
    Attribute(String),
}

struct ExceptionField {
    ident: Ident,
    ty: Type,
    kind: ExceptionFieldKind,
}

impl ExceptionField {
    fn new(input: ExceptionFieldInput, argument: &mut usize) -> Result<Self, HostMacroError> {
        let ident = input
            .ident
            .expect("named fields have identifiers");

        if input.arg.is_present() && input.name.is_some() {
            return Err(syn::Error::new(
                ident.span(),
                "an exception field cannot combine arg and name",
            )
            .into());
        }

        let kind = if input.arg.is_present() {
            let index = *argument;

            *argument += 1;

            ExceptionFieldKind::Argument(index)
        } else {
            ExceptionFieldKind::Attribute(
                input
                    .name
                    .unwrap_or_else(|| ident.to_string()),
            )
        };

        Ok(Self { ident, ty: input.ty, kind })
    }

    fn raise(&self) -> TokenStream {
        let ident = &self.ident;

        match &self.kind {
            ExceptionFieldKind::Argument(_) => quote!(.arg(self.#ident)),
            ExceptionFieldKind::Attribute(name) => quote!(.attr(#name, self.#ident)),
        }
    }

    fn read(&self) -> TokenStream {
        let ident = &self.ident;
        let ty = &self.ty;

        match &self.kind {
            ExceptionFieldKind::Argument(index) => {
                quote!(#ident: raised.arg::<#ty>(#index)?)
            }
            ExceptionFieldKind::Attribute(name) => {
                quote!(#ident: raised.attr::<#ty>(#name)?)
            }
        }
    }
}

pub(crate) struct HostExceptionDerive {
    ident: Ident,
    name: String,
    base: Base,
    crate_path: Path,
    fields: Vec<ExceptionField>,
}

impl HostExceptionDerive {
    pub(crate) fn new(input: &DeriveInput) -> Result<Self, HostMacroError> {
        let input = ExceptionDeriveInput::from_derive_input(input)?;

        if let Some(parameter) = input.generics.params.first() {
            return Err(syn::Error::new_spanned(
                parameter,
                "HostException does not support generic types",
            )
            .into());
        }

        let base = match (input.base, input.builtin) {
            (Some(_), Some(_)) => {
                return Err(syn::Error::new(
                    input.ident.span(),
                    "HostException cannot combine base and builtin",
                )
                .into());
            }
            (Some(base), None) => Base::Typed(base),
            (None, Some(base)) => Base::Builtin(base),
            (None, None) => Base::Default,
        };
        let Data::Struct(fields) = input.data else {
            unreachable!("darling rejects non-struct HostException inputs")
        };
        let mut argument = 0;
        let fields = fields
            .fields
            .into_iter()
            .map(|field| ExceptionField::new(field, &mut argument))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            name: input
                .name
                .unwrap_or_else(|| input.ident.to_string()),
            ident: input.ident,
            base,
            crate_path: CratePath::new(input.crate_path).resolve(),
            fields,
        })
    }

    fn field_types(&self) -> Vec<&Type> {
        let mut types = Vec::new();

        for field in &self.fields {
            if !types.iter().any(|ty| *ty == &field.ty) {
                types.push(&field.ty);
            }
        }

        types
    }

    fn host_exception(&self) -> TokenStream {
        let ident = &self.ident;
        let name = &self.name;
        let crate_path = &self.crate_path;
        let base = self.base.tokens(crate_path);

        quote! {
            impl #crate_path::host::exception::HostException for #ident {
                const NAME: &'static str = #name;

                fn base() -> #crate_path::host::exception::ExceptionClass {
                    #base
                }
            }
        }
    }

    fn into_raise(&self) -> TokenStream {
        let ident = &self.ident;
        let crate_path = &self.crate_path;
        let values = self
            .fields
            .iter()
            .map(ExceptionField::raise);
        let predicates = self
            .field_types()
            .into_iter()
            .map(|ty| quote!(#ty: #crate_path::marshal::ToGuest<B> + 'static,));

        quote! {
            impl<B> #crate_path::host::exception::IntoRaise<B> for #ident
            where
                B: #crate_path::backend::Backend,
                #(#predicates)*
            {
                fn values(
                    self,
                    raise: #crate_path::host::exception::Raise<B>,
                ) -> #crate_path::host::exception::Raise<B> {
                    raise #(#values)*
                }
            }
        }
    }

    fn from_raised(&self) -> TokenStream {
        let ident = &self.ident;
        let crate_path = &self.crate_path;
        let predicates = self
            .field_types()
            .into_iter()
            .map(|ty| {
                quote!(
                    #ty: #crate_path::marshal::FromGuest<B, Owned = #ty>,
                )
            });
        let value = if self.fields.is_empty() {
            quote!(Self)
        } else {
            let fields = self
                .fields
                .iter()
                .map(ExceptionField::read);

            quote!(Self { #(#fields),* })
        };

        quote! {
            impl<B> #crate_path::host::exception::FromRaised<B> for #ident
            where
                B: #crate_path::backend::Backend
                    + #crate_path::backend::BackendValues,
                #(#predicates)*
            {
                fn from_raised<'py>(
                    raised: &#crate_path::host::exception::Raised<'py, '_, B>,
                ) -> ::core::result::Result<Self, #crate_path::errors::Error> {
                    ::core::result::Result::Ok(#value)
                }
            }
        }
    }

    pub(crate) fn expand(&self) -> TokenStream {
        let host_exception = self.host_exception();
        let into_raise = self.into_raise();
        let from_raised = self.from_raised();

        quote! {
            #host_exception
            #into_raise
            #from_raised
        }
    }
}
