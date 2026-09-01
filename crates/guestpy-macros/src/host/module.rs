use darling::{
    FromMeta,
    ast::NestedMeta,
    util::{Flag, PathList},
};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{FnArg, ImplItem, ImplItemFn, ItemImpl, Path, Type, spanned::Spanned};

use crate::{
    attributes::HelperAttributes,
    host::{
        backend::BackendParameter,
        callable::{Callable, Parameter, Receiver},
        target::HostTarget,
        types::TypeList,
        HostMacroError,
    },
    naming::{Naming, RenameRule},
    path::CratePath,
};

#[derive(Default, FromMeta)]
#[darling(default)]
struct ModuleOptions {
    name: Option<String>,
    rename_all: Option<RenameRule>,
    method_name: Option<syn::Ident>,
    backend: Option<Type>,
    classes: TypeList,
    exceptions: PathList,
    crate_path: Option<Path>,
}

#[derive(Default, FromMeta)]
#[darling(default)]
struct ModuleItemOptions {
    function: Flag,
    getter: Flag,
    constant: Flag,
    init: Flag,
    object: Flag,
    name: Option<String>,
}

impl ModuleItemOptions {
    fn role_count(&self) -> usize {
        [
            self.function.is_present(),
            self.getter.is_present(),
            self.constant.is_present(),
            self.init.is_present(),
            self.object.is_present(),
        ]
        .into_iter()
        .filter(|present| *present)
        .count()
    }
}

enum ModuleMember {
    Function(Callable),
    Getter(Callable),
    Init {
        ident: syn::Ident,
        shared: bool,
        takes_enter: bool,
    },
    Object {
        ident: syn::Ident,
        name: String,
        shared: bool,
    },
    Constant {
        ident: syn::Ident,
        name: String,
    },
}

impl ModuleMember {
    fn is_stateful(&self) -> bool {
        match self {
            Self::Function(callable) | Self::Getter(callable) => {
                matches!(callable.receiver(), Receiver::Shared)
            }
            Self::Init { shared, .. } | Self::Object { shared, .. } => *shared,
            Self::Constant { .. } => false,
        }
    }

    fn registration(&self) -> TokenStream {
        match self {
            Self::Function(callable) if callable.asynchronous() => {
                let name = callable.name();
                let ident = callable.ident();
                let enter = callable.enter_ident();
                let args = callable.args_ident();
                let bindings = callable.argument_bindings();
                let setup = callable.argument_setup();

                quote! {
                    .async_function(#name, |#enter, #args| {
                        #setup

                        ::core::result::Result::Ok(async move {
                            Self::#ident(#(#bindings),*)
                                .await
                                .map_err(::core::convert::Into::into)
                        })
                    })
                }
            }
            Self::Function(callable) => {
                let shared = matches!(callable.receiver(), Receiver::Shared);
                let name = callable.name();
                let enter = callable.enter_ident();
                let args = callable.args_ident();
                let bindings = callable.argument_bindings();
                let setup = callable.argument_setup();
                let invocation = Self::invoke(
                    shared,
                    callable.ident(),
                    &bindings
                        .iter()
                        .map(|binding| quote!(#binding))
                        .collect::<Vec<_>>(),
                );
                let closure = Self::state_closure(
                    shared,
                    quote!(
                        |#enter, #args| {
                            #setup

                            #invocation.map_err(::core::convert::Into::into)
                        }
                    ),
                );

                quote!(.function(#name, #closure))
            }
            Self::Getter(callable) => {
                let shared = matches!(callable.receiver(), Receiver::Shared);
                let name = callable.name();
                let enter = callable.enter_ident();
                let arguments = callable.accessor_expressions();
                let invocation = Self::invoke(shared, callable.ident(), &arguments);
                let closure = Self::state_closure(
                    shared,
                    quote!(
                        |#enter| {
                            #invocation.map_err(::core::convert::Into::into)
                        }
                    ),
                );

                quote!(.getter(#name, #closure))
            }
            Self::Init { ident, shared, takes_enter } => {
                let enter = if *takes_enter {
                    quote!(__guestpy_enter)
                } else {
                    quote!(_enter)
                };
                let arguments = if *takes_enter {
                    vec![quote!(__guestpy_enter)]
                } else {
                    Vec::new()
                };
                let invocation = Self::invoke(*shared, ident, &arguments);
                let closure = Self::state_closure(
                    *shared,
                    quote!(
                        |#enter| {
                            #invocation.map_err(::core::convert::Into::into)
                        }
                    ),
                );

                quote!(.init(#closure))
            }
            Self::Object { ident, name, shared } => {
                let invocation = Self::invoke(*shared, ident, &[quote!(__guestpy_ns)]);
                let closure = Self::state_closure(*shared, quote!(|__guestpy_ns| #invocation));

                quote!(.object(#name, #closure))
            }
            Self::Constant { ident, name } => quote!(.constant(#name, Self::#ident)),
        }
    }

    fn invoke(shared: bool, ident: &syn::Ident, arguments: &[TokenStream]) -> TokenStream {
        if shared {
            quote!(__guestpy_state.#ident(#(#arguments),*))
        } else {
            quote!(Self::#ident(#(#arguments),*))
        }
    }

    fn state_closure(shared: bool, closure: TokenStream) -> TokenStream {
        if shared {
            quote! {{
                let __guestpy_state = ::std::rc::Rc::clone(&__guestpy_state);

                move #closure
            }}
        } else {
            closure
        }
    }
}

pub(crate) struct HostModuleMacro {
    item: ItemImpl,
    definition: HostModuleDefinition,
}

struct HostModuleDefinition {
    name: String,
    method_name: syn::Ident,
    crate_path: Path,
    backend: BackendParameter,
    classes: TypeList,
    exceptions: Vec<String>,
    members: Vec<ModuleMember>,
    needs_state: bool,
}

impl HostModuleMacro {
    pub(crate) fn new(args: TokenStream, mut item: ItemImpl) -> Result<Self, HostMacroError> {
        let definition = HostModuleDefinition::from_impl(args, &mut item)?;

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

impl HostModuleDefinition {
    fn from_impl(args: TokenStream, item: &mut ItemImpl) -> Result<Self, HostMacroError> {
        let target = HostTarget::from_impl(item, "host_module")?;
        let options = ModuleOptions::from_list(&NestedMeta::parse_meta_list(args)?)?;
        let mut members = Vec::new();

        for element in &mut item.items {
            match element {
                ImplItem::Fn(method) => {
                    let helpers = HelperAttributes::take(&mut method.attrs)?;

                    if helpers.is_empty() {
                        continue;
                    }

                    let item_options = ModuleItemOptions::from_list(&helpers)?;

                    Self::classify_method(method, item_options, options.rename_all, &mut members)?;
                }
                ImplItem::Const(constant) => {
                    let helpers = HelperAttributes::take(&mut constant.attrs)?;

                    if helpers.is_empty() {
                        continue;
                    }

                    let item_options = ModuleItemOptions::from_list(&helpers)?;

                    if item_options.role_count() != 1 || !item_options.constant.is_present() {
                        return Err(syn::Error::new(
                            constant.ident.span(),
                            "an exported associated const requires exactly the constant role",
                        )
                        .into());
                    }

                    members.push(ModuleMember::Constant {
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

        if members
            .iter()
            .filter(|member| matches!(member, ModuleMember::Init { .. }))
            .count()
            > 1
        {
            return Err(syn::Error::new(
                item.impl_token.span(),
                "a host module may declare only one init hook",
            )
            .into());
        }

        let needs_state = members
            .iter()
            .any(ModuleMember::is_stateful);

        Ok(Self {
            name: options
                .name
                .unwrap_or_else(|| target.name()),
            method_name: options
                .method_name
                .unwrap_or_else(|| syn::Ident::new("module", Span::call_site())),
            crate_path: CratePath::new(options.crate_path).resolve(),
            backend: BackendParameter::resolve(options.backend, item, "host_module")?,
            classes: options.classes,
            exceptions: options
                .exceptions
                .iter()
                .filter_map(|path| {
                    path.segments
                        .last()
                        .map(|segment| segment.ident.to_string())
                })
                .collect(),
            members,
            needs_state,
        })
    }

    fn classify_method(
        method: &mut ImplItemFn,
        options: ModuleItemOptions,
        rename_all: Option<RenameRule>,
        members: &mut Vec<ModuleMember>,
    ) -> Result<(), HostMacroError> {
        let role_count = options.role_count();

        if role_count == 0 {
            return Err(syn::Error::new(
                method.sig.ident.span(),
                "an exported host module member requires a role",
            )
            .into());
        }

        if role_count > 1 {
            return Err(syn::Error::new(
                method.sig.ident.span(),
                "a host module member may declare only one role",
            )
            .into());
        }

        if options.init.is_present() {
            Self::reject_async(method, "an init hook")?;

            let shared = Self::receiver_state(
                Receiver::of(&method.sig)?,
                method.sig.ident.span(),
                "an init hook",
            )?;
            let parameters = method
                .sig
                .inputs
                .iter()
                .filter(|argument| matches!(argument, FnArg::Typed(_)))
                .count();

            if parameters > 1 {
                return Err(syn::Error::new(
                    method.sig.ident.span(),
                    "an init hook takes at most the enter reference",
                )
                .into());
            }

            members.push(ModuleMember::Init {
                ident: method.sig.ident.clone(),
                shared,
                takes_enter: parameters == 1,
            });

            return Ok(());
        }

        if options.object.is_present() {
            Self::reject_async(method, "an object hook")?;

            let shared = Self::receiver_state(
                Receiver::of(&method.sig)?,
                method.sig.ident.span(),
                "an object hook",
            )?;

            members.push(ModuleMember::Object {
                name: Naming::member(&method.sig.ident, options.name.clone(), rename_all),
                ident: method.sig.ident.clone(),
                shared,
            });

            return Ok(());
        }

        if options.constant.is_present() {
            return Err(syn::Error::new(
                method.sig.ident.span(),
                "the constant role applies to an associated const, not a function",
            )
            .into());
        }

        let callable = Callable::parse(
            method,
            Naming::member(&method.sig.ident, options.name.clone(), rename_all),
        )?;

        if callable.uses_this() {
            return Err(syn::Error::new(
                callable.span(),
                "a #[guestpy(this)] parameter is only valid on a host class raw_method, \
                 async_raw_method, or class_method",
            )
            .into());
        }

        if options.getter.is_present() {
            if callable.asynchronous() {
                return Err(syn::Error::new(
                    callable.span(),
                    "a #[guestpy(getter)] cannot be async",
                )
                .into());
            }

            Self::receiver_state(callable.receiver(), callable.span(), "a getter")?;

            if callable
                .parameters()
                .iter()
                .any(Parameter::consumes_arg)
            {
                return Err(syn::Error::new(
                    callable.span(),
                    "a #[guestpy(getter)] cannot accept guest arguments",
                )
                .into());
            }

            members.push(ModuleMember::Getter(callable));

            return Ok(());
        }

        match (callable.receiver(), callable.asynchronous()) {
            (Receiver::Exclusive, _) => Err(syn::Error::new(
                callable.span(),
                "a #[guestpy(function)] cannot take &mut self; share state through &self and \
                 interior mutability",
            )
            .into()),
            (Receiver::Shared, true) => Err(syn::Error::new(
                callable.span(),
                "a stateful (&self) async module function is unsupported; make it non-async, or \
                 make it receiverless",
            )
            .into()),
            (_, true)
                if callable
                    .parameters()
                    .iter()
                    .any(Parameter::is_enter) =>
            {
                Err(syn::Error::new(
                    callable.span(),
                    "an async module function cannot take a #[guestpy(enter)] parameter",
                )
                .into())
            }
            _ => {
                members.push(ModuleMember::Function(callable));

                Ok(())
            }
        }
    }

    fn receiver_state(
        receiver: Receiver,
        span: Span,
        subject: &str,
    ) -> Result<bool, HostMacroError> {
        match receiver {
            Receiver::None => Ok(false),
            Receiver::Shared => Ok(true),
            Receiver::Exclusive => Err(syn::Error::new(
                span,
                format!(
                    "{subject} cannot take &mut self; a host module shares state through &self"
                ),
            )
            .into()),
        }
    }

    fn reject_async(method: &ImplItemFn, subject: &str) -> Result<(), HostMacroError> {
        if method.sig.asyncness.is_some() {
            return Err(syn::Error::new_spanned(
                method.sig.asyncness,
                format!("{subject} cannot be async"),
            )
            .into());
        }

        Ok(())
    }

    fn render(self, item: &ItemImpl) -> TokenStream {
        let Self {
            name,
            method_name,
            crate_path,
            backend,
            classes,
            exceptions,
            members,
            needs_state,
        } = self;
        let target = item.self_ty.as_ref();
        let generics = item.generics.clone();
        let (impl_generics, _, where_clause) = generics.split_for_impl();
        let registrations = members
            .iter()
            .map(ModuleMember::registration);
        let exception_registrations = exceptions.iter().map(|exception| {
            quote!(.exception(
                #exception,
                #crate_path::host::exception::ExceptionBase::Exception,
            ))
        });
        let class_registrations = classes
            .iter()
            .map(|class| quote!(.class::<#class>()));
        let receiver = if needs_state { quote!(self) } else { quote!() };
        let state = if needs_state {
            quote!(let __guestpy_state = ::std::rc::Rc::new(self);)
        } else {
            quote!()
        };
        let has_async_function = members.iter().any(
            |member| matches!(member, ModuleMember::Function(callable) if callable.asynchronous()),
        );
        let backend_type = backend.ty();
        let method_generics = backend.method_generics();
        let mut capabilities = vec![
            quote!(#crate_path::backend::Backend),
            quote!(#crate_path::backend::BackendValues),
            quote!(#crate_path::backend::BackendCallables),
        ];

        if !classes.is_empty() {
            capabilities.push(quote!(#crate_path::backend::BackendClasses));
        }

        if has_async_function {
            capabilities.extend([
                quote!(#crate_path::backend::BackendModules),
                quote!(#crate_path::backend::BackendCoroutines),
            ]);
        }

        if has_async_function || !exceptions.is_empty() {
            capabilities.push(quote!(#crate_path::backend::BackendExceptions));
        }

        let method_where_clause = backend
            .capability_predicate(&capabilities)
            .map(|predicate| {
                let class_definition_bounds = classes.iter().map(|class| {
                    quote!(#class: #crate_path::host::class::HostClassDefinition<#backend_type>,)
                });

                quote! {
                    where
                        #predicate,
                        #(#class_definition_bounds)*
                }
            });

        quote! {
            impl #impl_generics #target #where_clause {
                pub fn #method_name #method_generics (#receiver)
                    -> #crate_path::host::module::ModuleSpec<#backend_type>
                #method_where_clause
                {
                    #state

                    #crate_path::host::module::ModuleSpec::<#backend_type>::new(#name)
                        #(#registrations)*
                        #(#exception_registrations)*
                        #(#class_registrations)*
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;
    use syn::parse_quote;

    use super::HostModuleMacro;

    fn expand(args: proc_macro2::TokenStream, item: syn::ItemImpl) -> String {
        HostModuleMacro::new(args, item)
            .unwrap()
            .expand()
            .to_string()
    }

    #[test]
    fn generates_stateful_module_with_rc_and_registrations() {
        let output = expand(
            quote!(
                name = "geometry",
                exceptions(GeometryError),
                classes(Vector2),
                crate_path = crate,
            ),
            parse_quote! {
                impl Geometry {
                    #[guestpy(constant)]
                    const API_VERSION: i64 = 1;

                    #[guestpy(function)]
                    fn hypot(&self, x: f64, y: f64) -> Result<f64, Error> {
                        Ok((x * x + y * y).sqrt() * self.scale)
                    }

                    #[guestpy(getter)]
                    fn version(&self) -> Result<i64, Error> {
                        Ok(self.version)
                    }

                    #[guestpy(init)]
                    fn setup(&self, enter: &Enter<'_, B>) -> Result<(), Error> {
                        let _ = enter;

                        Ok(())
                    }
                }
            },
        );

        assert!(output.contains("pub fn module < B >"));
        assert!(output.contains("(self)"));
        assert!(output.contains("let __guestpy_state = :: std :: rc :: Rc :: new (self)"));
        assert!(output.contains(":: std :: rc :: Rc :: clone (& __guestpy_state)"));
        assert!(output.contains("ModuleSpec :: < B > :: new (\"geometry\")"));
        assert!(output.contains(". constant (\"API_VERSION\" , Self :: API_VERSION)"));
        assert!(output.contains(". function (\"hypot\""));
        assert!(output.contains(". getter (\"version\""));
        assert!(output.contains(". init ("));
        assert!(output.contains(". exception (\"GeometryError\""));
        assert!(output.contains(". class :: < Vector2 > ()"));
        assert!(output.contains("BackendClasses"));
        assert!(output.contains("BackendExceptions"));
        assert!(output.contains(". finish () ?"));
    }

    #[test]
    fn renders_an_empty_init_only_module() {
        let output = expand(
            quote!(name = "lifecycle", crate_path = crate),
            parse_quote! {
                impl Lifecycle {
                    #[guestpy(init)]
                    fn setup() -> Result<(), Error> {
                        Ok(())
                    }
                }
            },
        );

        assert!(output.contains("pub fn module < B > ()"));
        assert!(output.contains("BackendValues"));
        assert!(output.contains("BackendCallables"));
        assert!(output.contains(". init ("));
    }

    #[test]
    fn renders_a_synchronous_function_module() {
        let output = expand(
            quote!(name = "mathx", crate_path = crate),
            parse_quote! {
                impl MathX {
                    #[guestpy(function)]
                    fn hypot(x: f64, y: f64) -> Result<f64, Error> {
                        Ok((x * x + y * y).sqrt())
                    }
                }
            },
        );

        assert!(output.contains("pub fn module < B > ()"));
        assert!(output.contains("BackendValues"));
        assert!(output.contains("BackendCallables"));
        assert!(output.contains(". function (\"hypot\""));
    }

    #[test]
    fn renders_an_async_function_module() {
        let output = expand(
            quote!(name = "mathx", crate_path = crate),
            parse_quote! {
                impl MathX {
                    #[guestpy(function)]
                    async fn resolve() -> Result<String, Error> {
                        Ok(String::new())
                    }
                }
            },
        );

        assert!(output.contains("pub fn module < B > ()"));
        assert!(output.contains(". async_function (\"resolve\""));
        assert!(output.contains("async move"));
        assert!(output.contains("BackendModules"));
        assert!(output.contains("BackendCoroutines"));
        assert!(output.contains("BackendExceptions"));
    }

    #[test]
    fn renders_a_class_bearing_module() {
        let output = expand(
            quote!(name = "geometry", classes(Vector2), crate_path = crate),
            parse_quote! {
                impl Geometry {}
            },
        );

        assert!(output.contains("pub fn module < B > ()"));
        assert!(output.contains(". class :: < Vector2 > ()"));
        assert!(output.contains("BackendClasses"));
    }

    #[test]
    fn renders_an_exception_bearing_module() {
        let output = expand(
            quote!(name = "geometry", exceptions(GeometryError), crate_path = crate),
            parse_quote! {
                impl Geometry {}
            },
        );

        assert!(output.contains("pub fn module < B > ()"));
        assert!(output.contains(". exception (\"GeometryError\""));
        assert!(output.contains("BackendExceptions"));
    }

    #[test]
    fn skips_unannotated_items_and_preserves_them() {
        let output = expand(
            quote!(name = "svc", crate_path = crate),
            parse_quote! {
                impl Service {
                    #[guestpy(function)]
                    fn ping(&self) -> Result<i64, Error> {
                        Ok(self.count)
                    }

                    #[allow(dead_code)]
                    fn helper(&self) -> i64 {
                        self.count
                    }
                }
            },
        );

        assert!(output.contains("fn helper"));
        assert!(output.contains("allow (dead_code)"));
    }

    #[test]
    fn rejects_mut_self_and_stateful_async() {
        assert!(
            HostModuleMacro::new(
                quote!(name = "bad", crate_path = crate),
                parse_quote! {
                    impl Bad {
                        #[guestpy(function)]
                        fn tick(&mut self) -> Result<(), Error> {
                            Ok(())
                        }
                    }
                },
            )
            .is_err(),
        );

        assert!(
            HostModuleMacro::new(
                quote!(name = "bad", crate_path = crate),
                parse_quote! {
                    impl Bad {
                        #[guestpy(function)]
                        async fn tick(&self) -> Result<(), Error> {
                            Ok(())
                        }
                    }
                },
            )
            .is_err(),
        );
    }

    #[test]
    fn registers_a_generic_class() {
        let output = expand(
            quote!(name = "host_mail", classes(Envelope<B>), crate_path = crate),
            parse_quote! {
                impl Mail {}
            },
        );

        assert!(output.contains(". class :: < Envelope < B > > ()"));
        assert!(
            output.contains(
                "Envelope < B > : crate :: host :: class :: HostClassDefinition < B >",
            ),
        );
    }

    #[test]
    fn reuses_a_declared_backend_parameter() {
        let output = expand(
            quote!(name = "host_mail", backend = B, crate_path = crate),
            parse_quote! {
                impl<B> Mail<B> {}
            },
        );

        assert!(output.contains("ModuleSpec < B >"));
        assert!(!output.contains("fn module < B >"));
    }

    #[test]
    fn pins_a_module_to_a_concrete_backend() {
        let output = expand(
            quote!(name = "host_mail", backend = RustPython, crate_path = crate),
            parse_quote! {
                impl Mail {}
            },
        );

        assert!(output.contains("ModuleSpec < RustPython >"));
        assert!(!output.contains("where"));
    }
}
