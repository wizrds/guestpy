//! Guest coroutine handles.

use std::{future::IntoFuture, marker::PhantomData};

use crate::{
    backend::{
        Backend, BackendCallables, BackendCoroutines, BackendExceptions, BackendInterrupt,
        BackendModules, BackendValues,
    },
    driver::CoroutineFuture,
    errors::Error,
    guest::Guest,
    marshal::FromGuest,
    scope::Enter,
};

pub struct Coroutine<B: Backend, T> {
    owned: B::Owned,
    guest: Guest<B>,
    marker: PhantomData<fn() -> T>,
}

impl<B, T> FromGuest<B> for Coroutine<B, T>
where
    B: Backend + BackendCoroutines,
    T: 'static,
{
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self, Error> {
        if !B::is_awaitable(enter.token(), &value) {
            return Err(Error::unsupported("value is not awaitable"));
        }

        Ok(Self {
            owned: B::detach(enter.token(), value),
            guest: enter.guest().clone(),
            marker: PhantomData,
        })
    }
}

impl<B, T> IntoFuture for Coroutine<B, T>
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
    type Output = Result<T::Owned, Error>;
    type IntoFuture = CoroutineFuture<B, T>;

    fn into_future(self) -> Self::IntoFuture {
        CoroutineFuture::new(self.guest, self.owned)
    }
}

pub struct Awaitable<B: Backend, T> {
    owned: B::Owned,
    guest: Guest<B>,
    marker: PhantomData<fn() -> T>,
}

impl<B: Backend, T: 'static> FromGuest<B> for Awaitable<B, T> {
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self, Error> {
        Ok(Self {
            owned: B::detach(enter.token(), value),
            guest: enter.guest().clone(),
            marker: PhantomData,
        })
    }
}

impl<B, T> IntoFuture for Awaitable<B, T>
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
    type Output = Result<T::Owned, Error>;
    type IntoFuture = CoroutineFuture<B, T>;

    fn into_future(self) -> Self::IntoFuture {
        CoroutineFuture::new(self.guest, self.owned)
    }
}
