use darling::{FromDeriveInput, FromField, ast::Data, util::Flag};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, Generics, Ident, Path, Type, parse_quote};

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
    generics: Generics,
    name: String,
    base: Base,
    crate_path: Path,
    fields: Vec<ExceptionField>,
}

impl HostExceptionDerive {
    pub(crate) fn new(input: &DeriveInput) -> Result<Self, HostMacroError> {
        let input = ExceptionDeriveInput::from_derive_input(input)?;

        if let Some(parameter) = input.generics.type_params().nth(1) {
            return Err(syn::Error::new_spanned(
                parameter,
                "HostException supports at most one generic type parameter",
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
            generics: input.generics,
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

    fn implementation_generics(&self) -> (Generics, Ident) {
        let mut implementation = self.generics.clone();
        let backend = if let Some(parameter) = implementation.type_params().next() {
            parameter.ident.clone()
        } else {
            implementation
                .params
                .push(parse_quote!(B));

            parse_quote!(B)
        };

        (implementation, backend)
    }

    fn host_exception(&self) -> TokenStream {
        let ident = &self.ident;
        let name = &self.name;
        let crate_path = &self.crate_path;
        let base = self.base.tokens(crate_path);
        let (impl_generics, ty_generics, where_clause) = self.generics.split_for_impl();

        quote! {
            impl #impl_generics #crate_path::host::exception::HostException
                for #ident #ty_generics #where_clause
            {
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
        let (mut implementation, backend) = self.implementation_generics();
        let predicates = &mut implementation
            .make_where_clause()
            .predicates;

        predicates.push(parse_quote!(
            #backend: #crate_path::backend::Backend
                + #crate_path::backend::BackendValues
        ));
        for ty in self.field_types() {
            predicates.push(parse_quote!(
                #ty: #crate_path::marshal::ToGuest<#backend> + 'static
            ));
        }

        let (impl_generics, _, where_clause) = implementation.split_for_impl();
        let (_, ty_generics, _) = self.generics.split_for_impl();

        quote! {
            impl #impl_generics #crate_path::host::exception::IntoRaise<#backend>
                for #ident #ty_generics #where_clause
            {
                fn values(
                    self,
                    raise: #crate_path::host::exception::Raise<#backend>,
                ) -> #crate_path::host::exception::Raise<#backend> {
                    raise #(#values)*
                }
            }
        }
    }

    fn from_raised(&self) -> TokenStream {
        let ident = &self.ident;
        let crate_path = &self.crate_path;
        let value = if self.fields.is_empty() {
            quote!(Self)
        } else {
            let fields = self
                .fields
                .iter()
                .map(ExceptionField::read);

            quote!(Self { #(#fields),* })
        };
        let (mut implementation, backend) = self.implementation_generics();
        let predicates = &mut implementation
            .make_where_clause()
            .predicates;

        predicates.push(parse_quote!(
            #backend: #crate_path::backend::Backend
                + #crate_path::backend::BackendValues
        ));
        for ty in self.field_types() {
            predicates.push(parse_quote!(
                #ty: #crate_path::marshal::FromGuest<#backend, Owned = #ty>
            ));
        }

        let (impl_generics, _, where_clause) = implementation.split_for_impl();
        let (_, ty_generics, _) = self.generics.split_for_impl();

        quote! {
            impl #impl_generics #crate_path::host::exception::FromRaised<#backend>
                for #ident #ty_generics #where_clause
            {
                fn from_raised<'py>(
                    raised: &#crate_path::host::exception::Raised<'py, '_, #backend>,
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

#[cfg(test)]
mod tests {
    use quote::quote;
    use syn::{DeriveInput, Item, ItemImpl, parse_quote};

    use super::HostExceptionDerive;

    struct Fixture;

    impl Fixture {
        fn expand(input: DeriveInput) -> Vec<ItemImpl> {
            syn::parse2::<syn::File>(
                HostExceptionDerive::new(&input)
                    .expect("failed to create HostExceptionDerive")
                    .expand(),
            )
            .expect("generated HostException code parses")
            .items
            .into_iter()
            .map(|item| match item {
                Item::Impl(implementation) => implementation,
                _ => panic!("HostException generated a non-impl item"),
            })
            .collect()
        }

        fn render(implementation: &ItemImpl) -> String {
            quote!(#implementation).to_string()
        }
    }

    #[test]
    fn non_generic_exception_keeps_a_synthetic_backend() {
        let expanded = Fixture::expand(parse_quote! {
            struct RequestTimeout {
                #[guestpy(arg)]
                message: String,
            }
        });

        assert_eq!(expanded.len(), 3);
        assert!(expanded[0].generics.params.is_empty());
        assert_eq!(expanded[1].generics.params.len(), 1);
        assert_eq!(expanded[2].generics.params.len(), 1);
        assert!(Fixture::render(&expanded[1]).contains("IntoRaise < B > for RequestTimeout"));
        assert!(Fixture::render(&expanded[2]).contains("FromRaised < B > for RequestTimeout"));
    }

    #[test]
    fn generic_exception_reuses_its_declared_backend() {
        let expanded = Fixture::expand(parse_quote! {
            struct HttpStatusError<Engine: guestpy::backend::Backend> {
                #[guestpy(arg)]
                message: String,
                response: guestpy::handle::Instance<Engine>,
            }
        });
        let host_exception = Fixture::render(&expanded[0]);
        let into_raise = Fixture::render(&expanded[1]);
        let from_raised = Fixture::render(&expanded[2]);

        assert_eq!(expanded.len(), 3);
        assert_eq!(expanded[0].generics.params.len(), 1);
        assert_eq!(expanded[1].generics.params.len(), 1);
        assert_eq!(expanded[2].generics.params.len(), 1);
        assert!(host_exception.contains("HostException for HttpStatusError < Engine >",));
        assert!(into_raise.contains("IntoRaise < Engine > for HttpStatusError < Engine >",));
        assert!(into_raise.contains("ToGuest < Engine > + 'static",));
        assert!(from_raised.contains("FromRaised < Engine > for HttpStatusError < Engine >",));
        assert!(
            from_raised.contains(
                "FromGuest < Engine , Owned = guestpy :: handle :: Instance < Engine > >",
            )
        );
    }

    #[test]
    fn rejects_more_than_one_generic_type_parameter() {
        let error = match HostExceptionDerive::new(&parse_quote! {
            struct Invalid<First, Second> {
                first: First,
                second: Second,
            }
        }) {
            Ok(_) => panic!("HostException accepted two generic type parameters"),
            Err(error) => error,
        };

        assert!(
            error
                .write_errors()
                .to_string()
                .contains("HostException supports at most one generic type parameter")
        );
    }
}
