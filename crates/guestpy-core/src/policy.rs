//! Guest execution budgets and cancellation.

use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use crate::errors::Error;

struct Deadline {
    base: Instant,
    at: Cell<u64>,
}

impl Deadline {
    fn new() -> Self {
        Self {
            base: Instant::now(),
            at: Cell::new(u64::MAX),
        }
    }

    fn arm(&self, budget: Duration) {
        self.at.set(
            self.base
                .elapsed()
                .saturating_add(budget)
                .as_nanos()
                .min(u128::from(u64::MAX)) as u64,
        );
    }

    fn disarm(&self) {
        self.at.set(u64::MAX);
    }

    fn is_armed(&self) -> bool {
        self.at.get() != u64::MAX
    }

    fn expired(&self) -> bool {
        self.is_armed() && self.base.elapsed().as_nanos() as u64 >= self.at.get()
    }

    fn remaining(&self) -> Option<Duration> {
        self.is_armed().then(|| {
            Duration::from_nanos(
                self.at
                    .get()
                    .saturating_sub(self.base.elapsed().as_nanos() as u64),
            )
        })
    }
}

pub trait CancelSignal: 'static {
    fn is_cancelled(&self) -> bool;
}

#[derive(Clone, Default)]
pub struct Cancellation(Rc<Cell<bool>>);

impl Cancellation {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.set(true);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.get()
    }
}

impl CancelSignal for Cancellation {
    fn is_cancelled(&self) -> bool {
        self.0.get()
    }
}

#[cfg(feature = "tokio")]
impl CancelSignal for tokio_util::sync::CancellationToken {
    fn is_cancelled(&self) -> bool {
        tokio_util::sync::CancellationToken::is_cancelled(self)
    }
}

#[derive(Clone)]
pub(crate) struct ExecutionPolicy {
    timeout: Option<Duration>,
    cancellation: Option<Rc<dyn CancelSignal>>,
    deadline: Rc<Deadline>,
    cancel_poll_interval: Duration,
}

impl ExecutionPolicy {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn timeout(mut self, budget: impl Into<Duration>) -> Self {
        self.timeout = Some(budget.into());

        self
    }

    pub(crate) fn cancellation<S: CancelSignal>(mut self, signal: S) -> Self {
        self.cancellation = Some(Rc::new(signal));

        self
    }

    pub(crate) fn cancel_poll_interval(mut self, interval: impl Into<Duration>) -> Self {
        self.cancel_poll_interval = interval.into();

        self
    }

    pub(crate) fn derive(&self, timeout: Option<Duration>) -> Self {
        Self {
            timeout: timeout.or(self.timeout),
            cancellation: self.cancellation.clone(),
            deadline: Rc::new(Deadline::new()),
            cancel_poll_interval: self.cancel_poll_interval,
        }
    }

    pub(crate) fn begin(&self) -> Result<(), Error> {
        if self
            .cancellation
            .as_ref()
            .is_some_and(|signal| signal.is_cancelled())
        {
            return Err(Error::Cancelled);
        }

        if let Some(budget) = self.timeout
            && !self.deadline.is_armed()
        {
            self.deadline.arm(budget);
        }

        Ok(())
    }

    pub(crate) fn should_abort(&self) -> bool {
        self.cancellation
            .as_ref()
            .is_some_and(|signal| signal.is_cancelled())
            || self.deadline.expired()
    }

    pub(crate) fn abort_error(&self) -> Error {
        if self.deadline.expired() {
            Error::Timeout
        } else if self
            .cancellation
            .as_ref()
            .is_some_and(|signal| signal.is_cancelled())
        {
            Error::Cancelled
        } else {
            Error::Interrupted
        }
    }

    pub(crate) fn classify<R>(&self, result: Result<R, Error>) -> Result<R, Error> {
        match result {
            Err(Error::Interrupted) if self.deadline.expired() => Err(Error::Timeout),
            Err(Error::Interrupted)
                if self
                    .cancellation
                    .as_ref()
                    .is_some_and(|signal| signal.is_cancelled()) =>
            {
                Err(Error::Cancelled)
            }
            other => other,
        }
    }

    pub(crate) fn disarm(&self) {
        self.deadline.disarm();
    }

    pub(crate) fn remaining(&self) -> Option<Duration> {
        self.deadline.remaining()
    }

    pub(crate) fn poll_interval(&self) -> Option<Duration> {
        self.cancellation
            .as_ref()
            .map(|_| self.cancel_poll_interval)
    }
}

impl Default for ExecutionPolicy {
    fn default() -> Self {
        Self {
            timeout: None,
            cancellation: None,
            deadline: Rc::new(Deadline::new()),
            cancel_poll_interval: Duration::from_millis(100),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use super::{Cancellation, ExecutionPolicy};
    use crate::errors::Error;

    #[test]
    fn begin_rejects_a_tripped_signal() {
        let cancellation = Cancellation::new();

        cancellation.cancel();

        assert!(matches!(
            ExecutionPolicy::new()
                .cancellation(cancellation)
                .begin(),
            Err(Error::Cancelled)
        ));
    }

    #[test]
    fn nesting_does_not_rearm() {
        let policy = ExecutionPolicy::new().timeout(Duration::from_millis(50));

        policy.begin().unwrap();

        thread::sleep(Duration::from_millis(30));

        policy.begin().unwrap();

        assert!(policy.remaining().unwrap() < Duration::from_millis(25));
    }

    #[test]
    fn classify_refines_interrupted() {
        let timeout = ExecutionPolicy::new().timeout(Duration::ZERO);

        timeout.begin().unwrap();

        assert!(matches!(timeout.classify::<()>(Err(Error::Interrupted)), Err(Error::Timeout)));

        let cancellation = Cancellation::new();

        cancellation.cancel();

        assert!(matches!(
            ExecutionPolicy::new()
                .cancellation(cancellation)
                .classify::<()>(Err(Error::Interrupted)),
            Err(Error::Cancelled)
        ));

        assert!(matches!(
            ExecutionPolicy::new().classify::<()>(Err(Error::Interrupted)),
            Err(Error::Interrupted)
        ));
    }

    #[test]
    fn abort_error_prioritises_timeout() {
        let timed_out = ExecutionPolicy::new().timeout(Duration::ZERO);

        timed_out.begin().unwrap();

        assert!(matches!(timed_out.abort_error(), Error::Timeout));

        let cancelled = Cancellation::new();

        cancelled.cancel();

        assert!(matches!(
            ExecutionPolicy::new()
                .cancellation(cancelled)
                .abort_error(),
            Error::Cancelled
        ));

        assert!(matches!(ExecutionPolicy::new().abort_error(), Error::Interrupted));
    }

    #[test]
    fn poll_interval_only_with_a_signal() {
        assert_eq!(ExecutionPolicy::new().poll_interval(), None);
        assert_eq!(
            ExecutionPolicy::new()
                .cancellation(Cancellation::new())
                .poll_interval(),
            Some(Duration::from_millis(100))
        );
    }
}
