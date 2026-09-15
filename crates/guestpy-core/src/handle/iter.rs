//! Guest iterator handles.

use std::{
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures::Stream;

use crate::{
    backend::{
        Backend, BackendCallables, BackendClasses, BackendCoroutines, BackendInterrupt,
        BackendModules, BackendValues,
    },
    driver::AsyncCursor,
    errors::Error,
    handle::{
        base::{Handle, Value},
        traits::HasHandle,
    },
    marshal::{
        FromGuest, ToGuest,
        describe::{Describe, Expected},
    },
    scope::Enter,
};

pub struct Iter<B: Backend>(Handle<B>);

impl<B: Backend> Clone for Iter<B> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<B: Backend> Iter<B> {
    pub(crate) fn from_handle(handle: Handle<B>) -> Self {
        Self(handle)
    }
}

impl<B: Backend> HasHandle<B> for Iter<B> {
    fn handle(&self) -> &Handle<B> {
        &self.0
    }
}

impl<B> Iter<B>
where
    B: Backend + BackendValues,
{
    pub fn next<T: FromGuest<B>>(&self) -> Result<Option<T::Owned>, Error> {
        self.0.with_enter(|enter, iterator| {
            B::next(enter.token(), iterator)?
                .map(|value| T::from_guest(enter, value))
                .transpose()
        })
    }

    pub fn collect<T: FromGuest<B>>(self) -> Result<Vec<T::Owned>, Error> {
        std::iter::from_fn(|| self.next::<T>().transpose()).collect()
    }
}

impl<B: Backend> Describe for Iter<B> {
    fn describe(expected: &mut Expected) {
        expected.push("iterable");
    }
}

impl<B> FromGuest<B> for Iter<B>
where
    B: Backend + BackendValues,
{
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        if !B::is_iterable(enter.token(), &value) {
            return Err(Error::mismatch::<Self>(&B::type_name(enter.token(), &value)));
        }

        Ok(Self(Handle::from_value(enter, B::iter(enter.token(), &value)?)))
    }
}

impl<B> ToGuest<B> for Iter<B>
where
    B: Backend + BackendValues,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        Ok(B::attach(enter.token(), self.0.owned()))
    }
}

impl<B> Iterator for Iter<B>
where
    B: Backend + BackendValues,
{
    type Item = Result<Value<B>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        Iter::next::<Value<B>>(self).transpose()
    }
}

pub struct AsyncIter<B, T>
where
    B: Backend + BackendCoroutines + BackendClasses + BackendModules,
{
    handle: Handle<B>,
    cursor: AsyncCursor<B, T>,
    marker: PhantomData<fn() -> T>,
}

impl<B, T> AsyncIter<B, T>
where
    B: Backend + BackendCoroutines + BackendClasses + BackendModules,
{
    pub(crate) fn from_handle(handle: Handle<B>) -> Self {
        Self {
            handle,
            cursor: AsyncCursor::default(),
            marker: PhantomData,
        }
    }
}

impl<B, T> Unpin for AsyncIter<B, T> where
    B: Backend + BackendCoroutines + BackendClasses + BackendModules
{
}

impl<B, T> AsyncIter<B, T>
where
    B: Backend
        + BackendValues
        + BackendCallables
        + BackendClasses
        + BackendModules
        + BackendCoroutines
        + BackendInterrupt,
    T: FromGuest<B>,
{
    pub async fn anext(&self) -> Result<Option<T::Owned>, Error> {
        self.handle
            .with_async_step::<T>(|enter, iterator| B::anext(enter.token(), iterator))
            .await
    }

    pub async fn collect(self) -> Result<Vec<T::Owned>, Error> {
        let mut items = Vec::new();

        while let Some(item) = self.anext().await? {
            items.push(item);
        }

        Ok(items)
    }
}

impl<B, T> AsyncIter<B, T>
where
    B: Backend + BackendValues + BackendCoroutines + BackendClasses + BackendModules,
{
    pub(crate) fn validate<'py>(enter: &Enter<'py, B>, value: &B::Value<'py>) -> Result<(), Error> {
        match B::get_attr(enter.token(), value, "__anext__") {
            Ok(anext) if B::is_callable(enter.token(), &anext) => Ok(()),
            _ => Err(Error::mismatch::<Self>(&B::type_name(enter.token(), value))),
        }
    }
}

impl<B, T> Describe for AsyncIter<B, T>
where
    B: Backend + BackendCoroutines + BackendClasses + BackendModules,
{
    fn describe(expected: &mut Expected) {
        expected.push("async iterator");
    }
}

impl<B, T> FromGuest<B> for AsyncIter<B, T>
where
    B: Backend + BackendValues + BackendCoroutines + BackendClasses + BackendModules,
    T: 'static,
{
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        Self::validate(enter, &value)?;
        Ok(Self::from_handle(Handle::from_value(enter, value)))
    }
}

impl<B, T> ToGuest<B> for AsyncIter<B, T>
where
    B: Backend + BackendValues + BackendCoroutines + BackendClasses + BackendModules,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        Ok(B::attach(enter.token(), self.handle.owned()))
    }
}

impl<B, T> Stream for AsyncIter<B, T>
where
    B: Backend
        + BackendValues
        + BackendCallables
        + BackendModules
        + BackendCoroutines
        + BackendClasses
        + BackendInterrupt,
    T: FromGuest<B>,
{
    type Item = Result<T::Owned, Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        this.cursor
            .poll(cx, || {
                this.handle
                    .with_async_step::<T>(|enter, iterator| B::anext(enter.token(), iterator))
            })
            .map(Result::transpose)
    }
}

pub struct AsyncIterable<B, T>(pub AsyncIter<B, T>)
where
    B: Backend + BackendCoroutines + BackendClasses + BackendModules;

impl<B, T> AsyncIterable<B, T>
where
    B: Backend + BackendCoroutines + BackendClasses + BackendModules,
{
    pub fn as_inner(&self) -> &AsyncIter<B, T> {
        &self.0
    }

    pub fn into_inner(self) -> AsyncIter<B, T> {
        self.0
    }
}

impl<B, T> FromGuest<B> for AsyncIterable<B, T>
where
    B: Backend + BackendValues + BackendCoroutines + BackendClasses + BackendModules,
    T: 'static,
{
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        AsyncIter::<B, T>::from_guest(
            enter,
            B::call(
                enter.token(),
                &match B::get_attr(enter.token(), &value, "__aiter__") {
                    Ok(aiter) if B::is_callable(enter.token(), &aiter) => aiter,
                    _ => {
                        return Err(Error::mismatch::<Self>(&B::type_name(enter.token(), &value)));
                    }
                },
                &[],
                &[],
            )?,
        )
        .map(Self)
    }
}

impl<B, T> Describe for AsyncIterable<B, T>
where
    B: Backend + BackendCoroutines + BackendClasses + BackendModules,
{
    fn describe(expected: &mut Expected) {
        expected.push("async iterable");
    }
}

#[cfg(test)]
mod tests {
    use super::{AsyncIter, Iter};
    use crate::{backend::tests::Stub, handle::Value, marshal::ToGuest};

    fn accepts<T: ToGuest<Stub>>() {}

    #[test]
    fn iterator_handles_can_return_to_the_guest() {
        accepts::<Iter<Stub>>();
        accepts::<AsyncIter<Stub, Value<Stub>>>();
    }
}
