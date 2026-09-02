use darling::{FromMeta, ast::NestedMeta, util::Flag};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{ImplItem, ImplItemFn, ItemImpl, Path, TypeParamBound, parse_quote};

use crate::{
    attributes::HelperAttributes,
    host::{
        HostMacroError,
        backend::{BackendBounds, BackendOption, BackendParameter},
        callable::{Callable, Parameter, Receiver},
        target::HostTarget,
        types::TypeList,
    },
    naming::{Naming, RenameRule},
    path::CratePath,
};

#[derive(Default, FromMeta)]
#[darling(default)]
struct ClassOptions {
    name: Option<String>,
    rename_all: Option<RenameRule>,
    backend: Option<BackendOption>,
    extends: TypeList,
    generic: Flag,
    crate_path: Option<Path>,
}

#[derive(Default, FromMeta)]
#[darling(default)]
struct ClassItemOptions {
    constructor: Flag,
    method: Flag,
    async_method: Flag,
    raw_method: Flag,
    async_raw_method: Flag,
    class_method: Flag,
    static_method: Flag,
    get: Flag,
    set: Flag,
    delete: Flag,
    statics: Flag,
    constant: Flag,
    dunder: Option<String>,
    name: Option<String>,
}

impl ClassItemOptions {
    fn role_count(&self) -> usize {
        [
            self.constructor.is_present(),
            self.method.is_present(),
            self.async_method.is_present(),
            self.raw_method.is_present(),
            self.async_raw_method.is_present(),
            self.class_method.is_present(),
            self.static_method.is_present(),
            self.get.is_present(),
            self.set.is_present(),
            self.delete.is_present(),
            self.statics.is_present(),
            self.constant.is_present(),
            self.dunder.is_some(),
        ]
        .into_iter()
        .filter(|present| *present)
        .count()
    }
}

enum ClassMember {
    Method { callable: Callable, exclusive: bool },
    AsyncMethod(Callable),
    RawMethod(Callable),
    AsyncRawMethod(Callable),
    ClassMethod(Callable),
    StaticMethod(Callable),
    Getter(Callable),
    Setter(Callable),
    Deleter(Callable),
    Dunder { dunder: String, callable: Callable },
    Statics(syn::Ident),
    Constant { ident: syn::Ident, name: String },
}

impl ClassMember {
    fn ident(&self) -> &syn::Ident {
        match self {
            Self::Method { callable, .. } | Self::Dunder { callable, .. } => callable.ident(),
            Self::AsyncMethod(callable)
            | Self::RawMethod(callable)
            | Self::AsyncRawMethod(callable)
            | Self::ClassMethod(callable)
            | Self::StaticMethod(callable)
            | Self::Getter(callable)
            | Self::Setter(callable)
            | Self::Deleter(callable) => callable.ident(),
            Self::Statics(ident) | Self::Constant { ident, .. } => ident,
        }
    }

    fn registration(&self, krate: &Path, backend: &BackendParameter) -> TokenStream {
        match self {
            Self::Method { callable, exclusive } => {
                let verb = if *exclusive {
                    quote!(method_mut)
                } else {
                    quote!(method)
                };
                let name = callable.name();
                let ident = callable.ident();
                let enter = callable.enter_ident();
                let args = callable.args_ident();
                let bindings = callable.argument_bindings();
                let setup = callable.argument_setup();
                let turbofish = backend.turbofish();

                quote! {
                    builder.#verb(#name, |__guestpy_this, #enter, #args| {
                        #setup

                        __guestpy_this
                            .#ident #turbofish (#(#bindings),*)
                            .map_err(::core::convert::Into::into)
                    });
                }
            }
            Self::AsyncMethod(callable) => {
                let name = callable.name();
                let ident = callable.ident();
                let enter = callable.enter_ident();
                let args = callable.args_ident();
                let bindings = callable.argument_bindings();
                let setup = callable.argument_setup();
                let turbofish = backend.turbofish();

                quote! {
                    builder.async_method(#name, |__guestpy_this, #enter, #args| {
                        #setup

                        __guestpy_this
                            .#ident #turbofish (#(#bindings),*)
                            .map_err(::core::convert::Into::into)
                    });
                }
            }
            Self::RawMethod(callable) => {
                let name = callable.name();
                let ident = callable.ident();
                let enter = callable.enter_ident();
                let args = callable.args_ident();
                let bindings = callable.argument_bindings();
                let setup = callable.argument_setup();
                let turbofish = backend.turbofish();

                quote! {
                    builder.raw_method(#name, |__guestpy_this, #enter, #args| {
                        #setup

                        Self::#ident #turbofish (#(#bindings),*)
                            .map_err(::core::convert::Into::into)
                    });
                }
            }
            Self::AsyncRawMethod(callable) => {
                let name = callable.name();
                let ident = callable.ident();
                let enter = callable.enter_ident();
                let args = callable.args_ident();
                let bindings = callable.argument_bindings();
                let setup = callable.argument_setup();
                let turbofish = backend.turbofish();

                quote! {
                    builder.async_raw_method(#name, |__guestpy_this, #enter, #args| {
                        #setup

                        Self::#ident #turbofish (#(#bindings),*)
                            .map_err(::core::convert::Into::into)
                    });
                }
            }
            Self::ClassMethod(callable) => {
                let name = callable.name();
                let ident = callable.ident();
                let enter = callable.enter_ident();
                let args = callable.args_ident();
                let bindings = callable.argument_bindings();
                let setup = callable.argument_setup();
                let backend_type = backend.ty();
                let turbofish = backend.turbofish();

                quote! {
                    builder.class_method(#name, |__guestpy_enter, __guestpy_class, #args| {
                        let __guestpy_this = &<
                            #krate::handle::Class<#backend_type>
                            as #krate::marshal::FromGuest<#backend_type>
                        >::from_guest(__guestpy_enter, __guestpy_class)?;
                        let #enter = __guestpy_enter;

                        #setup

                        Self::#ident #turbofish (#(#bindings),*)
                            .map_err(::core::convert::Into::into)
                    });
                }
            }
            Self::StaticMethod(callable) => {
                let name = callable.name();
                let ident = callable.ident();
                let enter = callable.enter_ident();
                let args = callable.args_ident();
                let bindings = callable.argument_bindings();
                let setup = callable.argument_setup();
                let turbofish = backend.turbofish();

                quote! {
                    builder.static_method(#name, |#enter, #args| {
                        #setup

                        Self::#ident #turbofish (#(#bindings),*)
                            .map_err(::core::convert::Into::into)
                    });
                }
            }
            Self::Getter(callable) => {
                let name = callable.name();
                let ident = callable.ident();
                let enter = callable.enter_ident();
                let arguments = callable.accessor_expressions();
                let turbofish = backend.turbofish();

                quote! {
                    builder.getter(#name, |__guestpy_this, #enter| {
                        __guestpy_this
                            .#ident #turbofish (#(#arguments),*)
                            .map_err(::core::convert::Into::into)
                    });
                }
            }
            Self::Setter(callable) => {
                let name = callable.name();
                let ident = callable.ident();
                let enter = callable.enter_ident();
                let arguments = callable.accessor_expressions();
                let turbofish = backend.turbofish();

                quote! {
                    builder.setter(#name, |__guestpy_this, #enter, __guestpy_value| {
                        __guestpy_this
                            .#ident #turbofish (#(#arguments),*)
                            .map_err(::core::convert::Into::into)
                    });
                }
            }
            Self::Deleter(callable) => {
                let name = callable.name();
                let ident = callable.ident();
                let enter = callable.enter_ident();
                let arguments = callable.accessor_expressions();
                let turbofish = backend.turbofish();

                quote! {
                    builder.deleter(#name, |__guestpy_this, #enter| {
                        __guestpy_this
                            .#ident #turbofish (#(#arguments),*)
                            .map_err(::core::convert::Into::into)
                    });
                }
            }
            Self::Dunder { dunder, callable } => {
                let ident = callable.ident();
                let enter = callable.enter_ident();
                let args = callable.args_ident();
                let bindings = callable.argument_bindings();
                let setup = callable.argument_setup();
                let message = format!("unknown dunder name {dunder:?} in #[guestpy(dunder = ...)]");
                let turbofish = backend.turbofish();

                quote! {
                    builder.dunder(
                        <#krate::host::dunder::Dunder as ::core::str::FromStr>::from_str(#dunder)
                            .expect(#message),
                        |__guestpy_this, #enter, #args| {
                            #setup

                            __guestpy_this
                                .#ident #turbofish (#(#bindings),*)
                                .map_err(::core::convert::Into::into)
                        },
                    );
                }
            }
            Self::Statics(ident) => {
                let turbofish = backend.turbofish();

                quote! {
                    builder.statics(|__guestpy_ns| Self::#ident #turbofish (__guestpy_ns));
                }
            }
            Self::Constant { ident, name } => quote! {
                builder.constant(#name, Self::#ident);
            },
        }
    }
}

pub(crate) struct HostClassMacro {
    item: ItemImpl,
    definition: HostClassDefinition,
}

struct HostClassDefinition {
    name: String,
    crate_path: Path,
    backend: BackendParameter,
    extends: TypeList,
    generic: bool,
    constructor: Option<Callable>,
    members: Vec<ClassMember>,
    bounds: BackendBounds,
}

impl HostClassMacro {
    pub(crate) fn new(args: TokenStream, mut item: ItemImpl) -> Result<Self, HostMacroError> {
        let definition = HostClassDefinition::from_impl(args, &mut item)?;

        Ok(Self { item, definition })
    }

    pub(crate) fn expand(self) -> TokenStream {
        let Self { item, definition } = self;
        let implementation = definition.render(&item);

        quote! {
            #item
            #implementation
        }
    }
}

impl HostClassDefinition {
    fn from_impl(args: TokenStream, item: &mut ItemImpl) -> Result<Self, HostMacroError> {
        let target = HostTarget::from_impl(item, "host_class")?;
        let options = ClassOptions::from_list(&NestedMeta::parse_meta_list(args)?)?;
        let mut constructor = None;
        let mut members = Vec::new();

        for element in &mut item.items {
            match element {
                ImplItem::Fn(method) => {
                    let helpers = HelperAttributes::take(&mut method.attrs)?;

                    if helpers.is_empty() {
                        continue;
                    }

                    let item_options = ClassItemOptions::from_list(&helpers)?;

                    Self::classify_method(
                        method,
                        item_options,
                        options.rename_all,
                        &mut constructor,
                        &mut members,
                    )?;
                }
                ImplItem::Const(constant) => {
                    let helpers = HelperAttributes::take(&mut constant.attrs)?;

                    if helpers.is_empty() {
                        continue;
                    }

                    let item_options = ClassItemOptions::from_list(&helpers)?;

                    if item_options.role_count() != 1 || !item_options.constant.is_present() {
                        return Err(syn::Error::new(
                            constant.ident.span(),
                            "an exported associated const requires exactly the constant role",
                        )
                        .into());
                    }

                    members.push(ClassMember::Constant {
                        name: Naming::member(
                            &constant.ident,
                            item_options.name,
                            options.rename_all,
                        ),
                        ident: constant.ident.clone(),
                    });
                }
                _ => {}
            }
        }

        let backend = BackendParameter::resolve(options.backend, item, "host_class")?;
        let crate_path = CratePath::new(options.crate_path).resolve();
        let mut bounds = BackendBounds::new(
            &backend,
            Self::capabilities(&crate_path, &constructor, &members),
        );

        for method in Self::exported_methods(item, &constructor, &members) {
            bounds.absorb(&mut method.sig.generics);
        }

        let definition = Self {
            name: options
                .name
                .unwrap_or_else(|| target.name()),
            crate_path,
            backend,
            extends: options.extends,
            generic: options.generic.is_present(),
            constructor,
            members,
            bounds,
        };

        definition.inject_backend_parameter(item);

        Ok(definition)
    }

    fn exported_methods<'item>(
        item: &'item mut ItemImpl,
        constructor: &'item Option<Callable>,
        members: &'item [ClassMember],
    ) -> impl Iterator<Item = &'item mut ImplItemFn> {
        item.items
            .iter_mut()
            .filter_map(|element| match element {
                ImplItem::Fn(method) => Some(method),
                _ => None,
            })
            .filter(|method| {
                constructor
                    .as_ref()
                    .is_some_and(|callable| callable.ident() == &method.sig.ident)
                    || members
                        .iter()
                        .any(|member| member.ident() == &method.sig.ident)
            })
    }

    fn inject_backend_parameter(&self, item: &mut ItemImpl) {
        let Some(backend) = self.backend.introduced() else {
            return;
        };
        let Some(predicate) = self.bounds.predicate() else {
            return;
        };

        for method in Self::exported_methods(item, &self.constructor, &self.members) {
            method.sig.generics.params.push(parse_quote!(#backend));
            method
                .sig
                .generics
                .make_where_clause()
                .predicates
                .push(predicate.clone());
        }
    }

    fn capabilities(
        crate_path: &Path,
        constructor: &Option<Callable>,
        members: &[ClassMember],
    ) -> Vec<TypeParamBound> {
        let mut capabilities = vec![
            parse_quote!(#crate_path::backend::Backend),
            parse_quote!(#crate_path::backend::BackendValues),
            parse_quote!(#crate_path::backend::BackendCallables),
            parse_quote!(#crate_path::backend::BackendClasses),
        ];

        if constructor.iter().any(Callable::asynchronous)
            || members.iter().any(|member| {
                matches!(
                    member,
                    ClassMember::AsyncMethod(_) | ClassMember::AsyncRawMethod(_),
                )
            })
        {
            capabilities.extend([
                parse_quote!(#crate_path::backend::BackendModules),
                parse_quote!(#crate_path::backend::BackendCoroutines),
                parse_quote!(#crate_path::backend::BackendExceptions),
            ]);
        }

        capabilities
    }

    fn classify_method(
        method: &mut syn::ImplItemFn,
        options: ClassItemOptions,
        rename_all: Option<RenameRule>,
        constructor: &mut Option<Callable>,
        members: &mut Vec<ClassMember>,
    ) -> Result<(), HostMacroError> {
        let role_count = options.role_count();

        if role_count == 0 {
            return Err(syn::Error::new(
                method.sig.ident.span(),
                "an exported host class member requires a role",
            )
            .into());
        }

        if role_count > 1 {
            return Err(syn::Error::new(
                method.sig.ident.span(),
                "a host class member may declare only one role",
            )
            .into());
        }

        if options.statics.is_present() {
            if Receiver::of(&method.sig)? != Receiver::None {
                return Err(syn::Error::new(
                    method.sig.ident.span(),
                    "a #[guestpy(statics)] hook cannot have a receiver",
                )
                .into());
            }

            members.push(ClassMember::Statics(method.sig.ident.clone()));

            return Ok(());
        }

        let callable = Callable::parse(
            method,
            Naming::member(&method.sig.ident, options.name.clone(), rename_all),
        )?;

        if callable.asynchronous() {
            return Err(syn::Error::new(
                callable.span(),
                "async fn is unsupported in host classes; for an async method use \
                 #[guestpy(async_method)] on a non-async fn returning \
                 Result<impl core::future::Future<Output = Result<R, Error>> + 'static, E>",
            )
            .into());
        }

        if callable.uses_this()
            && !(options.raw_method.is_present()
                || options.async_raw_method.is_present()
                || options.class_method.is_present())
        {
            return Err(syn::Error::new(
                callable.span(),
                "a #[guestpy(this)] parameter is only valid on a raw_method, async_raw_method, \
                 or class_method",
            )
            .into());
        }

        if options.constructor.is_present() {
            Self::require_receiver(&callable, Receiver::None, "a constructor")?;

            if constructor.is_some() {
                return Err(syn::Error::new(
                    callable.span(),
                    "a host class may have only one constructor",
                )
                .into());
            }

            *constructor = Some(callable);
        } else if options.method.is_present() {
            match callable.receiver() {
                Receiver::Shared => {
                    members.push(ClassMember::Method { callable, exclusive: false })
                }
                Receiver::Exclusive => {
                    members.push(ClassMember::Method { callable, exclusive: true })
                }
                Receiver::None => {
                    return Err(syn::Error::new(
                        callable.span(),
                        "a #[guestpy(method)] requires &self or &mut self; use \
                         #[guestpy(static_method)] for a receiverless function",
                    )
                    .into());
                }
            }
        } else if options.async_method.is_present() {
            Self::require_receiver(&callable, Receiver::Shared, "an async_method")?;
            members.push(ClassMember::AsyncMethod(callable));
        } else if options.raw_method.is_present() {
            Self::require_receiver(&callable, Receiver::None, "a raw_method")?;
            Self::require_this(&callable, "a raw_method")?;
            members.push(ClassMember::RawMethod(callable));
        } else if options.async_raw_method.is_present() {
            Self::require_receiver(&callable, Receiver::None, "an async_raw_method")?;
            Self::require_this(&callable, "an async_raw_method")?;
            members.push(ClassMember::AsyncRawMethod(callable));
        } else if options.class_method.is_present() {
            Self::require_receiver(&callable, Receiver::None, "a class_method")?;
            Self::require_this(&callable, "a class_method")?;
            members.push(ClassMember::ClassMethod(callable));
        } else if options.static_method.is_present() {
            Self::require_receiver(&callable, Receiver::None, "a static_method")?;
            members.push(ClassMember::StaticMethod(callable));
        } else if options.get.is_present() {
            Self::require_receiver(&callable, Receiver::Shared, "a getter")?;

            if callable
                .parameters()
                .iter()
                .any(Parameter::consumes_arg)
            {
                return Err(syn::Error::new(
                    callable.span(),
                    "a #[guestpy(get)] getter cannot accept guest arguments",
                )
                .into());
            }

            members.push(ClassMember::Getter(callable));
        } else if options.set.is_present() {
            Self::require_receiver(&callable, Receiver::Exclusive, "a setter")?;

            if callable
                .parameters()
                .iter()
                .filter(|parameter| parameter.consumes_arg())
                .count()
                != 1
                || callable
                    .parameters()
                    .iter()
                    .any(Parameter::is_rest_or_borrow_or_kw)
            {
                return Err(syn::Error::new(
                    callable.span(),
                    "a #[guestpy(set)] setter requires exactly one plain value parameter",
                )
                .into());
            }

            members.push(ClassMember::Setter(callable));
        } else if options.delete.is_present() {
            Self::require_receiver(&callable, Receiver::Exclusive, "a deleter")?;

            if callable
                .parameters()
                .iter()
                .any(Parameter::consumes_arg)
            {
                return Err(syn::Error::new(
                    callable.span(),
                    "a #[guestpy(delete)] deleter cannot accept guest arguments",
                )
                .into());
            }

            members.push(ClassMember::Deleter(callable));
        } else {
            Self::require_receiver(&callable, Receiver::Shared, "a dunder")?;
            members.push(ClassMember::Dunder {
                dunder: options
                    .dunder
                    .expect("dunder is the only remaining role"),
                callable,
            });
        }

        Ok(())
    }

    fn require_receiver(
        callable: &Callable,
        expected: Receiver,
        subject: &str,
    ) -> Result<(), HostMacroError> {
        if callable.receiver() == expected {
            return Ok(());
        }

        Err(syn::Error::new(
            callable.span(),
            match expected {
                Receiver::None => format!("{subject} cannot have a receiver"),
                Receiver::Shared => format!("{subject} requires &self"),
                Receiver::Exclusive => format!("{subject} requires &mut self"),
            },
        )
        .into())
    }

    fn require_this(callable: &Callable, subject: &str) -> Result<(), HostMacroError> {
        if callable
            .parameters()
            .iter()
            .filter(|parameter| parameter.is_this())
            .count()
            == 1
        {
            return Ok(());
        }

        Err(syn::Error::new(
            callable.span(),
            format!("{subject} requires exactly one #[guestpy(this)] parameter"),
        )
        .into())
    }

    fn render(self, item: &ItemImpl) -> TokenStream {
        let Self {
            name,
            crate_path,
            backend,
            extends,
            generic,
            constructor,
            members,
            bounds,
        } = self;
        let target = item.self_ty.as_ref();
        let backend_type = backend.ty();
        let turbofish = backend.turbofish();
        let mut generics = item.generics.clone();

        generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(#target: 'static));

        let (impl_generics, _, where_clause) = generics.split_for_impl();
        let construct = constructor.map(|callable| {
            let ident = callable.ident();
            let enter = callable.enter_ident();
            let args = callable.args_ident();
            let bindings = callable.argument_bindings();
            let setup = callable.argument_setup();

            quote! {
                fn construct<'py>(
                    #enter: &#crate_path::scope::Enter<'py, #backend_type>,
                    #args: #crate_path::marshal::args::Args<'py, #backend_type>,
                ) -> ::core::result::Result<Self, #crate_path::errors::Error> {
                    #setup

                    Self::#ident #turbofish (#(#bindings),*)
                        .map_err(::core::convert::Into::into)
                }
            }
        });
        let definition_generics = backend.definition_generics(&item.generics, &bounds);

        let (definition_impl_generics, _, definition_where_clause) =
            definition_generics.split_for_impl();
        let registrations = members
            .iter()
            .map(|member| member.registration(&crate_path, &backend));
        let bases = extends
            .iter()
            .map(|base| quote!(builder.base::<#base>();));
        let builder = if members.is_empty() && extends.is_empty() && !generic {
            quote!(_builder)
        } else {
            quote!(builder)
        };
        let generic_hook = generic.then(|| quote!(builder.generic();));

        quote! {
            impl #impl_generics #crate_path::host::class::HostClass for #target #where_clause {
                const NAME: &'static str = #name;
            }

            impl #definition_impl_generics
                #crate_path::host::class::HostClassDefinition<#backend_type>
                for #target #definition_where_clause
            {
                #construct

                fn build(
                    #builder: &mut #crate_path::host::class::ClassBuilder<#backend_type, Self>,
                ) {
                    #generic_hook
                    #(#registrations)*
                    #(#bases)*
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;
    use syn::parse_quote;

    use crate::host::HostMacroError;

    use super::HostClassMacro;

    fn expand(args: proc_macro2::TokenStream, item: syn::ItemImpl) -> String {
        HostClassMacro::new(args, item)
            .unwrap()
            .expand()
            .to_string()
    }

    #[test]
    fn generates_constructor_receivers_getter_and_dunder() {
        let output = expand(
            quote!(name = "Vector2", extends(BaseVector), crate_path = crate),
            parse_quote! {
                impl Vector2 {
                    #[guestpy(constructor)]
                    fn new(x: i64, y: i64) -> Result<Self, Error> {
                        Ok(Self { x, y })
                    }

                    #[guestpy(method)]
                    fn length(&self) -> Result<i64, Error> {
                        Ok(self.x + self.y)
                    }

                    #[guestpy(method)]
                    fn translate(&mut self, dx: i64) -> Result<(), Error> {
                        self.x += dx;

                        Ok(())
                    }

                    #[guestpy(get)]
                    fn x(&self) -> Result<i64, Error> {
                        Ok(self.x)
                    }

                    #[guestpy(dunder = "__repr__")]
                    fn repr(&self) -> Result<String, Error> {
                        Ok("Vector2".into())
                    }
                }
            },
        );

        assert!(output.contains("HostClass for Vector2"));
        assert!(output.contains("const NAME : & 'static str = \"Vector2\""));
        assert!(
            output.find("fn construct").unwrap()
                > output
                    .find("HostClassDefinition")
                    .unwrap(),
        );
        assert!(output.contains("HostClassDefinition < B > for Vector2"));
        assert!(output.contains("builder . method (\"length\""));
        assert!(output.contains("builder . method_mut (\"translate\""));
        assert!(output.contains("builder . getter (\"x\""));
        assert!(output.contains("builder . dunder"));
        assert!(output.contains("from_str (\"__repr__\")"));
        assert!(output.contains("builder . base :: < BaseVector > ()"));
        assert!(output.contains(". finish () ?"));
        assert!(output.contains("BackendClasses"));
    }

    #[test]
    fn generates_raw_class_and_delete_roles_and_a_generic_hook() {
        let output = expand(
            quote!(name = "Contract", backend = B, generic, crate_path = crate,),
            parse_quote! {
                impl<B> Contract<B> {
                    #[guestpy(raw_method)]
                    fn describe(
                        #[guestpy(this)] this: &Object<B>,
                    ) -> Result<String, Error> {
                        this.type_name()
                    }

                    #[guestpy(class_method)]
                    fn of(
                        #[guestpy(this)] cls: &Class<B>,
                    ) -> Result<String, Error> {
                        cls.name()
                    }

                    #[guestpy(delete)]
                    fn clear(&mut self) -> Result<(), Error> {
                        Ok(())
                    }
                }
            },
        );

        assert!(output.contains("builder . generic ()"));
        assert!(output.contains("raw_method (\"describe\""));
        assert!(output.contains("class_method (\"of\""));
        assert!(output.contains("deleter (\"clear\""));
        assert!(output.contains("HostClassDefinition < B > for Contract < B >"));
    }

    #[test]
    fn widens_the_capability_bound_for_an_async_method() {
        let output = expand(
            quote!(name = "Session", crate_path = crate),
            parse_quote! {
                impl Session {
                    #[guestpy(async_method)]
                    fn refresh(
                        &self,
                    ) -> Result<
                        impl core::future::Future<Output = Result<(), Error>> + 'static,
                        Error,
                    > {
                        Ok(async { Ok(()) })
                    }
                }
            },
        );

        assert!(output.contains("HostClassDefinition < B > for Session"));
        assert!(output.contains("builder . async_method (\"refresh\""));
        assert!(output.contains("BackendClasses"));
        assert!(output.contains("BackendModules"));
        assert!(output.contains("BackendCoroutines"));
        assert!(output.contains("BackendExceptions"));
    }

    #[test]
    fn omits_construct_when_no_constructor() {
        let output = expand(
            quote!(name = "Session", crate_path = crate),
            parse_quote! {
                impl Session {
                    #[guestpy(method)]
                    fn id(&self) -> Result<String, Error> {
                        Ok(self.id.clone())
                    }
                }
            },
        );

        assert!(output.contains("builder . method (\"id\""));
    }

    #[test]
    fn skips_unannotated_methods_and_preserves_them() {
        let output = expand(
            quote!(name = "Counter", crate_path = crate),
            parse_quote! {
                impl Counter {
                    #[guestpy(constructor)]
                    fn new() -> Result<Self, Error> {
                        Ok(Self)
                    }

                    #[allow(dead_code)]
                    fn helper(&self) {}
                }
            },
        );

        assert!(output.contains("fn helper"));
        assert!(output.contains("allow (dead_code)"));
    }

    #[test]
    fn rejects_async_fn_and_receiverless_method() {
        assert!(
            HostClassMacro::new(
                quote!(name = "Bad", crate_path = crate),
                parse_quote! {
                    impl Bad {
                        #[guestpy(async_method)]
                        async fn go(&self) -> Result<i32, Error> {
                            Ok(1)
                        }
                    }
                },
            )
            .is_err(),
        );

        assert!(
            HostClassMacro::new(
                quote!(name = "Bad", crate_path = crate),
                parse_quote! {
                    impl Bad {
                        #[guestpy(method)]
                        fn go() -> Result<i32, Error> {
                            Ok(1)
                        }
                    }
                },
            )
            .is_err(),
        );
    }

    #[test]
    fn reuses_a_declared_backend_parameter() {
        let output = expand(
            quote!(name = "Envelope", backend = B, crate_path = crate),
            parse_quote! {
                impl<B: Backend> Envelope<B> {
                    #[guestpy(constructor)]
                    fn new(payload: Object<B>) -> Result<Self, Error> {
                        Ok(Self { payload })
                    }

                    #[guestpy(get)]
                    fn payload(&self) -> Result<Object<B>, Error> {
                        Ok(self.payload.clone())
                    }
                }
            },
        );

        assert!(output.contains("HostClass for Envelope < B >"));
        assert!(output.contains("HostClassDefinition < B > for Envelope < B >"));
        assert!(output.contains("ClassBuilder < B , Self >"));
        assert!(!output.contains("__GuestpyBackend"));
    }

    #[test]
    fn pins_a_class_to_a_concrete_backend() {
        let output = expand(
            quote!(name = "Envelope", backend(pin = RustPython), crate_path = crate),
            parse_quote! {
                impl Envelope {
                    #[guestpy(class_method)]
                    fn of(#[guestpy(this)] cls: &Class<RustPython>) -> Result<String, Error> {
                        cls.name()
                    }
                }
            },
        );

        assert!(output.contains("HostClassDefinition < RustPython > for Envelope"));
        assert!(output.contains("ClassBuilder < RustPython , Self >"));
        assert!(output.contains("Class < RustPython >"));
        assert!(!output.contains("HostClassDefinition < B >"));
    }

    #[test]
    fn introduces_a_backend_parameter_on_members() {
        let output = expand(
            quote!(name = "Contract", backend = B, generic, crate_path = crate),
            parse_quote! {
                impl Contract {
                    #[guestpy(constructor)]
                    fn new() -> Result<Self, Error> {
                        Ok(Self)
                    }

                    #[guestpy(raw_method)]
                    fn invoke(#[guestpy(this)] this: &Object<B>) -> Result<String, Error> {
                        this.type_name()
                    }
                }
            },
        );

        assert!(output.contains("fn new < B > ()"));
        assert!(output.contains("fn invoke < B > (this : & Object < B >)"));
        assert!(output.contains("B : crate :: backend :: Backend"));
        assert!(output.contains("Self :: new :: < B > ()"));
        assert!(output.contains("Self :: invoke :: < B > (__guestpy_arg0)"));
        assert!(output.contains("HostClassDefinition < B > for Contract"));
        assert!(output.contains("ClassBuilder < B , Self >"));
    }

    #[test]
    fn leaves_members_alone_for_a_declared_backend() {
        let output = expand(
            quote!(name = "Envelope", backend = B, crate_path = crate),
            parse_quote! {
                impl<B: Backend> Envelope<B> {
                    #[guestpy(get)]
                    fn payload(&self) -> Result<Object<B>, Error> {
                        Ok(self.payload.clone())
                    }
                }
            },
        );

        assert!(output.contains("fn payload (& self)"));
        assert!(!output.contains(":: < B > ("));
    }

    #[test]
    fn accepts_a_lifetime_on_a_member() {
        let output = expand(
            quote!(name = "Contract", backend = B, crate_path = crate),
            parse_quote! {
                impl Contract {
                    #[guestpy(raw_method)]
                    fn invoke<'py>(
                        #[guestpy(this)] this: &Object<B>,
                        #[guestpy(enter)] enter: &Enter<'py, B>,
                    ) -> Result<String, Error> {
                        this.type_name()
                    }
                }
            },
        );

        assert!(output.contains("fn invoke < 'py , B >"));
    }

    #[test]
    fn rejects_a_type_parameter_on_a_member() {
        let Err(HostMacroError::Syntax(error)) = HostClassMacro::new(
            quote!(name = "Contract", crate_path = crate),
            parse_quote! {
                impl Contract {
                    #[guestpy(static_method)]
                    fn convert<T>(value: T) -> Result<String, Error> {
                        Ok(String::new())
                    }
                }
            },
        ) else {
            panic!("a member declaring a type parameter returns a syntax error");
        };

        assert!(error.to_string().contains("backend = <name>"));
    }

    #[test]
    fn rejects_a_qualified_backend_name() {
        assert!(
            HostClassMacro::new(
                quote!(name = "Envelope", backend = guestpy::CPython, crate_path = crate),
                parse_quote! {
                    impl Envelope {}
                },
            )
            .is_err(),
        );
    }

    #[test]
    fn hoists_a_member_bound_onto_the_definition() {
        let output = expand(
            quote!(backend = B, crate_path = crate),
            parse_quote! {
                impl Contract {
                    #[guestpy(raw_method)]
                    fn invoke(#[guestpy(this)] this: &Object<B>) -> Result<Object<B>, Error>
                    where
                        B: InterpreterBackend,
                    {
                        Ok(this.clone())
                    }
                }
            },
        );

        assert!(output.contains("HostClassDefinition < B > for Contract where B :"));
        assert!(output.contains("+ InterpreterBackend"));
        assert_eq!(output.matches("InterpreterBackend").count(), 2);
    }

    #[test]
    fn hoists_a_member_bound_for_a_declared_backend() {
        let output = expand(
            quote!(backend = B, crate_path = crate),
            parse_quote! {
                impl<B> Contract<B> {
                    #[guestpy(raw_method)]
                    fn invoke(#[guestpy(this)] this: &Object<B>) -> Result<Object<B>, Error>
                    where
                        B: InterpreterBackend,
                    {
                        Ok(this.clone())
                    }
                }
            },
        );

        assert!(output.contains("+ InterpreterBackend"));
        assert!(!output.contains("Self :: invoke :: <"));
    }

    #[test]
    fn retains_a_member_bound_that_does_not_name_the_backend() {
        let output = expand(
            quote!(backend = B, crate_path = crate),
            parse_quote! {
                impl Contract {
                    #[guestpy(raw_method)]
                    fn invoke(#[guestpy(this)] this: &Object<B>) -> Result<Object<B>, Error>
                    where
                        Self: Sized,
                    {
                        Ok(this.clone())
                    }
                }
            },
        );

        assert!(output.contains("where Self : Sized"));
        assert!(!output.contains("HostClassDefinition < B > for Contract where Self : Sized"));
    }

    #[test]
    fn renames_the_synthesized_backend_when_b_is_taken() {
        let output = expand(
            quote!(name = "Wrapper", crate_path = crate),
            parse_quote! {
                impl<B: Serialize> Wrapper<B> {
                    #[guestpy(method)]
                    fn describe(&self) -> Result<String, Error> {
                        Ok(String::new())
                    }
                }
            },
        );

        assert!(output.contains("HostClassDefinition < __GuestpyBackend > for Wrapper < B >"));
        assert!(output.contains("ClassBuilder < __GuestpyBackend , Self >"));
    }

    #[test]
    fn rejects_a_backend_bounded_parameter_without_a_declaration() {
        let Err(HostMacroError::Syntax(error)) = HostClassMacro::new(
            quote!(name = "Envelope", crate_path = crate),
            parse_quote! {
                impl<B: Backend> Envelope<B> {
                    #[guestpy(method)]
                    fn describe(&self) -> Result<String, Error> {
                        Ok(String::new())
                    }
                }
            },
        ) else {
            panic!("a Backend-bounded parameter without a declaration returns a syntax error",);
        };

        assert!(
            error
                .to_string()
                .contains("backend = B")
        );
    }
}
