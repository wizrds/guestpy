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
            Backend, BackendCallables, BackendClasses, BackendCoroutines, BackendExceptions,
            BackendInterrupt, BackendModules, BackendValues, guest_fixture,
        },
        handle::{AsyncGenerator, AsyncIter, Object},
        runtime::Runtime,
    };

    guest_fixture! {
        pub async fn anext_advances_a_plain_async_iterator<B>()
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
        pub async fn controls_an_async_generator<B>()
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
            async fn controls_an_async_generator() {
                $crate::backend::coroutines::fixtures::controls_an_async_generator::<
                    $backend,
                >()
                .await;
            }
        };
    }

    pub use crate::__guestpy_backend_coroutines_tests as tests;
}
