use super::{Backend, BackendValues, Tok, Val};

pub trait BackendExceptions: Backend + BackendValues {
    fn traceback<'py>(token: Tok<'py, Self>, exception: &Val<'py, Self>) -> Option<String>;
}

#[doc(hidden)]
pub mod fixtures {
    use std::any::TypeId;

    use crate::{
        backend::{Backend, BackendCallables, BackendModules, BackendValues, guest_fixture},
        bundle::Bundle,
        errors::{Error, GuestException},
        handle::{Function, ObjectProtocol},
        host::{
            exception::{ExceptionClass, FromRaised, HostException, IntoRaise, Raise, Raised},
            module::ModuleSpec,
        },
        marshal::{FromGuest, ToGuest, args::Args},
        runtime::Runtime,
        scope::Enter,
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

    struct TransportError {
        message: String,
    }

    impl HostException for TransportError {
        const NAME: &'static str = "TransportError";
    }

    impl<B> IntoRaise<B> for TransportError
    where
        B: Backend,
        String: ToGuest<B>,
    {
        fn values(self, raise: Raise<B>) -> Raise<B> {
            raise.arg(self.message)
        }
    }

    impl<B> FromRaised<B> for TransportError
    where
        B: Backend + BackendValues,
        String: FromGuest<B, Owned = String>,
    {
        fn from_raised<'py>(raised: &Raised<'py, '_, B>) -> Result<Self, Error> {
            Ok(Self { message: raised.arg::<String>(0)? })
        }
    }

    struct RequestTimeout {
        message: String,
        request_id: String,
    }

    impl HostException for RequestTimeout {
        const NAME: &'static str = "RequestTimeout";

        fn base() -> ExceptionClass {
            TransportError::class()
        }
    }

    impl<B> IntoRaise<B> for RequestTimeout
    where
        B: Backend,
        String: ToGuest<B>,
    {
        fn values(self, raise: Raise<B>) -> Raise<B> {
            raise
                .arg(self.message)
                .attr("request_id", self.request_id)
        }
    }

    impl<B> FromRaised<B> for RequestTimeout
    where
        B: Backend + BackendValues,
        String: FromGuest<B, Owned = String>,
    {
        fn from_raised<'py>(raised: &Raised<'py, '_, B>) -> Result<Self, Error> {
            Ok(Self {
                message: raised.arg::<String>(0)?,
                request_id: raised.attr::<String>("request_id")?,
            })
        }
    }

    struct InvalidInput;

    impl HostException for InvalidInput {
        const NAME: &'static str = "InvalidInput";

        fn base() -> ExceptionClass {
            ExceptionClass::builtin("ValueError")
        }
    }

    impl<B: Backend> IntoRaise<B> for InvalidInput {
        fn values(self, raise: Raise<B>) -> Raise<B> {
            raise
        }
    }

    impl<B: Backend> FromRaised<B> for InvalidInput {
        fn from_raised<'py>(_: &Raised<'py, '_, B>) -> Result<Self, Error> {
            Ok(Self)
        }
    }

    struct UnitError;

    impl HostException for UnitError {
        const NAME: &'static str = "UnitError";
    }

    impl<B: Backend> IntoRaise<B> for UnitError {
        fn values(self, raise: Raise<B>) -> Raise<B> {
            raise
        }
    }

    impl<B: Backend> FromRaised<B> for UnitError {
        fn from_raised<'py>(_: &Raised<'py, '_, B>) -> Result<Self, Error> {
            Ok(Self)
        }
    }

    struct UnregisteredError;

    impl HostException for UnregisteredError {
        const NAME: &'static str = "UnregisteredError";
    }

    impl<B: Backend> IntoRaise<B> for UnregisteredError {
        fn values(self, raise: Raise<B>) -> Raise<B> {
            raise
        }
    }

    struct TypedErrors;

    impl TypedErrors {
        fn base_module<B>() -> ModuleSpec<B>
        where
            B: Backend + BackendValues + BackendCallables + BackendModules,
        {
            ModuleSpec::new("transport_errors").exception_type::<TransportError>()
        }

        fn module<B>(name: impl Into<String>) -> ModuleSpec<B>
        where
            B: Backend + BackendValues + BackendCallables + BackendModules,
            String: FromGuest<B, Owned = String> + ToGuest<B>,
        {
            ModuleSpec::new(name)
                .exception_type::<RequestTimeout>()
                .exception_type::<InvalidInput>()
                .exception_type::<UnitError>()
                .function("timeout", |_, _| {
                    Err::<(), Error>(
                        Raise::<B>::host(RequestTimeout {
                            message: String::from("timed out"),
                            request_id: String::from("request-1"),
                        })
                        .into(),
                    )
                })
                .function("invalid", |_, _| Err::<(), Error>(Raise::<B>::host(InvalidInput).into()))
                .function("unit", |_, _| Err::<(), Error>(Raise::<B>::host(UnitError).into()))
                .function("unregistered", |_, _| {
                    Err::<(), Error>(Raise::<B>::host(UnregisteredError).into())
                })
                .function("fatal", |_, _| Err::<(), Error>(Error::Timeout))
                .function("inspect_timeout", |enter, args| {
                    match Self::catch::<B, RequestTimeout>(enter, &args) {
                        Ok(Some(timeout)) => Ok(timeout.request_id),
                        Ok(None) => Ok(String::from("not matched")),
                        Err(error) => Ok(error.to_string()),
                    }
                })
                .function("inspect_transport", |enter, args| {
                    match Self::catch::<B, TransportError>(enter, &args) {
                        Ok(Some(error)) => Ok(error.message),
                        Ok(None) => Ok(String::from("not matched")),
                        Err(error) => Ok(error.to_string()),
                    }
                })
                .function("inspect_unit", |enter, args| {
                    match Self::catch::<B, UnitError>(enter, &args) {
                        Ok(Some(_)) => Ok(String::from("caught")),
                        Ok(None) => Ok(String::from("not matched")),
                        Err(error) => Ok(error.to_string()),
                    }
                })
                .function("missing_object", |enter, _| {
                    let mut exception = GuestException::new(
                        String::from("RequestTimeout"),
                        String::from("typed_errors.RequestTimeout"),
                        String::from("timed out"),
                        None,
                        vec![String::from("typed_errors.RequestTimeout")],
                        None,
                        None,
                    );

                    exception.identify(vec![TypeId::of::<RequestTimeout>()]);

                    match RequestTimeout::caught(enter, &exception) {
                        Ok(_) => Err(Error::unexpected("expected reconstruction to fail")),
                        Err(error) => Ok(error.to_string()),
                    }
                })
        }

        fn catch<'py, B, E>(enter: &Enter<'py, B>, args: &Args<'py, B>) -> Result<Option<E>, Error>
        where
            B: Backend + BackendValues,
            E: FromRaised<B>,
        {
            let function = args.required::<Function<B>>(enter, 0, "function")?;

            args.finish()?;

            match function.call::<_, ()>(()) {
                Err(Error::Guest(exception)) => E::caught(enter, &exception),
                Err(error) => Err(error),
                Ok(()) => Ok(None),
            }
        }

        fn qualified_name<B>(module: &str) -> String
        where
            B: Backend + BackendValues + BackendCallables + BackendModules,
            String: FromGuest<B, Owned = String> + ToGuest<B>,
        {
            let guest = Runtime::<B>::builder()
                .bind(Self::base_module())
                .bind(Self::module(module))
                .build()
                .unwrap()
                .guest()
                .build()
                .unwrap();

            guest
                .exec(&format!("import {module}"))
                .unwrap();

            Raises::guest(
                guest
                    .exec(&format!("{module}.timeout()"))
                    .unwrap_err(),
            )
            .qualified_name()
            .to_owned()
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

    guest_fixture! {
        pub fn typed_exceptions_match_and_reconstruct_without_public_entry<B>()
        where B: [Backend, BackendValues, BackendCallables, BackendModules]
        using Runtime::<B>::builder()
            .bind(TypedErrors::base_module())
            .bind(TypedErrors::module("typed_errors"))
            .bundle(
                Bundle::single(
                    "same_name",
                    r#"
class RequestTimeout(Exception):
    pass
"#,
                )
                .unwrap(),
            );
        |guest| {
            guest.exec("import same_name").unwrap();
            guest.exec("import typed_errors").unwrap();
            guest
                .exec(
                    r#"
def raise_timeout():
    typed_errors.timeout()

def raise_missing_attribute():
    raise typed_errors.RequestTimeout("timed out")

def raise_value_error():
    raise ValueError()

def raise_unit():
    typed_errors.unit()
"#,
                )
                .unwrap();

            let timeout = Raises::guest(
                guest.exec("typed_errors.timeout()").unwrap_err(),
            );

            assert!(RequestTimeout::class().matches(&timeout));
            assert!(TransportError::class().matches(&timeout));
            assert!(!ExceptionClass::builtin("TimeoutError").matches(&timeout));
            assert_eq!(
                guest
                    .eval::<String>("typed_errors.inspect_timeout(raise_timeout)")
                    .unwrap(),
                "request-1",
            );
            assert_eq!(
                guest
                    .eval::<String>("typed_errors.inspect_transport(raise_timeout)")
                    .unwrap(),
                "timed out",
            );
            assert_eq!(
                guest
                    .eval::<String>("typed_errors.inspect_timeout(raise_value_error)")
                    .unwrap(),
                "not matched",
            );
            assert_eq!(
                guest
                    .eval::<String>("typed_errors.inspect_unit(raise_unit)")
                    .unwrap(),
                "caught",
            );
            assert_eq!(
                guest
                    .eval::<String>(
                        "typed_errors.inspect_timeout(raise_missing_attribute)",
                    )
                    .unwrap(),
                "no attribute named request_id",
            );

            let same_name = Raises::guest(
                guest.exec("raise same_name.RequestTimeout()").unwrap_err(),
            );

            assert!(!RequestTimeout::class().matches(&same_name));
            assert!(ExceptionClass::guest("same_name", "RequestTimeout").matches(&same_name));

            let builtin = Raises::guest(guest.exec("raise TimeoutError()").unwrap_err());

            assert!(ExceptionClass::builtin("TimeoutError").matches(&builtin));
            assert!(!RequestTimeout::class().matches(&builtin));

            let fatal = Raises::guest(
                guest.exec("typed_errors.fatal()").unwrap_err(),
            );

            assert!(ExceptionClass::host("guestpy", "TimeoutError").matches(&fatal));
            assert!(!ExceptionClass::builtin("TimeoutError").matches(&fatal));

            let invalid = Raises::guest(
                guest.exec("typed_errors.invalid()").unwrap_err(),
            );

            assert!(InvalidInput::class().matches(&invalid));
            assert!(ExceptionClass::builtin("ValueError").matches(&invalid));

            let unit = Raises::guest(
                guest.exec("typed_errors.unit()").unwrap_err(),
            );

            assert!(UnitError::class().matches(&unit));

            let unregistered = Raises::guest(
                guest.exec("typed_errors.unregistered()").unwrap_err(),
            );

            assert!(ExceptionClass::builtin("SystemError").matches(&unregistered));
            assert!(!UnregisteredError::class().matches(&unregistered));
            assert_eq!(
                guest
                    .eval::<String>("typed_errors.missing_object()")
                    .unwrap(),
                "conversion error: typed_errors.RequestTimeout has no object for this backend",
            );
        }
    }

    guest_fixture! {
        pub fn the_first_typed_registration_controls_python_identity<B>()
        where B: [Backend, BackendValues, BackendCallables, BackendModules]
        using Runtime::<B>::builder()
            .bind(TypedErrors::base_module())
            .bind(TypedErrors::module("first_errors"))
            .bind(TypedErrors::module("second_errors"));
        |guest| {
            guest.exec("import first_errors").unwrap();
            guest.exec("import second_errors").unwrap();

            let error = Raises::guest(
                guest.exec("second_errors.timeout()").unwrap_err(),
            );

            assert_eq!(error.qualified_name(), "first_errors.RequestTimeout");
            assert!(RequestTimeout::class().matches(&error));
            assert!(
                guest
                    .eval::<bool>(
                        "first_errors.RequestTimeout is second_errors.RequestTimeout",
                    )
                    .unwrap(),
            );
        }
    }

    pub fn typed_registrations_are_runtime_local<B>()
    where
        B: Backend + BackendValues + BackendCallables + BackendModules,
        String: FromGuest<B, Owned = String> + ToGuest<B>,
    {
        assert_eq!(TypedErrors::qualified_name::<B>("runtime_a"), "runtime_a.RequestTimeout",);
        assert_eq!(TypedErrors::qualified_name::<B>("runtime_b"), "runtime_b.RequestTimeout",);
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

            #[test]
            fn typed_exceptions_match_and_reconstruct_without_public_entry() {
                $crate::backend::exceptions::fixtures::typed_exceptions_match_and_reconstruct_without_public_entry::<$backend>();
            }

            #[test]
            fn the_first_typed_registration_controls_python_identity() {
                $crate::backend::exceptions::fixtures::the_first_typed_registration_controls_python_identity::<$backend>();
            }

            #[test]
            fn typed_registrations_are_runtime_local() {
                $crate::backend::exceptions::fixtures::typed_registrations_are_runtime_local::<$backend>();
            }
        };
    }

    pub use crate::__guestpy_backend_exceptions_tests as tests;
}
