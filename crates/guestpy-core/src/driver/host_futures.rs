use std::{
    cell::RefCell,
    task::{Context, Poll, Waker},
};

use crate::{
    backend::{
        Backend, BackendExceptions, BackendValues,
        callables::{HostFuture, PendingResult},
    },
    errors::Error,
    scope::Enter,
};

struct Pending<B: Backend> {
    asyncio_future: B::Owned,
    future: HostFuture<B>,
}

struct Completed<B: Backend> {
    asyncio_future: B::Owned,
    outcome: Result<Box<dyn PendingResult<B>>, Error>,
}

struct PendingHostFutureState<B: Backend> {
    entries: Vec<Pending<B>>,
    polling: usize,
    ready: Vec<Completed<B>>,
}

impl<B: Backend> PendingHostFutureState<B> {
    fn take_entries(&mut self) -> Vec<Pending<B>> {
        let entries = self
            .entries
            .drain(..)
            .collect::<Vec<_>>();
        self.polling += entries.len();
        entries
    }

    fn settle(&mut self, entries: Vec<Pending<B>>, ready: Vec<Completed<B>>) {
        self.polling -= entries.len() + ready.len();
        self.entries.splice(0..0, entries);
        self.ready.extend(ready);
    }

    fn drain_ready(&mut self) -> Vec<Completed<B>> {
        self.ready.drain(..).collect()
    }
}

pub(crate) struct PendingHostFutures<B: Backend> {
    state: RefCell<PendingHostFutureState<B>>,
}

impl<B: Backend + BackendValues> PendingHostFutures<B> {
    pub(crate) fn new() -> Self {
        Self {
            state: RefCell::new(PendingHostFutureState {
                entries: Vec::new(),
                polling: 0,
                ready: Vec::new(),
            }),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        let state = self.state.borrow();

        state.entries.is_empty() && state.polling == 0 && state.ready.is_empty()
    }

    pub(crate) fn has_ready(&self) -> bool {
        !self.state.borrow().ready.is_empty()
    }

    pub(crate) fn register<'py>(
        &self,
        token: B::Token<'py>,
        asyncio_loop: &B::Value<'py>,
        future: HostFuture<B>,
    ) -> Result<B::Value<'py>, Error> {
        let asyncio_future =
            B::call(token, &B::get_attr(token, asyncio_loop, "create_future")?, &[], &[])?;

        self.state
            .borrow_mut()
            .entries
            .push(Pending {
                asyncio_future: B::detach(token, asyncio_future.clone()),
                future,
            });

        Ok(asyncio_future)
    }

    pub(crate) fn poll_all(&self, cx: &mut Context<'_>) {
        // Polling a host future can re-enter this scheduler. Move the current batch out
        // before polling so re-entrant registrations can borrow the state and remain
        // queued behind the unfinished entries when the batch is settled.
        let entries = self.state.borrow_mut().take_entries();

        let mut pending = Vec::new();
        let mut ready = Vec::new();

        for mut entry in entries {
            match entry.future.as_mut().poll(cx) {
                Poll::Pending => pending.push(entry),
                Poll::Ready(outcome) => {
                    ready.push(Completed {
                        asyncio_future: entry.asyncio_future,
                        outcome,
                    });
                }
            }
        }

        self.state
            .borrow_mut()
            .settle(pending, ready);
    }

    pub(crate) fn poll_all_now(&self) {
        self.poll_all(&mut Context::from_waker(Waker::noop()));
    }
}

impl<B> PendingHostFutures<B>
where
    B: Backend + BackendValues + BackendExceptions,
{
    pub(crate) fn publish<'py>(&self, enter: &Enter<'py, B>) -> Result<(), Error> {
        let ready = self.state.borrow_mut().drain_ready();

        for entry in ready {
            let future = B::attach(enter.token(), &entry.asyncio_future);
            let cancelled = B::as_bool(
                enter.token(),
                &B::call(
                    enter.token(),
                    &B::get_attr(enter.token(), &future, "cancelled")?,
                    &[],
                    &[],
                )?,
            )?;

            if cancelled {
                continue;
            }

            match entry.outcome {
                Ok(result) => {
                    B::call(
                        enter.token(),
                        &B::get_attr(enter.token(), &future, "set_result")?,
                        &[result.complete(enter)?],
                        &[],
                    )?;
                }
                Err(error) => {
                    B::call(
                        enter.token(),
                        &B::get_attr(enter.token(), &future, "set_exception")?,
                        &[B::exception_object(enter.token(), error)?],
                        &[],
                    )?;
                }
            }
        }

        Ok(())
    }
}
