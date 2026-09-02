use std::ops::{Deref, DerefMut};

use super::{Backend, BackendCallables, BackendValues, Tok, Val};
use crate::errors::Error;

pub trait BackendClasses: Backend + BackendValues + BackendCallables {
    type Ref<'a, C: 'static>: Deref<Target = C> + 'a
    where
        Self: 'a;

    type RefMut<'a, C: 'static>: DerefMut<Target = C> + 'a
    where
        Self: 'a;

    fn new_class<'py>(
        token: Tok<'py, Self>,
        name: &str,
        bases: &[Val<'py, Self>],
        namespace: &Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error>;

    fn alloc<'py, C: 'static>(
        token: Tok<'py, Self>,
        class: &Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error>;

    fn set_payload<'py, C: 'static>(
        token: Tok<'py, Self>,
        instance: &Val<'py, Self>,
        payload: C,
    ) -> Result<(), Error>;

    fn instantiate<'py, C: 'static>(
        token: Tok<'py, Self>,
        class: &Val<'py, Self>,
        payload: C,
    ) -> Result<Val<'py, Self>, Error> {
        let instance = Self::alloc::<C>(token, class)?;

        Self::set_payload::<C>(token, &instance, payload)?;

        Ok(instance)
    }

    fn borrow<'py, 'a, C: 'static>(
        token: Tok<'py, Self>,
        instance: &'a Val<'py, Self>,
    ) -> Result<Self::Ref<'a, C>, Error>;

    fn borrow_mut<'py, 'a, C: 'static>(
        token: Tok<'py, Self>,
        instance: &'a Val<'py, Self>,
    ) -> Result<Self::RefMut<'a, C>, Error>;

    fn is_host_instance<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> bool;

    fn generic_alias<'py>(
        token: Tok<'py, Self>,
        origin: &Val<'py, Self>,
        arguments: &[Val<'py, Self>],
    ) -> Result<Val<'py, Self>, Error>;
}

#[doc(hidden)]
pub mod fixtures {
    use crate::{
        backend::{
            Backend, BackendCallables, BackendClasses, BackendCoroutines, BackendExceptions,
            BackendInterrupt, BackendModules, BackendValues, guest_fixture,
        },
        errors::Error,
        handle::{
            Annotated, Class, Coroutine, Function, GenericAlias, Instance, Module, Named, Object,
            ObjectProtocol, TypeProtocol,
        },
        host::{
            class::{ClassBuilder, HostClass, HostClassDefinition},
            module::ModuleSpec,
        },
        marshal::args::Args,
        runtime::Runtime,
        scope::Enter,
    };

    struct Vector2 {
        x: f64,
        y: f64,
    }

    impl HostClass for Vector2 {
        const NAME: &'static str = "Vector2";
    }

    impl<B> HostClassDefinition<B> for Vector2
    where
        B: Backend + BackendValues + BackendCallables + BackendClasses,
    {
        fn construct<'py>(enter: &Enter<'py, B>, args: Args<'py, B>) -> Result<Self, Error> {
            let x = args.required::<f64>(enter, 0, "x")?;
            let y = args.required::<f64>(enter, 1, "y")?;

            args.finish()?;

            Ok(Self { x, y })
        }

        fn build(builder: &mut ClassBuilder<B, Self>) {
            builder
                .method("length", |vector, _, _| Ok::<_, Error>(vector.x.hypot(vector.y)))
                .getter("x", |vector, _| Ok::<_, Error>(vector.x))
                .setter("x", |vector, _, value: f64| {
                    vector.x = value;

                    Ok::<_, Error>(())
                });
        }
    }

    struct Contract;

    impl HostClass for Contract {
        const NAME: &'static str = "Contract";
        const DOC: Option<&'static str> = Some("Reports a result for an input.");
    }

    impl<B> HostClassDefinition<B> for Contract
    where
        B: Backend
            + BackendValues
            + BackendCallables
            + BackendClasses
            + BackendModules
            + BackendCoroutines
            + BackendExceptions,
    {
        fn construct<'py>(_: &Enter<'py, B>, args: Args<'py, B>) -> Result<Self, Error> {
            args.finish()?;
            Ok(Self)
        }

        fn build(builder: &mut ClassBuilder<B, Self>) {
            builder.generic();

            builder.async_raw_method("invoke", |this, enter, args| {
                let this = this.clone();
                let city = args.required::<String>(enter, 0, "city")?;

                args.finish()?;

                Ok(async move { this.call_method::<_, String>("execute", (city,)) })
            });
        }
    }

    guest_fixture! {
        pub fn host_borrows_dynamic_and_typed_payloads<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendExceptions,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("geometry").class::<Vector2>());
        |guest| {
            guest.exec("import geometry").unwrap();

            let dynamic = guest
                .eval::<Instance<B>>("geometry.Vector2(3, 4)")
                .unwrap();

            assert_eq!(
                dynamic
                    .borrow_as_with::<Vector2, _, _>(|vector| vector.x)
                    .unwrap(),
                3.0,
            );

            let typed = dynamic.as_typed::<Vector2>().unwrap();

            typed
                .borrow_with_mut(|vector| {
                    vector.x = 1.0;
                })
                .unwrap();

            assert_eq!(typed.borrow_with(|vector| vector.x).unwrap(), 1.0);
        }
    }

    guest_fixture! {
        pub fn ordinary_guest_class_uses_instance_handle<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendExceptions,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("geometry").class::<Vector2>());
        |guest| {
            guest.exec("import geometry").unwrap();
            guest
                .exec(
                    r#"
class GuestVector:
    def __init__(self, x):
        self.x = x

value = GuestVector(3)
"#,
                )
                .unwrap();

            assert_eq!(
                guest
                    .globals()
                    .unwrap()
                    .item::<Instance<_>, _>("value")
                    .unwrap()
                    .get::<i64>("x")
                    .unwrap(),
                3,
            );
        }
    }

    guest_fixture! {
        pub fn class_handle_rejects_other_callables<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendExceptions,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("geometry").class::<Vector2>());
        |guest| {
            guest.exec("import geometry").unwrap();
            guest
                .exec(
                    r#"
def callable_value():
    pass
"#,
                )
                .unwrap();

            assert!(guest.eval::<Class<_>>("geometry.Vector2").is_ok());
            assert!(guest.eval::<Class<_>>("lambda: None").is_err());
            assert!(
                guest
                    .globals()
                    .unwrap()
                    .class("callable_value")
                    .is_err(),
            );
        }
    }

    guest_fixture! {
        pub fn class_constructs_and_overrides_its_result_descriptor<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendExceptions,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("geometry").class::<Vector2>());
        |guest| {
            guest.exec("import geometry").unwrap();

            let class = guest
                .eval::<Class<_, Vector2>>("geometry.Vector2")
                .unwrap();
            let typed = class.construct((3.0_f64, 4.0_f64)).unwrap();
            let dynamic = class
                .construct_as::<_, Instance<_>>((6.0_f64, 8.0_f64))
                .unwrap();
            let retyped = class.with_result::<Instance<_>>();

            assert_eq!(
                typed
                    .borrow_with(|vector| vector.x.hypot(vector.y))
                    .unwrap(),
                5.0,
            );
            assert_eq!(dynamic.get::<f64>("x").unwrap(), 6.0);
            assert_eq!(
                retyped
                    .construct((5.0_f64, 12.0_f64))
                    .unwrap()
                    .borrow_as_with::<Vector2, _, _>(|vector| vector.y)
                    .unwrap(),
                12.0,
            );
            assert!(class.value().ptr_eq(&retyped.value()));
        }
    }

    guest_fixture! {
        pub fn guest_subclass_works<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendExceptions,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("geometry").class::<Vector2>());
        |guest| {
            guest.exec("import geometry").unwrap();
            guest
                .exec(
                    r#"
class Tagged(geometry.Vector2):
    def __init__(self, x, y):
        super().__init__(x, y)
        self.tag = 'tagged'

t = Tagged(3, 4)
"#,
                )
                .unwrap();

            assert_eq!(
                guest
                    .eval::<Vector2>("t")
                    .unwrap()
                    .borrow_with(|vector| vector.x.hypot(vector.y))
                    .unwrap(),
                5.0,
            );
        }
    }

    guest_fixture! {
        pub fn subclass_that_skips_super_init_fails_clearly<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendExceptions,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("geometry").class::<Vector2>());
        |guest| {
            guest.exec("import geometry").unwrap();
            guest
                .exec(
                    r#"
class Empty(geometry.Vector2):
    def __init__(self, x, y):
        pass

e = Empty(3, 4)
"#,
                )
                .unwrap();

            assert!(
                guest
                    .eval::<Vector2>("e")
                    .err()
                    .unwrap()
                    .to_string()
                    .contains("no payload"),
            );
        }
    }

    guest_fixture! {
        pub fn class_handle_reads_class_attributes<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendExceptions,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder();
        |guest| {
            guest
                .exec(
                    r#"
class Impl:
    description = 'a description'
"#,
                )
                .unwrap();

            let class = guest.eval::<Class<B>>("Impl").unwrap();

            assert_eq!(class.get::<String>("description").unwrap(), "a description");
            assert!(class.has("description").unwrap());
            assert!(!class.has("missing").unwrap());
            assert!(class.dir().unwrap().contains(&String::from("description")));
            assert_eq!(class.name().unwrap(), "Impl");
        }
    }

    guest_fixture! {
        pub fn module_and_function_expose_names_and_annotations<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendExceptions,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder();
        |guest| {
            guest
                .exec(
                    r#"
import types

m = types.ModuleType('probe')
m.__doc__ = 'a probe'

def scale(value: int, factor: float) -> float:
    return value * factor
"#,
                )
                .unwrap();

            let module = guest.eval::<Module<B>>("m").unwrap();
            let function = guest.eval::<Function<B>>("scale").unwrap();

            assert_eq!(module.name().unwrap(), "probe");
            assert_eq!(module.doc().unwrap(), Some(String::from("a probe")));
            assert_eq!(function.name().unwrap(), "scale");
            assert!(function.annotation("factor").unwrap().is_some());
            assert!(function.annotation("missing").unwrap().is_none());
            assert_eq!(
                function
                    .annotations()
                    .unwrap()
                    .iter()
                    .map(|(name, _)| name.clone())
                    .collect::<Vec<_>>(),
                vec![
                    String::from("value"),
                    String::from("factor"),
                    String::from("return"),
                ],
            );
        }
    }

    guest_fixture! {
        pub fn class_reports_bases_and_mro<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendExceptions,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder();
        |guest| {
            guest
                .exec(
                    r#"
class Base:
    pass

class Derived(Base):
    pass
"#,
                )
                .unwrap();

            let derived = guest.eval::<Class<B>>("Derived").unwrap();

            assert_eq!(
                derived
                    .bases()
                    .unwrap()
                    .iter()
                    .map(Named::name)
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap(),
                vec![String::from("Base")],
            );
            assert_eq!(
                derived
                    .mro()
                    .unwrap()
                    .iter()
                    .map(Named::name)
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap(),
                vec![
                    String::from("Derived"),
                    String::from("Base"),
                    String::from("object"),
                ],
            );
        }
    }

    guest_fixture! {
        pub fn isinstance_and_issubclass_agree_across_backends<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendExceptions,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("host_lib").class::<Contract>());
        |guest| {
            guest.exec("import host_lib").unwrap();
            guest
                .exec(
                    r#"
class Impl(host_lib.Contract):
    pass

class Plain:
    pass

i = Impl()
p = Plain()
"#,
                )
                .unwrap();

            let contract = guest.eval::<Class<B>>("host_lib.Contract").unwrap();
            let implementation = guest.eval::<Class<B>>("Impl").unwrap();
            let plain = guest.eval::<Class<B>>("Plain").unwrap();
            let instance = guest.eval::<Object<B>>("i").unwrap();
            let other = guest.eval::<Object<B>>("p").unwrap();

            assert!(instance.is_instance_of(&contract).unwrap());
            assert!(instance.is_instance_of(&implementation).unwrap());
            assert!(!other.is_instance_of(&contract).unwrap());
            assert!(implementation.is_subclass_of(&contract).unwrap());
            assert!(!plain.is_subclass_of(&contract).unwrap());
            assert!(contract.is_subclass_of(&contract).unwrap());
        }
    }

    guest_fixture! {
        pub fn host_class_is_generic<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendExceptions,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("host_lib").class::<Contract>());
        |guest| {
            guest.exec("import host_lib").unwrap();
            guest
                .exec(
                    r#"
class Args:
    pass

class Result:
    pass

alias = host_lib.Contract[Args, Result]
"#,
                )
                .unwrap();

            let alias = GenericAlias::of(&guest.eval::<Object<B>>("alias").unwrap())
                .unwrap()
                .unwrap();

            assert_eq!(alias.origin().unwrap().name().unwrap(), "Contract");
            assert_eq!(
                alias
                    .arguments()
                    .unwrap()
                    .iter()
                    .map(|argument| {
                        argument
                            .cast::<Class<B>>()
                            .and_then(|class| class.name())
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap(),
                vec![String::from("Args"), String::from("Result")],
            );

            guest.exec("single = host_lib.Contract[Args]").unwrap();

            assert_eq!(
                GenericAlias::of(&guest.eval::<Object<B>>("single").unwrap())
                    .unwrap()
                    .unwrap()
                    .arguments()
                    .unwrap()
                    .len(),
                1,
            );

            guest
                .exec(
                    r#"
class Impl(host_lib.Contract[Args, Result]):
    pass
"#,
                )
                .unwrap();

            let implementation = guest.eval::<Class<B>>("Impl").unwrap();

            assert!(
                implementation
                    .is_subclass_of(&guest.eval::<Class<B>>("host_lib.Contract").unwrap())
                    .unwrap(),
            );
            assert_eq!(
                implementation
                    .generic_base_of(&guest.eval::<Class<B>>("host_lib.Contract").unwrap())
                    .unwrap()
                    .unwrap()
                    .arguments()
                    .unwrap()
                    .len(),
                2,
            );
        }
    }

    guest_fixture! {
        pub async fn host_base_method_calls_guest_override<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendExceptions,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("host_lib").class::<Contract>());
        |guest| {
            guest.exec("import host_lib").unwrap();
            guest
                .exec(
                    r#"
class Impl(host_lib.Contract):
    def execute(self, city):
        return 'sunny in ' + city
"#,
                )
                .unwrap();

            assert_eq!(
                guest
                    .eval::<Instance<B>>("Impl()")
                    .unwrap()
                    .call_method::<_, Coroutine<B, String>>("invoke", ("Vancouver",))
                    .unwrap()
                    .await
                    .unwrap(),
                "sunny in Vancouver",
            );
        }
    }

    guest_fixture! {
        pub fn annotations_preserve_declaration_order<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendExceptions,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder();
        |guest| {
            guest
                .exec(
                    r#"
class Row:
    city: str
    units: str
    temperature: float
"#,
                )
                .unwrap();

            assert_eq!(
                guest
                    .eval::<Class<B>>("Row")
                    .unwrap()
                    .annotations()
                    .unwrap()
                    .iter()
                    .map(|(name, _)| name.clone())
                    .collect::<Vec<_>>(),
                vec![
                    String::from("city"),
                    String::from("units"),
                    String::from("temperature"),
                ],
            );
        }
    }

    guest_fixture! {
        pub fn any_handle_calls_a_callable_and_reports_a_clear_error_otherwise<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendExceptions,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder()
            .bind(ModuleSpec::new("host_lib").class::<Contract>());
        |guest| {
            guest
                .exec(
                    r#"
def twice(value):
    return value * 2
"#,
                )
                .unwrap();

            assert_eq!(
                guest
                    .eval::<Object<B>>("twice")
                    .unwrap()
                    .call::<_, i64>((21,))
                    .unwrap(),
                42,
            );

            let module = guest.import("host_lib").unwrap();
            let message = module
                .call::<_, i64>(())
                .err()
                .unwrap()
                .to_string();

            assert!(message.contains("callable") || message.contains("not callable"));
        }
    }

    #[doc(hidden)]
    #[macro_export]
    macro_rules! __guestpy_backend_classes_tests {
        ($backend:ty) => {
            #[test]
            fn host_borrows_dynamic_and_typed_payloads() {
                $crate::backend::classes::fixtures::host_borrows_dynamic_and_typed_payloads::<
                    $backend,
                >();
            }

            #[test]
            fn ordinary_guest_class_uses_instance_handle() {
                $crate::backend::classes::fixtures::ordinary_guest_class_uses_instance_handle::<
                    $backend,
                >();
            }

            #[test]
            fn class_handle_rejects_other_callables() {
                $crate::backend::classes::fixtures::class_handle_rejects_other_callables::<
                    $backend,
                >();
            }

            #[test]
            fn class_constructs_and_overrides_its_result_descriptor() {
                $crate::backend::classes::fixtures::class_constructs_and_overrides_its_result_descriptor::<
                    $backend,
                >();
            }

            #[test]
            fn guest_subclass_works() {
                $crate::backend::classes::fixtures::guest_subclass_works::<$backend>();
            }

            #[test]
            fn subclass_that_skips_super_init_fails_clearly() {
                $crate::backend::classes::fixtures::subclass_that_skips_super_init_fails_clearly::<
                    $backend,
                >();
            }

            #[test]
            fn class_handle_reads_class_attributes() {
                $crate::backend::classes::fixtures::class_handle_reads_class_attributes::<
                    $backend,
                >();
            }

            #[test]
            fn module_and_function_expose_names_and_annotations() {
                $crate::backend::classes::fixtures::module_and_function_expose_names_and_annotations::<
                    $backend,
                >();
            }

            #[test]
            fn class_reports_bases_and_mro() {
                $crate::backend::classes::fixtures::class_reports_bases_and_mro::<$backend>();
            }

            #[test]
            fn isinstance_and_issubclass_agree_across_backends() {
                $crate::backend::classes::fixtures::isinstance_and_issubclass_agree_across_backends::<
                    $backend,
                >();
            }

            #[test]
            fn host_class_is_generic() {
                $crate::backend::classes::fixtures::host_class_is_generic::<$backend>();
            }

            #[tokio::test]
            async fn host_base_method_calls_guest_override() {
                $crate::backend::classes::fixtures::host_base_method_calls_guest_override::<
                    $backend,
                >()
                .await;
            }

            #[test]
            fn annotations_preserve_declaration_order() {
                $crate::backend::classes::fixtures::annotations_preserve_declaration_order::<
                    $backend,
                >();
            }

            #[test]
            fn any_handle_calls_a_callable_and_reports_a_clear_error_otherwise() {
                $crate::backend::classes::fixtures::any_handle_calls_a_callable_and_reports_a_clear_error_otherwise::<
                    $backend,
                >();
            }
        };
    }

    pub use crate::__guestpy_backend_classes_tests as tests;
}
