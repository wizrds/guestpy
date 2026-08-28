use std::{
    cell::{Ref, RefCell},
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    backend::{
        Backend, BackendCallables, BackendCoroutines, BackendExceptions, BackendModules,
        BackendValues, Val, callables::HostFuture,
    },
    driver::{event_loop::EventLoop, host_futures::PendingHostFutures, progress::Progress},
    errors::Error,
    scope::Enter,
};

pub(crate) trait AsyncDriver<B: Backend> {
    fn register_host_future<'py>(
        &self,
        enter: &Enter<'py, B>,
        future: HostFuture<B>,
    ) -> Result<Val<'py, B>, Error>;

    fn prepare_awaitable<'py>(
        &self,
        enter: &Enter<'py, B>,
        awaitable: &B::Owned,
    ) -> Result<B::Owned, Error>;

    fn poll_all(&self, context: &mut Context<'_>);
    fn poll_all_now(&self);
    fn has_pending(&self) -> bool;
    fn has_ready(&self) -> bool;
    fn advance<'py>(&self, enter: &Enter<'py, B>) -> Result<Progress, Error>;
    fn cancel<'py>(&self, enter: &Enter<'py, B>) -> Result<(), Error>;
    fn close<'py>(&self, enter: &Enter<'py, B>) -> Result<(), Error>;
}

pub(crate) struct AsyncDriverSlot<B: Backend> {
    driver: RefCell<Option<Box<dyn AsyncDriver<B>>>>,
}

impl<B: Backend> AsyncDriverSlot<B> {
    pub(crate) fn new() -> Self {
        Self { driver: RefCell::new(None) }
    }

    pub(crate) fn get(&self) -> Option<Ref<'_, dyn AsyncDriver<B>>> {
        Ref::filter_map(self.driver.borrow(), |driver| driver.as_deref()).ok()
    }

    pub(crate) fn is_initialized(&self) -> bool {
        self.driver.borrow().is_some()
    }

    pub(crate) fn ensure<D, F>(&self, create: F) -> Result<(), Error>
    where
        D: AsyncDriver<B> + 'static,
        F: FnOnce() -> Result<D, Error>,
    {
        if self.driver.borrow().is_none() {
            *self.driver.borrow_mut() = Some(Box::new(create()?));
        }

        Ok(())
    }

    pub(crate) fn take(&self) -> Option<Box<dyn AsyncDriver<B>>> {
        self.driver.borrow_mut().take()
    }
}

pub(crate) struct AsyncRuntime<B: Backend> {
    event_loop: EventLoop<B>,
    pending: PendingHostFutures<B>,
}

impl<B> AsyncRuntime<B>
where
    B: Backend + BackendValues + BackendCallables + BackendModules + BackendCoroutines,
{
    pub(crate) fn new<'py>(enter: &Enter<'py, B>) -> Result<Self, Error> {
        Ok(Self {
            event_loop: EventLoop::new(enter)?,
            pending: PendingHostFutures::new(),
        })
    }

    fn tasks<'py>(&self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        B::call(
            enter.token(),
            &B::get_attr(
                enter.token(),
                &enter
                    .guest()
                    .real_module(enter, "asyncio")?,
                "all_tasks",
            )?,
            &[self.event_loop.asyncio_loop(enter)],
            &[],
        )
    }
}

impl<B> AsyncDriver<B> for AsyncRuntime<B>
where
    B: Backend
        + BackendValues
        + BackendCallables
        + BackendModules
        + BackendCoroutines
        + BackendExceptions,
{
    fn register_host_future<'py>(
        &self,
        enter: &Enter<'py, B>,
        future: HostFuture<B>,
    ) -> Result<B::Value<'py>, Error> {
        self.pending
            .register(enter.token(), &self.event_loop.asyncio_loop(enter), future)
    }

    fn prepare_awaitable<'py>(
        &self,
        enter: &Enter<'py, B>,
        awaitable: &B::Owned,
    ) -> Result<B::Owned, Error> {
        let asyncio = enter
            .guest()
            .real_module(enter, "asyncio")?;
        let task = B::call(
            enter.token(),
            &B::get_attr(enter.token(), &asyncio, "ensure_future")?,
            &[B::attach(enter.token(), awaitable)],
            &[("loop", self.event_loop.asyncio_loop(enter))],
        )?;

        Ok(B::detach(enter.token(), task))
    }

    fn poll_all(&self, context: &mut Context<'_>) {
        self.pending.poll_all(context);
    }

    fn poll_all_now(&self) {
        self.pending.poll_all_now();
    }

    fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    fn has_ready(&self) -> bool {
        self.pending.has_ready()
    }

    fn advance<'py>(&self, enter: &Enter<'py, B>) -> Result<Progress, Error> {
        self.pending.publish(enter)?;

        Ok(match self.event_loop.step(enter)? {
            Progress::Idle if self.has_pending() => Progress::Blocked,
            progress => progress,
        })
    }

    fn cancel<'py>(&self, enter: &Enter<'py, B>) -> Result<(), Error> {
        let iterator = B::iter(enter.token(), &self.tasks(enter)?)?;

        while let Some(task) = B::next(enter.token(), &iterator)? {
            B::call(enter.token(), &B::get_attr(enter.token(), &task, "cancel")?, &[], &[])?;
        }

        Ok(())
    }

    fn close<'py>(&self, enter: &Enter<'py, B>) -> Result<(), Error> {
        let iterator = B::iter(enter.token(), &self.tasks(enter)?)?;

        while let Some(task) = B::next(enter.token(), &iterator)? {
            let coroutine =
                B::call(enter.token(), &B::get_attr(enter.token(), &task, "get_coro")?, &[], &[])?;

            B::call(enter.token(), &B::get_attr(enter.token(), &coroutine, "close")?, &[], &[])?;
        }

        Ok(())
    }
}

pub(crate) struct HostFutureReady<'a, B: Backend> {
    async_driver: &'a dyn AsyncDriver<B>,
}

impl<'a, B: Backend> HostFutureReady<'a, B> {
    pub(crate) fn new(async_driver: &'a dyn AsyncDriver<B>) -> Self {
        Self { async_driver }
    }
}

impl<B: Backend> Future for HostFutureReady<'_, B> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.async_driver.poll_all(cx);

        if self.async_driver.has_ready() || !self.async_driver.has_pending() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}
