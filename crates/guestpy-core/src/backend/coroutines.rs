use super::{Backend, BackendValues, Tok, Val};
use crate::errors::Error;

pub trait BackendCoroutines: Backend + BackendValues {
    fn is_coroutine<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> bool;

    fn is_awaitable<'py>(token: Tok<'py, Self>, value: &Val<'py, Self>) -> bool;

    fn anext<'py>(
        token: Tok<'py, Self>,
        async_iterator: &Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error>;

    fn asend<'py>(
        token: Tok<'py, Self>,
        async_generator: &Val<'py, Self>,
        value: Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error>;

    fn athrow<'py>(
        token: Tok<'py, Self>,
        async_generator: &Val<'py, Self>,
        exception: Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error>;

    fn aclose<'py>(
        token: Tok<'py, Self>,
        async_generator: &Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error>;

    fn set_running_loop<'py>(
        token: Tok<'py, Self>,
        asyncio_loop: Option<&Val<'py, Self>>,
    ) -> Result<(), Error>;
}

#[doc(hidden)]
pub mod fixtures {
    use crate::{
        backend::{
            Backend, BackendCallables, BackendClasses, BackendCoroutines, BackendInterrupt,
            BackendModules, BackendValues, guest_fixture,
        },
        errors::Error,
        handle::{AsyncGenerator, AsyncIter, AsyncIterable, Coroutine, Object, ObjectProtocol},
        runtime::Runtime,
        host::{module::ModuleSpec, iter::HostStream},
    };

    struct Streams;

    impl Streams {
        fn module<B>() -> ModuleSpec<B>
        where
            B: Backend
                + BackendValues
                + BackendCallables
                + BackendClasses
                + BackendModules
                + BackendCoroutines
                + BackendInterrupt,
        {
            ModuleSpec::new("streams")
                .function("numbers", |_, _| {
                    Ok::<_, Error>(HostStream::new(futures::stream::iter([
                        Ok::<_, Error>(1_i64),
                        Ok(2),
                        Ok(3),
                    ])))
                })
                .function("failing", |_, _| {
                    Ok::<_, Error>(HostStream::new(futures::stream::iter([
                        Ok::<_, Error>(1_i64),
                        Err(Error::conversion("deliberate failure")),
                    ])))
                })
        }
    }

    guest_fixture! {
        pub async fn anext_advances_a_plain_async_iterator<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder();
        |guest| {
            guest
                .exec(
                    r#"
class Counter:
    def __init__(self):
        self.value = 0

    def __aiter__(self):
        return self

    async def __anext__(self):
        self.value += 1
        if self.value > 2:
            raise StopAsyncIteration
        return self.value
"#,
                )
                .unwrap();

            assert_eq!(
                guest
                    .eval::<AsyncIter<B, i64>>("Counter()")
                    .unwrap()
                    .collect()
                    .await
                    .unwrap(),
                vec![1, 2],
            );
        }
    }

    guest_fixture! {
        pub async fn accepts_and_delegates_async_iterables<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder();
        |guest| {
            guest
                .exec(
                    r#"
class Counter:
    def __init__(self):
        self.value = 0

    def __aiter__(self):
        return self

    async def __anext__(self):
        self.value += 1
        if self.value > 2:
            raise StopAsyncIteration
        return self.value

class NonCallableAsyncIterable:
    __aiter__ = 42

class InvalidAsyncIterable:
    def __aiter__(self):
        return 42

class BrokenAsyncIterable:
    def __aiter__(self):
        raise ValueError("broken aiter")
"#,
                )
                .unwrap();

            assert_eq!(
                guest
                    .eval::<AsyncIterable<B, i64>>("Counter()")
                    .unwrap()
                    .0
                    .collect()
                    .await
                    .unwrap(),
                vec![1, 2],
            );
            assert_eq!(
                guest
                    .eval::<Object<B>>("Counter()")
                    .unwrap()
                    .cast::<AsyncIterable<B, i64>>()
                    .unwrap()
                    .into_inner()
                    .collect()
                    .await
                    .unwrap(),
                vec![1, 2],
            );

            let non_callable = guest
                .eval::<AsyncIterable<B, i64>>("NonCallableAsyncIterable()")
                .err()
                .unwrap();
            let invalid = guest
                .eval::<AsyncIterable<B, i64>>("InvalidAsyncIterable()")
                .err()
                .unwrap();

            assert!(matches!(&non_callable, Error::Conversion { .. }));
            assert!(
                non_callable
                    .to_string()
                    .contains("expected async iterable, got NonCallableAsyncIterable"),
            );
            assert!(matches!(&invalid, Error::Conversion { .. }));
            assert!(
                invalid
                    .to_string()
                    .contains("expected async iterator, got int"),
            );
            assert!(matches!(
                guest
                    .eval::<AsyncIterable<B, i64>>("BrokenAsyncIterable()")
                    .err()
                    .unwrap(),
                Error::Guest(_),
            ));
        }
    }

    guest_fixture! {
        pub async fn controls_an_async_generator<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder();
        |guest| {
            guest
                .exec(
                    r#"
async def values():
    try:
        value = yield 1
        yield value
    except ValueError:
        yield 3
"#,
                )
                .unwrap();

            let generator = guest
                .eval::<AsyncGenerator<B, i64>>("values()")
                .unwrap();

            assert_eq!(generator.anext().await.unwrap(), Some(1));
            assert_eq!(generator.asend(2).await.unwrap(), Some(2));
            assert_eq!(
                generator
                    .athrow(
                        guest
                            .eval::<Object<B>>("ValueError('boom')")
                            .unwrap(),
                    )
                    .await
                    .unwrap(),
                Some(3),
            );
            assert_eq!(generator.aclose().await.unwrap(), ());
        }
    }

    guest_fixture! {
        pub async fn async_for_awaits_non_coroutine_anext_results<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder();
        |guest| {
            guest
                .exec(
                    r#"
class AwaitOnly:
    def __init__(self, value):
        self.value = value

    def __await__(self):
        return self.value
        yield

class Numbers:
    def __init__(self):
        self.remaining = [1, 2, 3]

    def __aiter__(self):
        return self

    def __anext__(self):
        if not self.remaining:
            raise StopAsyncIteration
        return AwaitOnly(self.remaining.pop(0))

async def run():
    return [value async for value in Numbers()]
"#,
                )
                .unwrap();

            assert_eq!(
                guest
                    .eval::<Coroutine<B, Vec<i64>>>("run()")
                    .unwrap()
                    .await
                    .unwrap(),
                vec![1, 2, 3],
            );
        }
    }

    guest_fixture! {
        pub async fn async_for_drives_a_host_stream<B>()
        where B: [
            Backend,
            BackendValues,
            BackendCallables,
            BackendClasses,
            BackendModules,
            BackendCoroutines,
            BackendInterrupt,
        ]
        using Runtime::<B>::builder().bind(Streams::module());
        |guest| {
            guest
                .exec(
                    r#"
import asyncio, inspect, streams

async def collect():
    return [value async for value in streams.numbers()]

async def protocol():
    stream = streams.numbers()
    first = stream.__anext__()
    assert inspect.isawaitable(first)
    assert asyncio.isfuture(first)
    assert await first == 1
    assert await anext(stream) == 2
    assert await anext(stream) == 3
    return await anext(stream, 0)

async def failing():
    seen = []
    try:
        async for value in streams.failing():
            seen.append(value)
    except Exception as error:
        return seen, str(error)
"#,
                )
                .unwrap();

            assert_eq!(
                guest
                    .eval::<Coroutine<B, Vec<i64>>>("collect()")
                    .unwrap()
                    .await
                    .unwrap(),
                vec![1, 2, 3],
            );

            assert_eq!(
                guest
                    .eval::<Coroutine<B, i64>>("protocol()")
                    .unwrap()
                    .await
                    .unwrap(),
                0,
            );

            let (seen, message) = guest
                .eval::<Coroutine<B, (Vec<i64>, String)>>("failing()")
                .unwrap()
                .await
                .unwrap();

            assert_eq!(seen, vec![1]);
            assert!(message.contains("deliberate failure"));
        }
    }

    #[doc(hidden)]
    #[macro_export]
    macro_rules! __guestpy_backend_coroutines_tests {
        ($backend:ty) => {
            #[tokio::test]
            async fn anext_advances_a_plain_async_iterator() {
                $crate::backend::coroutines::fixtures::anext_advances_a_plain_async_iterator::<
                    $backend,
                >()
                .await;
            }

            #[tokio::test]
            async fn accepts_and_delegates_async_iterables() {
                $crate::backend::coroutines::fixtures::accepts_and_delegates_async_iterables::<
                    $backend,
                >()
                .await;
            }

            #[tokio::test]
            async fn controls_an_async_generator() {
                $crate::backend::coroutines::fixtures::controls_an_async_generator::<$backend>()
                    .await;
            }

            #[tokio::test]
            async fn async_for_awaits_non_coroutine_anext_results() {
                $crate::backend::coroutines::fixtures::async_for_awaits_non_coroutine_anext_results::<
                    $backend,
                >()
                .await;
            }

            #[tokio::test]
            async fn async_for_drives_a_host_stream() {
                $crate::backend::coroutines::fixtures::async_for_drives_a_host_stream::<$backend>()
                    .await;
            }
        };
    }

    pub use crate::__guestpy_backend_coroutines_tests as tests;
}
