use darling::{
    FromMeta,
    ast::NestedMeta,
    util::{Flag, PathList},
};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{ImplItem, ItemImpl, Path, parse_quote};

use crate::{
    attributes::HelperAttributes,
    host::{
        HostMacroError,
        callable::{Callable, Parameter, Receiver},
        target::HostTarget,
    },
    naming::{Naming, RenameRule},
    path::CratePath,
};

#[derive(Default, FromMeta)]
#[darling(default)]
struct ClassOptions {
    name: Option<String>,
    rename_all: Option<RenameRule>,
    extends: PathList,
    crate_path: Option<Path>,
}

#[derive(Default, FromMeta)]
#[darling(default)]
struct ClassItemOptions {
    constructor: Flag,
    method: Flag,
    async_method: Flag,
    static_method: Flag,
    get: Flag,
    set: Flag,
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
            self.static_method.is_present(),
            self.get.is_present(),
            self.set.is_present(),
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
    StaticMethod(Callable),
    Getter(Callable),
    Setter(Callable),
    Dunder { dunder: String, callable: Callable },
    Statics(syn::Ident),
    Constant { ident: syn::Ident, name: String },
}

impl ClassMember {
    fn registration(&self, krate: &Path) -> TokenStream {
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

                quote! {
                    builder.#verb(#name, |__guestpy_this, #enter, #args| {
                        #setup

                        __guestpy_this
                            .#ident(#(#bindings),*)
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

                quote! {
                    builder.async_method(#name, |__guestpy_this, #enter, #args| {
                        #setup

                        __guestpy_this
                            .#ident(#(#bindings),*)
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

                quote! {
                    builder.static_method(#name, |#enter, #args| {
                        #setup

                        Self::#ident(#(#bindings),*)
                            .map_err(::core::convert::Into::into)
                    });
                }
            }
            Self::Getter(callable) => {
                let name = callable.name();
                let ident = callable.ident();
                let enter = callable.enter_ident();
                let arguments = callable.accessor_expressions();

                quote! {
                    builder.getter(#name, |__guestpy_this, #enter| {
                        __guestpy_this
                            .#ident(#(#arguments),*)
                            .map_err(::core::convert::Into::into)
                    });
                }
            }
            Self::Setter(callable) => {
                let name = callable.name();
                let ident = callable.ident();
                let enter = callable.enter_ident();
                let arguments = callable.accessor_expressions();

                quote! {
                    builder.setter(#name, |__guestpy_this, #enter, __guestpy_value| {
                        __guestpy_this
                            .#ident(#(#arguments),*)
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

                quote! {
                    builder.dunder(
                        <#krate::host::dunder::Dunder as ::core::str::FromStr>::from_str(#dunder)
                            .expect(#message),
                        |__guestpy_this, #enter, #args| {
                            #setup

                            __guestpy_this
                                .#ident(#(#bindings),*)
                                .map_err(::core::convert::Into::into)
                        },
                    );
                }
            }
            Self::Statics(ident) => quote! {
                builder.statics(|__guestpy_ns| Self::#ident(__guestpy_ns));
            },
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
    extends: Vec<Path>,
    constructor: Option<Callable>,
    members: Vec<ClassMember>,
}

impl HostClassMacro {
    pub(crate) fn new(
        args: TokenStream,
        mut item: ItemImpl,
    ) -> Result<Self, HostMacroError> {
        let definition = HostClassDefinition::from_impl(
            args,
            &mut item,
        )?;

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
    fn from_impl(
        args: TokenStream,
        item: &mut ItemImpl,
    ) -> Result<Self, HostMacroError> {
        let target = HostTarget::from_impl(item, "host_class")?;
        let options = ClassOptions::from_list(
            &NestedMeta::parse_meta_list(args)?,
        )?;
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

        Ok(Self {
            name: options
                .name
                .unwrap_or_else(|| target.name()),
            crate_path: CratePath::new(options.crate_path).resolve(),
            extends: options.extends.iter().cloned().collect(),
            constructor,
            members,
        })
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

    fn render(self, item: &ItemImpl) -> TokenStream {
        let Self {
            name,
            crate_path,
            extends,
            constructor,
            members,
        } = self;
        let target = item.self_ty.as_ref();
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
                fn construct<'py, B>(
                    #enter: &#crate_path::scope::Enter<'py, B>,
                    #args: #crate_path::marshal::args::Args<'py, B>,
                ) -> ::core::result::Result<Self, #crate_path::errors::Error>
                where
                    B: #crate_path::backend::Backend
                        + #crate_path::backend::BackendValues
                        + #crate_path::backend::BackendCallables
                        + #crate_path::backend::BackendClasses,
                {
                    #setup

                    Self::#ident(#(#bindings),*)
                        .map_err(::core::convert::Into::into)
                }
            }
        });
        let has_async_method = members
            .iter()
            .any(|member| matches!(member, ClassMember::AsyncMethod(_)));
        let mut definition_generics = item.generics.clone();

        definition_generics
            .params
            .push(parse_quote!(B));
        definition_generics
            .make_where_clause()
            .predicates
            .push(if has_async_method {
                parse_quote! {
                    B: #crate_path::backend::Backend
                        + #crate_path::backend::BackendValues
                        + #crate_path::backend::BackendCallables
                        + #crate_path::backend::BackendClasses
                        + #crate_path::backend::BackendModules
                        + #crate_path::backend::BackendCoroutines
                        + #crate_path::backend::BackendExceptions
                }
            } else {
                parse_quote! {
                    B: #crate_path::backend::Backend
                        + #crate_path::backend::BackendValues
                        + #crate_path::backend::BackendCallables
                        + #crate_path::backend::BackendClasses
                }
            });

        let (definition_impl_generics, _, definition_where_clause) =
            definition_generics.split_for_impl();
        let registrations = members
            .iter()
            .map(|member| member.registration(&crate_path));
        let bases = extends
            .iter()
            .map(|base| quote!(builder.base::<#base>();));
        let builder = if members.is_empty() && extends.is_empty() {
            quote!(_builder)
        } else {
            quote!(builder)
        };

        quote! {
            impl #impl_generics #crate_path::host::class::HostClass for #target #where_clause {
                const NAME: &'static str = #name;

                #construct
            }

            impl #definition_impl_generics
                #crate_path::host::class::HostClassDefinition<B>
                for #target #definition_where_clause
            {
                fn build(
                    #builder: &mut #crate_path::host::class::ClassBuilder<B, Self>,
                ) {
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
        assert!(output.contains("fn construct"));
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
}
