use super::{Backend, BackendValues, Tok, Val};

pub trait BackendExceptions: Backend + BackendValues {
    fn traceback<'py>(token: Tok<'py, Self>, exception: &Val<'py, Self>) -> Option<String>;
}

#[doc(hidden)]
pub mod fixtures {
    use crate::{
        backend::{guest_fixture, Backend, BackendCallables, BackendModules, BackendValues},
        bundle::Bundle,
        errors::{Error, GuestException},
        host::{
            exception::{ExceptionClass, Raise},
            module::ModuleSpec,
        },
        runtime::Runtime,
    };

    struct Raises;

    impl Raises {
        fn guest(error: Error) -> GuestException {
            match error {
                Error::Guest(exception) => *exception,
                other => panic!("expected a guest exception, got: {other}"),
            }
        }
    }

    struct BuiltinErrors;

    impl BuiltinErrors {
        fn module<B>() -> ModuleSpec<B>
        where
            B: Backend + BackendValues + BackendCallables,
        {
            ModuleSpec::new("builtin_errors").function("trigger", |enter, _| {
                Err::<i64, Error>(
                    Raise::within(enter, ExceptionClass::builtin("ValueError"))
                        .arg(String::from("bad value"))
                        .attr("code", 7_i64)
                        .into(),
                )
            })
        }
    }

    struct DeclaredErrors;

    impl DeclaredErrors {
        fn module<B>() -> ModuleSpec<B>
        where
            B: Backend + BackendValues + BackendCallables + BackendModules,
        {
            ModuleSpec::new("declared_errors")
                .exception("Boom", ExceptionClass::builtin("Exception"))
                .function("trigger", |enter, _| {
                    Err::<i64, Error>(
                        Raise::within(enter, ExceptionClass::host("declared_errors", "Boom"))
                            .into(),
                    )
                })
        }
    }

    struct CrossModuleErrors;

    impl CrossModuleErrors {
        fn base_module<B>() -> ModuleSpec<B>
        where
            B: Backend + BackendValues + BackendCallables + BackendModules,
        {
            ModuleSpec::new("errors_base").exception("Base", ExceptionClass::builtin("Exception"))
        }

        fn derived_module<B>() -> ModuleSpec<B>
        where
            B: Backend + BackendValues + BackendCallables + BackendModules,
        {
            ModuleSpec::new("errors_derived")
                .exception("Derived", ExceptionClass::host("errors_base", "Base"))
                .function("trigger", |enter, _| {
                    Err::<i64, Error>(
                        Raise::within(enter, ExceptionClass::host("errors_derived", "Derived"))
                            .into(),
                    )
                })
        }
    }

    struct NestedErrors;

    impl NestedErrors {
        fn module<B>() -> ModuleSpec<B>
        where
            B: Backend + BackendValues + BackendCallables,
        {
            ModuleSpec::new("nested_errors").function("trigger", |enter, _| {
                Err::<i64, Error>(
                    Raise::within(enter, ExceptionClass::guest("errors_mod", "Outer.Inner")).into(),
                )
            })
        }
    }

    struct CausedErrors;

    impl CausedErrors {
        fn module<B>() -> ModuleSpec<B>
        where
            B: Backend + BackendValues + BackendCallables,
        {
            ModuleSpec::new("caused_errors").function("trigger", |enter, _| {
                Err::<i64, Error>(
                    Raise::within(enter, ExceptionClass::builtin("RuntimeError"))
                        .arg(String::from("wrapped"))
                        .cause(Error::conversion("root cause"))
                        .into(),
                )
            })
        }
    }

    struct UnresolvedErrors;

    impl UnresolvedErrors {
        fn module<B>() -> ModuleSpec<B>
        where
            B: Backend + BackendValues + BackendCallables,
        {
            ModuleSpec::new("unresolved_errors").function("trigger", |enter, _| {
                Err::<i64, Error>(
                    Raise::within(enter, ExceptionClass::host("nowhere", "Missing")).into(),
                )
            })
        }
    }

    struct WrongKindErrors;

    impl WrongKindErrors {
        fn module<B>() -> ModuleSpec<B>
        where
            B: Backend + BackendValues + BackendCallables,
        {
            ModuleSpec::new("wrong_kind_errors").function("trigger", |enter, _| {
                Err::<i64, Error>(
                    Raise::within(enter, ExceptionClass::guest("builtins", "int")).into(),
                )
            })
        }
    }

    guest_fixture! {
        pub fn raises_a_builtin_exception_with_arguments_and_attributes<B>()
        where B: [Backend, BackendValues, BackendCallables, BackendModules]
        using Runtime::<B>::builder().bind(BuiltinErrors::module());
        |guest| {
            guest.exec("import builtin_errors").unwrap();

            let error = Raises::guest(guest.exec("builtin_errors.trigger()").unwrap_err());

            assert!(error.matches("ValueError"));
            assert_eq!(error.message(), "bad value");
        }
    }

    guest_fixture! {
        pub fn raises_a_declared_host_exception_class_with_stable_identity<B>()
        where B: [Backend, BackendValues, BackendCallables, BackendModules]
        using Runtime::<B>::builder().bind(DeclaredErrors::module());
        |guest| {
            guest.exec("import declared_errors").unwrap();
            guest.exec(r#"
first = None
second = None
try:
    declared_errors.trigger()
except declared_errors.Boom as e:
    first = type(e)
try:
    declared_errors.trigger()
except declared_errors.Boom as e:
    second = type(e)
"#).unwrap();

            assert!(guest.eval::<bool>("first is second is declared_errors.Boom").unwrap());
        }
    }

    guest_fixture! {
        pub fn a_host_exception_can_have_a_base_from_another_host_module<B>()
        where B: [Backend, BackendValues, BackendCallables, BackendModules]
        using Runtime::<B>::builder()
            .bind(CrossModuleErrors::base_module())
            .bind(CrossModuleErrors::derived_module());
        |guest| {
            guest.exec("import errors_base").unwrap();
            guest.exec("import errors_derived").unwrap();

            assert!(guest.eval::<bool>("issubclass(errors_derived.Derived, errors_base.Base)").unwrap());

            let caught = Raises::guest(guest.exec("errors_derived.trigger()").unwrap_err());

            assert!(caught.matches("errors_base.Base"));
        }
    }

    guest_fixture! {
        pub fn raises_a_nested_guest_exception_class<B>()
        where B: [Backend, BackendValues, BackendCallables, BackendModules]
        using Runtime::<B>::builder()
            .bind(NestedErrors::module())
            .bundle(Bundle::single("errors_mod", r#"
class Outer:
    class Inner(Exception):
        pass
"#).unwrap());
        |guest| {
            guest.exec("import errors_mod").unwrap();
            guest.exec("import nested_errors").unwrap();
            guest.exec(r#"
caught = False
try:
    nested_errors.trigger()
except errors_mod.Outer.Inner:
    caught = True
"#).unwrap();

            assert!(guest.eval::<bool>("caught").unwrap());
        }
    }

    guest_fixture! {
        pub fn raises_with_an_explicit_cause_and_suppressed_context<B>()
        where B: [Backend, BackendValues, BackendCallables, BackendModules]
        using Runtime::<B>::builder().bind(CausedErrors::module());
        |guest| {
            guest.exec("import caused_errors").unwrap();
            guest.exec(r#"
caught = None
try:
    caused_errors.trigger()
except RuntimeError as e:
    caught = e
"#).unwrap();

            assert!(guest.eval::<bool>("isinstance(caught.__cause__, TypeError)").unwrap());
            assert!(guest.eval::<bool>("caught.__suppress_context__ is True").unwrap());
        }
    }

    guest_fixture! {
        pub fn an_unregistered_host_exception_class_falls_back_to_system_error<B>()
        where B: [Backend, BackendValues, BackendCallables, BackendModules]
        using Runtime::<B>::builder().bind(UnresolvedErrors::module());
        |guest| {
            guest.exec("import unresolved_errors").unwrap();

            let error = Raises::guest(guest.exec("unresolved_errors.trigger()").unwrap_err());

            assert!(error.matches("SystemError"));
        }
    }

    guest_fixture! {
        pub fn raising_a_non_exception_class_falls_back_to_type_error<B>()
        where B: [Backend, BackendValues, BackendCallables, BackendModules]
        using Runtime::<B>::builder().bind(WrongKindErrors::module());
        |guest| {
            guest.exec("import wrong_kind_errors").unwrap();

            let error = Raises::guest(guest.exec("wrong_kind_errors.trigger()").unwrap_err());

            assert!(error.matches("TypeError"));
        }
    }

    #[doc(hidden)]
    #[macro_export]
    macro_rules! __guestpy_backend_exceptions_tests {
        ($backend:ty) => {
            #[test]
            fn raises_a_builtin_exception_with_arguments_and_attributes() {
                $crate::backend::exceptions::fixtures::raises_a_builtin_exception_with_arguments_and_attributes::<$backend>();
            }

            #[test]
            fn raises_a_declared_host_exception_class_with_stable_identity() {
                $crate::backend::exceptions::fixtures::raises_a_declared_host_exception_class_with_stable_identity::<$backend>();
            }

            #[test]
            fn a_host_exception_can_have_a_base_from_another_host_module() {
                $crate::backend::exceptions::fixtures::a_host_exception_can_have_a_base_from_another_host_module::<$backend>();
            }

            #[test]
            fn raises_a_nested_guest_exception_class() {
                $crate::backend::exceptions::fixtures::raises_a_nested_guest_exception_class::<$backend>();
            }

            #[test]
            fn raises_with_an_explicit_cause_and_suppressed_context() {
                $crate::backend::exceptions::fixtures::raises_with_an_explicit_cause_and_suppressed_context::<$backend>();
            }

            #[test]
            fn an_unregistered_host_exception_class_falls_back_to_system_error() {
                $crate::backend::exceptions::fixtures::an_unregistered_host_exception_class_falls_back_to_system_error::<$backend>();
            }

            #[test]
            fn raising_a_non_exception_class_falls_back_to_type_error() {
                $crate::backend::exceptions::fixtures::raising_a_non_exception_class_falls_back_to_type_error::<$backend>();
            }
        };
    }

    pub use crate::__guestpy_backend_exceptions_tests as tests;
}
