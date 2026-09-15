//! Guest generator handles.

use std::{
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures::Stream;

use crate::{
    backend::{
        Backend, BackendCallables, BackendClasses, BackendCoroutines, BackendInterrupt,
        BackendModules, BackendValues, Step,
    },
    driver::AsyncCursor,
    errors::Error,
    handle::{
        base::{Handle, Value},
        iter::{AsyncIter, Iter},
        traits::HasHandle,
    },
    marshal::{
        FromGuest, ToGuest,
        describe::{Describe, Expected},
    },
    scope::Enter,
};

pub struct Generator<B: Backend>(Handle<B>);

impl<B: Backend> Clone for Generator<B> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<B: Backend> Generator<B> {
    fn validate<'py>(enter: &Enter<'py, B>, value: &B::Value<'py>) -> Result<(), Error>
    where
        B: BackendValues,
    {
        for method in ["__next__", "send", "throw", "close"] {
            if !matches!(
                B::get_attr(enter.token(), value, method),
                Ok(member) if B::is_callable(enter.token(), &member),
            ) {
                return Err(Error::mismatch::<Self>(&B::type_name(enter.token(), value)));
            }
        }

        Ok(())
    }
}

impl<B: Backend> HasHandle<B> for Generator<B> {
    fn handle(&self) -> &Handle<B> {
        &self.0
    }
}

impl<B> Generator<B>
where
    B: Backend + BackendValues,
{
    pub fn next<T: FromGuest<B>>(&self) -> Result<Option<T::Owned>, Error> {
        self.0.with_enter(|enter, generator| {
            B::next(enter.token(), generator)?
                .map(|value| T::from_guest(enter, value))
                .transpose()
        })
    }

    pub fn send<A, T>(&self, value: A) -> Result<Step<T::Owned>, Error>
    where
        A: ToGuest<B>,
        T: FromGuest<B>,
    {
        self.0.with_enter(|enter, generator| {
            match B::send(enter.token(), generator, value.to_guest(enter)?)? {
                Step::Yielded(value) => Ok(Step::Yielded(T::from_guest(enter, value)?)),
                Step::Returned(value) => Ok(Step::Returned(T::from_guest(enter, value)?)),
            }
        })
    }

    pub fn throw<T, E>(&self, exception: E) -> Result<Step<T::Owned>, Error>
    where
        T: FromGuest<B>,
        E: ToGuest<B>,
    {
        self.0.with_enter(|enter, generator| {
            match B::throw(enter.token(), generator, exception.to_guest(enter)?)? {
                Step::Yielded(value) => Ok(Step::Yielded(T::from_guest(enter, value)?)),
                Step::Returned(value) => Ok(Step::Returned(T::from_guest(enter, value)?)),
            }
        })
    }

    pub fn close(&self) -> Result<(), Error> {
        self.0
            .with_enter(|enter, generator| B::close(enter.token(), generator))
    }

    pub fn collect<T: FromGuest<B>>(self) -> Result<Vec<T::Owned>, Error> {
        std::iter::from_fn(|| self.next::<T>().transpose()).collect()
    }

    pub fn iter(&self) -> Iter<B> {
        Iter::from_handle(self.0.clone())
    }
}

impl<B: Backend> Describe for Generator<B> {
    fn describe(expected: &mut Expected) {
        expected.push("generator");
    }
}

impl<B> FromGuest<B> for Generator<B>
where
    B: Backend + BackendValues,
{
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        Self::validate(enter, &value)?;

        Ok(Self(Handle::from_value(enter, value)))
    }
}

impl<B> ToGuest<B> for Generator<B>
where
    B: Backend,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        Ok(B::attach(enter.token(), self.0.owned()))
    }
}

impl<B> Iterator for Generator<B>
where
    B: Backend + BackendValues,
{
    type Item = Result<Value<B>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        Generator::next::<Value<B>>(self).transpose()
    }
}

pub struct AsyncGenerator<B, T>
where
    B: Backend + BackendCoroutines + BackendClasses + BackendModules,
{
    handle: Handle<B>,
    cursor: AsyncCursor<B, T>,
    marker: PhantomData<fn() -> T>,
}

impl<B, T> Unpin for AsyncGenerator<B, T> where
    B: Backend + BackendCoroutines + BackendClasses + BackendModules
{
}

impl<B, T> AsyncGenerator<B, T>
where
    B: Backend + BackendCoroutines + BackendClasses + BackendModules,
{
    fn from_handle(handle: Handle<B>) -> Self {
        Self {
            handle,
            cursor: AsyncCursor::default(),
            marker: PhantomData,
        }
    }
}

impl<B, T> AsyncGenerator<B, T>
where
    B: Backend + BackendValues + BackendCoroutines + BackendClasses + BackendModules,
{
    fn validate<'py>(enter: &Enter<'py, B>, value: &B::Value<'py>) -> Result<(), Error> {
        for method in ["__anext__", "asend", "athrow", "aclose"] {
            if !matches!(
                B::get_attr(enter.token(), value, method),
                Ok(member) if B::is_callable(enter.token(), &member),
            ) {
                return Err(Error::mismatch::<Self>(&B::type_name(enter.token(), value)));
            }
        }

        Ok(())
    }
}

impl<B, T> AsyncGenerator<B, T>
where
    B: Backend
        + BackendValues
        + BackendCoroutines
        + BackendCallables
        + BackendClasses
        + BackendModules
        + BackendInterrupt,
    T: FromGuest<B>,
{
    pub fn iter(&self) -> AsyncIter<B, T> {
        AsyncIter::from_handle(self.handle.clone())
    }

    pub async fn anext(&self) -> Result<Option<T::Owned>, Error> {
        self.handle
            .with_async_step::<T>(|enter, generator| B::anext(enter.token(), generator))
            .await
    }

    pub async fn asend<A: ToGuest<B>>(&self, value: A) -> Result<Option<T::Owned>, Error> {
        self.handle
            .with_async_step::<T>(|enter, generator| {
                B::asend(enter.token(), generator, value.to_guest(enter)?)
            })
            .await
    }

    pub async fn athrow<E: ToGuest<B>>(&self, exception: E) -> Result<Option<T::Owned>, Error> {
        self.handle
            .with_async_step::<T>(|enter, generator| {
                B::athrow(enter.token(), generator, exception.to_guest(enter)?)
            })
            .await
    }

    pub async fn aclose(&self) -> Result<(), Error> {
        self.handle
            .with_async_step::<()>(|enter, generator| B::aclose(enter.token(), generator))
            .await
            .map(|_| ())
    }

    pub async fn collect(self) -> Result<Vec<T::Owned>, Error> {
        let mut items = Vec::new();

        while let Some(item) = self.anext().await? {
            items.push(item);
        }

        Ok(items)
    }
}

impl<B, T> Describe for AsyncGenerator<B, T>
where
    B: Backend + BackendCoroutines + BackendClasses + BackendModules,
{
    fn describe(expected: &mut Expected) {
        expected.push("async generator");
    }
}

impl<B, T> FromGuest<B> for AsyncGenerator<B, T>
where
    B: Backend
        + BackendValues
        + BackendCoroutines
        + BackendCallables
        + BackendClasses
        + BackendModules
        + BackendInterrupt,
    T: 'static,
{
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        Self::validate(enter, &value)?;
        Ok(Self::from_handle(Handle::from_value(enter, value)))
    }
}

impl<B, T> ToGuest<B> for AsyncGenerator<B, T>
where
    B: Backend + BackendCoroutines + BackendClasses + BackendModules,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        Ok(B::attach(enter.token(), self.handle.owned()))
    }
}

impl<B, T> Stream for AsyncGenerator<B, T>
where
    B: Backend
        + BackendValues
        + BackendCoroutines
        + BackendCallables
        + BackendClasses
        + BackendModules
        + BackendInterrupt,
    T: FromGuest<B>,
{
    type Item = Result<T::Owned, Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        this.cursor
            .poll(cx, || {
                this.handle
                    .with_async_step::<T>(|enter, generator| B::anext(enter.token(), generator))
            })
            .map(Result::transpose)
    }
}

#[cfg(test)]
mod tests {
    use futures::Stream;

    use super::{AsyncGenerator, Generator};
    use crate::{backend::tests::Stub, errors::Error, handle::Value, marshal::ToGuest};

    fn accepts<T: ToGuest<Stub>>() {}

    fn iterates<T: Iterator<Item = Result<Value<Stub>, Error>>>() {}

    fn streams<T: Stream<Item = Result<Value<Stub>, Error>>>() {}

    #[test]
    fn generator_handles_have_their_declared_rust_interfaces() {
        accepts::<Generator<Stub>>();
        accepts::<AsyncGenerator<Stub, Value<Stub>>>();
        iterates::<Generator<Stub>>();
        streams::<AsyncGenerator<Stub, Value<Stub>>>();
    }
}
