//! Guest generator handles.

use std::{
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures::Stream;

use crate::{
    backend::{
        Backend, BackendCallables, BackendCoroutines, BackendExceptions, BackendInterrupt,
        BackendModules, BackendValues, Step, Tok, Val,
    },
    driver::CoroutineFuture,
    errors::Error,
    guest::Guest,
    handle::{AsyncIter, Handle, Iter, Value},
    marshal::{FromGuest, ToGuest},
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
                return Err(Error::type_mismatch("generator", &B::type_name(enter.token(), value)));
            }
        }

        Ok(())
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

    pub fn collect<T: FromGuest<B>>(&self) -> Result<Vec<T::Owned>, Error> {
        std::iter::from_fn(|| self.next::<T>().transpose()).collect()
    }

    pub fn iter(&self) -> Iter<B> {
        Iter::from_handle(self.0.clone())
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

pub struct AsyncGenerator<B: Backend, T> {
    owned: B::Owned,
    guest: Guest<B>,
    current: Option<CoroutineFuture<B, T>>,
    marker: PhantomData<fn() -> T>,
}

impl<B: Backend, T> Unpin for AsyncGenerator<B, T> {}

impl<B: Backend, T> AsyncGenerator<B, T> {
    fn from_parts(owned: B::Owned, guest: Guest<B>) -> Self {
        Self {
            owned,
            guest,
            current: None,
            marker: PhantomData,
        }
    }
}

impl<B, T> AsyncGenerator<B, T>
where
    B: Backend + BackendValues,
{
    fn validate<'py>(enter: &Enter<'py, B>, value: &B::Value<'py>) -> Result<(), Error> {
        for method in ["__anext__", "asend", "athrow", "aclose"] {
            if !matches!(
                B::get_attr(enter.token(), value, method),
                Ok(member) if B::is_callable(enter.token(), &member),
            ) {
                return Err(Error::type_mismatch(
                    "async generator",
                    &B::type_name(enter.token(), value),
                ));
            }
        }

        Ok(())
    }
}

impl<B, T> AsyncGenerator<B, T>
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
    fn advance<'py>(
        &self,
        enter: &Enter<'py, B>,
        operation: impl FnOnce(Tok<'py, B>, &Val<'py, B>) -> Result<Val<'py, B>, Error>,
    ) -> Result<CoroutineFuture<B, T>, Error> {
        Ok(CoroutineFuture::new(
            self.guest.clone(),
            B::detach(
                enter.token(),
                operation(
                    enter.token(),
                    &B::attach(enter.token(), &self.owned),
                )?,
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

    pub fn iter(&self) -> AsyncIter<B, T> {
        AsyncIter::from_parts(self.owned.clone(), self.guest.clone())
    }

    pub async fn anext(&self) -> Result<Option<T::Owned>, Error> {
        Self::resolve(
            self.guest.enter(|enter| {
                self.advance(enter, |token, generator| B::anext(token, generator))
            })?,
        )
        .await
    }

    pub async fn asend<A: ToGuest<B>>(&self, value: A) -> Result<Option<T::Owned>, Error> {
        Self::resolve(self.guest.enter(|enter| {
            self.advance(enter, |token, generator| {
                B::asend(token, generator, value.to_guest(enter)?)
            })
        })?)
        .await
    }

    pub async fn athrow<E: ToGuest<B>>(&self, exception: E) -> Result<Option<T::Owned>, Error> {
        Self::resolve(self.guest.enter(|enter| {
            self.advance(enter, |token, generator| {
                B::athrow(token, generator, exception.to_guest(enter)?)
            })
        })?)
        .await
    }

    pub async fn aclose(&self) -> Result<(), Error> {
        self.guest
            .enter(|enter| {
                Ok(CoroutineFuture::<B, ()>::new(
                    self.guest.clone(),
                    B::detach(
                        enter.token(),
                        B::aclose(enter.token(), &B::attach(enter.token(), &self.owned))?,
                    ),
                ))
            })?
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

impl<B, T> FromGuest<B> for AsyncGenerator<B, T>
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

impl<B, T> ToGuest<B> for AsyncGenerator<B, T>
where
    B: Backend,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        Ok(B::attach(enter.token(), &self.owned))
    }
}

impl<B, T> Stream for AsyncGenerator<B, T>
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
                .enter(|enter| this.advance(enter, |token, generator| B::anext(token, generator)))
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
