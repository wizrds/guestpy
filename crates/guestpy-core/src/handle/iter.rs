//! Guest iterator handles.

use std::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures::Stream;

use crate::{
    backend::{
        Backend, BackendCallables, BackendCoroutines, BackendExceptions, BackendInterrupt,
        BackendModules, BackendValues,
    },
    driver::CoroutineFuture,
    errors::Error,
    guest::Guest,
    handle::{Handle, Value},
    marshal::{FromGuest, ToGuest},
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

    pub fn collect<T: FromGuest<B>>(&self) -> Result<Vec<T::Owned>, Error> {
        std::iter::from_fn(|| self.next::<T>().transpose()).collect()
    }
}

impl<B> FromGuest<B> for Iter<B>
where
    B: Backend + BackendValues,
{
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        if !B::is_iterable(enter.token(), &value) {
            return Err(Error::type_mismatch("iterable", &B::type_name(enter.token(), &value)));
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

pub struct AsyncIter<B: Backend, T> {
    owned: B::Owned,
    guest: Guest<B>,
    current: Option<CoroutineFuture<B, T>>,
    marker: PhantomData<fn() -> T>,
}

impl<B: Backend, T> Unpin for AsyncIter<B, T> {}

impl<B: Backend, T> AsyncIter<B, T> {
    pub(crate) fn from_parts(owned: B::Owned, guest: Guest<B>) -> Self {
        Self {
            owned,
            guest,
            current: None,
            marker: PhantomData,
        }
    }
}

impl<B, T> AsyncIter<B, T>
where
    B: Backend
        + BackendValues
        + BackendCallables
        + BackendModules
        + BackendCoroutines
        + BackendExceptions
        + BackendInterrupt,
    T: FromGuest<B>,
{
    fn advance<'py>(&self, enter: &Enter<'py, B>) -> Result<CoroutineFuture<B, T>, Error> {
        Ok(CoroutineFuture::new(
            self.guest.clone(),
            B::detach(
                enter.token(),
                B::anext(enter.token(), &B::attach(enter.token(), &self.owned))?,
            ),
        ))
    }

    async fn resolve(future: CoroutineFuture<B, T>) -> Result<Option<T::Owned>, Error> {
        match future.await {
            Ok(value) => Ok(Some(value)),
            Err(Error::Guest(exception)) if exception.matches("StopAsyncIteration") => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub async fn anext(&self) -> Result<Option<T::Owned>, Error> {
        Self::resolve(
            self.guest
                .enter(|enter| self.advance(enter))?,
        )
        .await
    }

    pub async fn collect(&self) -> Result<Vec<T::Owned>, Error> {
        let mut items = Vec::new();

        while let Some(item) = self.anext().await? {
            items.push(item);
        }

        Ok(items)
    }
}

impl<B, T> AsyncIter<B, T>
where
    B: Backend + BackendValues,
{
    pub(crate) fn validate<'py>(enter: &Enter<'py, B>, value: &B::Value<'py>) -> Result<(), Error> {
        match B::get_attr(enter.token(), value, "__anext__") {
            Ok(anext) if B::is_callable(enter.token(), &anext) => Ok(()),
            _ => Err(Error::type_mismatch("async iterator", &B::type_name(enter.token(), value))),
        }
    }
}

impl<B, T> FromGuest<B> for AsyncIter<B, T>
where
    B: Backend + BackendValues,
    T: 'static,
{
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        Self::validate(enter, &value)?;

        Ok(Self::from_parts(B::detach(enter.token(), value), enter.guest().clone()))
    }
}

impl<B, T> ToGuest<B> for AsyncIter<B, T>
where
    B: Backend,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        Ok(B::attach(enter.token(), &self.owned))
    }
}

impl<B, T> Stream for AsyncIter<B, T>
where
    B: Backend
        + BackendValues
        + BackendCallables
        + BackendModules
        + BackendCoroutines
        + BackendExceptions
        + BackendInterrupt,
    T: FromGuest<B>,
{
    type Item = Result<T::Owned, Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        if this.current.is_none() {
            match this
                .guest
                .enter(|enter| this.advance(enter))
            {
                Ok(future) => this.current = Some(future),
                Err(error) => return Poll::Ready(Some(Err(error))),
            }
        }

        match Pin::new(
            this.current
                .as_mut()
                .expect("current is set above"),
        )
        .poll(cx)
        {
            Poll::Ready(Ok(value)) => {
                this.current = None;

                Poll::Ready(Some(Ok(value)))
            }
            Poll::Ready(Err(Error::Guest(exception)))
                if exception.matches("StopAsyncIteration") =>
            {
                this.current = None;

                Poll::Ready(None)
            }
            Poll::Ready(Err(error)) => {
                this.current = None;

                Poll::Ready(Some(Err(error)))
            }
            Poll::Pending => Poll::Pending,
        }
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
