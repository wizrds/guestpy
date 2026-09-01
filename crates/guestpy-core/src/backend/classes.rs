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
        handle::{Class, Instance, ObjectProtocol},
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

        fn construct<'py, B>(enter: &Enter<'py, B>, args: Args<'py, B>) -> Result<Self, Error>
        where
            B: Backend + BackendValues + BackendCallables + BackendClasses,
        {
            let x = args.required::<f64>(enter, 0, "x")?;
            let y = args.required::<f64>(enter, 1, "y")?;

            args.finish()?;

            Ok(Self { x, y })
        }
    }

    impl<B> HostClassDefinition<B> for Vector2
    where
        B: Backend + BackendValues + BackendCallables + BackendClasses,
    {
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
        };
    }

    pub use crate::__guestpy_backend_classes_tests as tests;
}
