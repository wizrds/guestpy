use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, ready},
};

use crate::{
    backend::{
        Backend, BackendCallables, BackendCoroutines, BackendInterrupt, BackendModules,
        BackendValues,
    },
    driver::CoroutineFuture,
    errors::Error,
    marshal::FromGuest,
};

pub(crate) enum AsyncStep<B: Backend, T> {
    Pending(CoroutineFuture<B, T>),
    Failed(Option<Error>),
}

impl<B: Backend, T> Unpin for AsyncStep<B, T> {}

impl<B: Backend, T> From<Result<CoroutineFuture<B, T>, Error>> for AsyncStep<B, T> {
    fn from(started: Result<CoroutineFuture<B, T>, Error>) -> Self {
        match started {
            Ok(future) => Self::Pending(future),
            Err(error) => Self::Failed(Some(error)),
        }
    }
}

impl<B, T> AsyncStep<B, T>
where
    B: Backend
        + BackendValues
        + BackendCallables
        + BackendModules
        + BackendCoroutines
        + BackendInterrupt,
    T: FromGuest<B>,
{
    fn settle(outcome: Result<T::Owned, Error>) -> Result<Option<T::Owned>, Error> {
        match outcome {
            Ok(value) => Ok(Some(value)),
            Err(Error::StopAsyncIteration) => Ok(None),
            Err(Error::Guest(exception)) if exception.matches("StopAsyncIteration") => Ok(None),
            Err(error) => Err(error),
        }
    }
}

impl<B, T> Future for AsyncStep<B, T>
where
    B: Backend
        + BackendValues
        + BackendCallables
        + BackendModules
        + BackendCoroutines
        + BackendInterrupt,
    T: FromGuest<B>,
{
    type Output = Result<Option<T::Owned>, Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Ready(Self::settle(match self.get_mut() {
            Self::Pending(future) => ready!(Pin::new(future).poll(cx)),
            Self::Failed(error) => Err(
                error
                    .take()
                    .expect("async step polled after completion")
            ),
        }))
    }
}
