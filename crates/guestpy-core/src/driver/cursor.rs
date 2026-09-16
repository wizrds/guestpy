use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, ready},
};

use crate::{backend::{Backend, BackendValues}, driver::step::AsyncStep, errors::Error, marshal::FromGuest};

pub(crate) struct AsyncCursor<B: Backend + BackendValues, T> {
    current: Option<AsyncStep<B, T>>,
}

impl<B: Backend + BackendValues, T> Default for AsyncCursor<B, T> {
    fn default() -> Self {
        Self { current: None }
    }
}

impl<B: Backend + BackendValues, T> AsyncCursor<B, T>
where
    AsyncStep<B, T>: Future<Output = Result<Option<T::Owned>, Error>> + Unpin,
    T: FromGuest<B>,
{
    pub(crate) fn poll(
        &mut self,
        cx: &mut Context<'_>,
        start: impl FnOnce() -> AsyncStep<B, T>,
    ) -> Poll<Result<Option<T::Owned>, Error>> {
        let outcome = ready!(Pin::new(self.current.get_or_insert_with(start)).poll(cx));

        self.current = None;

        Poll::Ready(outcome)
    }
}
