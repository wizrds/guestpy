use std::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    backend::{
        Backend, BackendCallables, BackendCoroutines, BackendExceptions, BackendInterrupt,
        BackendModules, BackendValues,
    },
    driver::{Progress, Timer},
    errors::Error,
    guest::{ActiveGuest, Guest},
    marshal::FromGuest,
    scope::Enter,
};

pub struct CoroutineFuture<B: Backend, T> {
    guest: Guest<B>,
    owned: B::Owned,
    task: Option<B::Owned>,
    active: Option<ActiveGuest<B>>,
    sleep: Timer,
    marker: PhantomData<fn() -> T>,
}

impl<B: Backend, T> Unpin for CoroutineFuture<B, T> {}

impl<B, T> CoroutineFuture<B, T>
where
    B: Backend
        + BackendValues
        + BackendCallables
        + BackendModules
        + BackendCoroutines
        + BackendExceptions,
    T: FromGuest<B>,
{
    pub(crate) fn new(guest: Guest<B>, owned: B::Owned) -> Self {
        Self {
            guest,
            owned,
            task: None,
            active: None,
            sleep: Timer::new(),
            marker: PhantomData,
        }
    }

    fn stepped<'py>(
        &mut self,
        enter: &Enter<'py, B>,
    ) -> Result<(Progress, Option<B::Value<'py>>), Error> {
        self.prepare(enter)?;

        let progress = self
            .guest
            .ensure_async_driver(enter)?
            .driver()
            .advance(enter)?;

        Ok((progress, self.task_state(enter)?))
    }

    fn prepare<'py>(&mut self, enter: &Enter<'py, B>) -> Result<(), Error> {
        if self.task.is_none() {
            self.task = Some(
                self.guest
                    .ensure_async_driver(enter)?
                    .driver()
                    .prepare_awaitable(enter, &self.owned)?,
            );
        }

        Ok(())
    }

    fn task_state<'py>(&self, enter: &Enter<'py, B>) -> Result<Option<B::Value<'py>>, Error> {
        let task = B::attach(
            enter.token(),
            self.task
                .as_ref()
                .expect("task initialized"),
        );

        if B::as_bool(
            enter.token(),
            &B::call(enter.token(), &B::get_attr(enter.token(), &task, "done")?, &[], &[])?,
        )? {
            Ok(Some(B::call(
                enter.token(),
                &B::get_attr(enter.token(), &task, "result")?,
                &[],
                &[],
            )?))
        } else {
            Ok(None)
        }
    }

    fn extract(&self, value: B::Owned) -> Result<T::Owned, Error> {
        B::enter(self.guest.engine(), |token| {
            T::from_guest(&Enter::new(token, self.guest.clone()), B::attach(token, &value))
        })
    }

    fn clamp(&self, delay: std::time::Duration) -> std::time::Duration {
        let delay = self
            .guest
            .policy()
            .remaining()
            .map_or(delay, |remaining| delay.min(remaining));

        self.guest
            .policy()
            .poll_interval()
            .map_or(delay, |interval| delay.min(interval))
    }
}

impl<B, T> Future for CoroutineFuture<B, T>
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

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        if this.active.is_none() {
            match this.guest.begin_operation() {
                Ok(active) => {
                    B::reset(this.guest.engine());
                    this.active = Some(active);
                }
                Err(error) => return Poll::Ready(Err(error)),
            }
        }

        if let Some(async_driver) = this.guest.async_driver() {
            async_driver.driver().poll_all(cx);
        }

        let guest = this.guest.clone();
        let outcome = B::enter(guest.engine(), |token| {
            let enter = Enter::new(token, this.guest.clone());

            this.guest
                .policy()
                .classify(B::check(token))?;
            if this.guest.policy().should_abort() {
                return Err(this.guest.policy().abort_error());
            }

            let stepped = this.stepped(&enter);
            let restored = B::set_running_loop(token, None);
            let (progress, done) = stepped?;

            restored?;

            Ok((progress, done.map(|value| B::detach(token, value))))
        });

        match this.guest.policy().classify(outcome) {
            Err(error) => Poll::Ready(Err(error)),
            Ok((_, Some(value))) => Poll::Ready(this.extract(value)),
            Ok((Progress::Ready, _)) => {
                cx.waker().wake_by_ref();

                Poll::Pending
            }
            Ok((Progress::Waiting(delay), _)) => {
                if this
                    .sleep
                    .poll_after(this.clamp(delay), cx)
                    .is_ready()
                {
                    cx.waker().wake_by_ref();
                }

                Poll::Pending
            }
            Ok((Progress::Blocked | Progress::Idle, _)) => {
                if let Some(async_driver) = this.guest.async_driver() {
                    let driver = async_driver.driver();

                    driver.poll_all(cx);

                    if driver.has_ready() {
                        cx.waker().wake_by_ref();
                    }
                }

                Poll::Pending
            }
        }
    }
}
