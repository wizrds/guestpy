use std::{future::Future, marker::PhantomData, pin::Pin, rc::Rc};

use super::{Backend, BackendValues, Tok, Val};
use crate::{
    errors::Error,
    marshal::{ToGuest, args::Args},
    scope::Enter,
};

pub struct RawCall<'py, B: Backend> {
    pub token: B::Token<'py>,
    pub positional: Vec<B::Value<'py>>,
    pub keyword: Vec<(String, B::Value<'py>)>,
}

pub type HostBody<B> =
    Rc<dyn for<'py> Fn(&Enter<'py, B>, Args<'py, B>) -> Result<<B as Backend>::Value<'py>, Error>>;

pub type RawBody<B> =
    Rc<dyn for<'py> Fn(RawCall<'py, B>) -> Result<<B as Backend>::Value<'py>, Error>>;

pub(crate) trait PendingResult<B: Backend> {
    fn complete<'py>(self: Box<Self>, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error>;
}

pub(crate) type HostFuture<B> =
    Pin<Box<dyn Future<Output = Result<Box<dyn PendingResult<B>>, Error>>>>;

pub(crate) type HostAsyncBody<B> =
    Rc<dyn for<'py> Fn(&Enter<'py, B>, Args<'py, B>) -> Result<HostFuture<B>, Error>>;

pub(crate) struct PendingValue<B: Backend, T> {
    value: T,
    backend: PhantomData<B>,
}

impl<B, T> PendingValue<B, T>
where
    B: Backend,
    T: ToGuest<B> + 'static,
{
    pub(crate) fn into_host_future<Fut>(future: Fut) -> HostFuture<B>
    where
        Fut: Future<Output = Result<T, Error>> + 'static,
    {
        Box::pin(async move {
            Ok(Box::new(Self {
                value: future.await?,
                backend: PhantomData,
            }) as Box<dyn PendingResult<B>>)
        })
    }
}

impl<B, T> PendingResult<B> for PendingValue<B, T>
where
    B: Backend,
    T: ToGuest<B>,
{
    fn complete<'py>(self: Box<Self>, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        self.value.to_guest(enter)
    }
}

pub trait BackendCallables: Backend + BackendValues {
    fn function<'py>(
        token: Tok<'py, Self>,
        name: &str,
        doc: Option<&str>,
        body: RawBody<Self>,
    ) -> Result<Val<'py, Self>, Error>;

    fn method<'py>(
        token: Tok<'py, Self>,
        name: &str,
        doc: Option<&str>,
        body: RawBody<Self>,
    ) -> Result<Val<'py, Self>, Error>;
}

#[doc(hidden)]
pub mod fixtures {
    use std::{
        cell::RefCell,
        collections::HashMap,
        rc::Rc,
    };

    use crate::{
        backend::{
            Backend, BackendCallables, BackendClasses, BackendCoroutines, BackendExceptions,
            BackendInterrupt, BackendModules, BackendValues, guest_fixture,
        },
        errors::{Error, GuestException},
        host::module::ModuleSpec,
        marshal::primitives::Bytes,
        runtime::Runtime,
    };

    struct Codec;

    impl Codec {
        fn module<B>() -> ModuleSpec<B>
        where
            B: Backend
                + BackendValues
                + BackendCallables
                + BackendClasses
                + BackendModules
                + BackendCoroutines
                + BackendExceptions
                + BackendInterrupt,
        {
            ModuleSpec::new("codec")
                .function("echo_i64", |enter, args| {
                    Ok::<_, Error>(args.required::<i64>(enter, 0, "value")?)
                })
                .function("echo_u8", |enter, args| {
                    Ok::<_, Error>(args.required::<u8>(enter, 0, "value")?)
                })
                .function("echo_f64", |enter, args| {
                    Ok::<_, Error>(args.required::<f64>(enter, 0, "value")?)
                })
                .function("echo_str", |enter, args| {
                    Ok::<_, Error>(args.required::<String>(enter, 0, "value")?)
                })
                .function("echo_bytes", |enter, args| {
                    Ok::<_, Error>(args.required::<Bytes>(enter, 0, "value")?)
                })
                .function("echo_list", |enter, args| {
                    Ok::<_, Error>(args.required::<Vec<i64>>(enter, 0, "value")?)
                })
                .function("echo_pair", |enter, args| {
                    Ok::<_, Error>(
                        args.required::<(i64, String)>(enter, 0, "value")?,
                    )
                })
                .function("echo_map", |enter, args| {
                    Ok::<_, Error>(
                        args.required::<HashMap<String, i64>>(enter, 0, "value")?,
                    )
                })
                .function("echo_opt", |enter, args| {
                    Ok::<_, Error>(args.required::<Option<i64>>(enter, 0, "value")?)
                })
                .function("add", |enter, args| {
                    Ok::<_, Error>(
                        args.required::<i64>(enter, 0, "left")?
                            + args.required::<i64>(enter, 1, "right")?,
                    )
                })
                .function("boom", |_, _| {
                    Err::<i64, _>(Error::conversion("deliberate failure"))
                })
                .async_function("later", |_, _| {
                    Ok::<_, Error>(async { Ok::<_, Error>(1_i64) })
                })
        }
    }

    struct Raises;

    impl Raises {
        fn guest(error: Error) -> GuestException {
            match error {
                Error::Guest(exception) => exception,
                other => panic!("expected a guest exception, got: {other}"),
            }
        }
    }

    guest_fixture! {
        pub fn calls_a_host_function_both_ways<B>()
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
        using Runtime::<B>::builder().bind(Codec::module());
        |guest| {
            guest.exec("import codec").unwrap();

            assert_eq!(guest.eval::<i64>("codec.add(2, 3)").unwrap(), 5);
            assert_eq!(
                guest
                    .eval::<i64>("codec.add(2, right=3)")
                    .unwrap(),
                5,
            );
            assert_eq!(
                guest
                    .eval::<i64>("codec.add(left=2, right=3)")
                    .unwrap(),
                5,
            );

            let error = Raises::guest(
                guest
                    .eval::<i64>("codec.add(2)")
                    .unwrap_err(),
            );

            assert!(error.matches("TypeError"));
            assert!(error.message().contains("right"));
        }
    }

    guest_fixture! {
        pub fn round_trips_integers<B>()
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
        using Runtime::<B>::builder().bind(Codec::module());
        |guest| {
            guest.exec("import codec").unwrap();

            assert_eq!(
                guest
                    .eval::<i64>("codec.echo_i64(-9007199254740993)")
                    .unwrap(),
                -9007199254740993,
            );
            assert_eq!(guest.eval::<i64>("codec.echo_u8(255)").unwrap(), 255);
            assert!(
                Raises::guest(
                    guest
                        .eval::<i64>("codec.echo_u8(300)")
                        .unwrap_err(),
                )
                .matches("TypeError"),
            );
            assert!(
                Raises::guest(
                    guest
                        .eval::<i64>("codec.echo_u8(-1)")
                        .unwrap_err(),
                )
                .matches("TypeError"),
            );
            assert!(
                Raises::guest(
                    guest
                        .eval::<i64>("codec.echo_i64('x')")
                        .unwrap_err(),
                )
                .matches("TypeError"),
            );
        }
    }

    guest_fixture! {
        pub fn round_trips_floats_and_strings<B>()
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
        using Runtime::<B>::builder().bind(Codec::module());
        |guest| {
            guest.exec("import codec").unwrap();

            assert!(guest.eval::<bool>("codec.echo_f64(1.5) == 1.5").unwrap());
            assert!(
                guest
                    .eval::<bool>("codec.echo_str('héllo') == 'héllo'")
                    .unwrap(),
            );
        }
    }

    guest_fixture! {
        pub fn round_trips_bytes<B>()
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
        using Runtime::<B>::builder().bind(Codec::module());
        |guest| {
            guest.exec("import codec").unwrap();

            assert!(
                guest
                    .eval::<bool>("codec.echo_bytes(b'\\x00\\xff') == b'\\x00\\xff'")
                    .unwrap(),
            );
            assert!(
                Raises::guest(
                    guest
                        .eval::<bool>("codec.echo_bytes('text')")
                        .unwrap_err(),
                )
                .matches("TypeError"),
            );
        }
    }

    guest_fixture! {
        pub fn round_trips_sequences<B>()
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
        using Runtime::<B>::builder().bind(Codec::module());
        |guest| {
            guest.exec("import codec").unwrap();

            assert!(
                guest
                    .eval::<bool>("codec.echo_list([1, 2, 3]) == [1, 2, 3]")
                    .unwrap(),
            );
            assert!(
                guest
                    .eval::<bool>("codec.echo_pair((1, 'a')) == (1, 'a')")
                    .unwrap(),
            );
            assert!(
                Raises::guest(
                    guest
                        .eval::<bool>("codec.echo_list((1, 2))")
                        .unwrap_err(),
                )
                .matches("TypeError"),
            );
        }
    }

    guest_fixture! {
        pub fn round_trips_mappings_and_optionals<B>()
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
        using Runtime::<B>::builder().bind(Codec::module());
        |guest| {
            guest.exec("import codec").unwrap();

            assert!(
                guest
                    .eval::<bool>("codec.echo_map({'a': 1}) == {'a': 1}")
                    .unwrap(),
            );
            assert!(
                guest
                    .eval::<bool>("codec.echo_opt(None) is None")
                    .unwrap(),
            );
            assert!(guest.eval::<bool>("codec.echo_opt(4) == 4").unwrap());
        }
    }

    guest_fixture! {
        pub fn an_error_from_a_host_body_is_catchable<B>()
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
        using Runtime::<B>::builder().bind(Codec::module());
        |guest| {
            guest
                .exec(
                    r#"
import codec
caught = False
try:
    codec.boom()
except TypeError as e:
    caught = str(e)
"#,
                )
                .unwrap();

            assert!(
                guest
                    .eval::<String>("caught")
                    .unwrap()
                    .contains("deliberate failure"),
            );
            assert!(
                Raises::guest(
                    guest
                        .exec("raise ValueError('nope')")
                        .unwrap_err(),
                )
                .matches("ValueError"),
            );
        }
    }

    pub fn init_hooks_run_once_per_guest_in_bind_order<B>()
    where
        B: Backend
            + BackendValues
            + BackendCallables
            + BackendClasses
            + BackendModules
            + BackendCoroutines
            + BackendExceptions
            + BackendInterrupt,
    {
        let log = Rc::new(RefCell::new(Vec::new()));
        let runtime = Runtime::<B>::builder()
            .bind(ModuleSpec::new("first").init({
                let log = log.clone();

                move |_| {
                    log.borrow_mut().push("first");

                    Ok::<_, Error>(())
                }
            }))
            .bind(ModuleSpec::new("second").init({
                let log = log.clone();

                move |_| {
                    log.borrow_mut().push("second");

                    Ok::<_, Error>(())
                }
            }))
            .build()
            .unwrap();

        let first = runtime.guest().build().unwrap();
        let second = runtime.guest().build().unwrap();

        assert_eq!(*log.borrow(), vec!["first", "second", "first", "second"]);

        drop((first, second));
    }

    #[doc(hidden)]
    #[macro_export]
    macro_rules! __guestpy_backend_callables_tests {
        ($backend:ty) => {
            #[test]
            fn calls_a_host_function_both_ways() {
                $crate::backend::callables::fixtures::calls_a_host_function_both_ways::<
                    $backend,
                >();
            }

            #[test]
            fn round_trips_integers() {
                $crate::backend::callables::fixtures::round_trips_integers::<$backend>();
            }

            #[test]
            fn round_trips_floats_and_strings() {
                $crate::backend::callables::fixtures::round_trips_floats_and_strings::<
                    $backend,
                >();
            }

            #[test]
            fn round_trips_bytes() {
                $crate::backend::callables::fixtures::round_trips_bytes::<$backend>();
            }

            #[test]
            fn round_trips_sequences() {
                $crate::backend::callables::fixtures::round_trips_sequences::<$backend>();
            }

            #[test]
            fn round_trips_mappings_and_optionals() {
                $crate::backend::callables::fixtures::round_trips_mappings_and_optionals::<
                    $backend,
                >();
            }

            #[test]
            fn an_error_from_a_host_body_is_catchable() {
                $crate::backend::callables::fixtures::an_error_from_a_host_body_is_catchable::<
                    $backend,
                >();
            }

            #[test]
            fn init_hooks_run_once_per_guest_in_bind_order() {
                $crate::backend::callables::fixtures::init_hooks_run_once_per_guest_in_bind_order::<
                    $backend,
                >();
            }
        };
    }

    pub use crate::__guestpy_backend_callables_tests as tests;
}
