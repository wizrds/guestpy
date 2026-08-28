use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::{Duration, Instant},
};

pub(crate) struct Timer {
    sleep: Option<Pin<Box<dyn Future<Output = ()>>>>,
    deadline: Option<Instant>,
}

impl Timer {
    const SLACK: Duration = Duration::from_millis(1);

    pub(crate) fn new() -> Self {
        Self { sleep: None, deadline: None }
    }

    fn sleep(duration: Duration) -> impl Future<Output = ()> {
        tokio::time::sleep(duration)
    }

    pub(crate) fn poll_after(&mut self, duration: Duration, cx: &mut Context<'_>) -> Poll<()> {
        let now = Instant::now();
        let target = now + duration;

        let reuse = matches!(
            self.deadline,
            Some(existing)
                if self.sleep.is_some()
                    && existing > now
                    && existing <= target + Self::SLACK
        );
        if !reuse {
            self.sleep = Some(Box::pin(Self::sleep(duration)));
            self.deadline = Some(target);
        }

        match self
            .sleep
            .as_mut()
            .expect("timer is armed")
            .as_mut()
            .poll(cx)
        {
            Poll::Pending => Poll::Pending,
            Poll::Ready(()) => {
                self.sleep = None;
                self.deadline = None;

                Poll::Ready(())
            }
        }
    }
}

pub(crate) struct WaitTimer<'a> {
    timer: &'a mut Timer,
    delay: Duration,
}

impl<'a> WaitTimer<'a> {
    pub(crate) fn new(timer: &'a mut Timer, delay: Duration) -> Self {
        Self { timer, delay }
    }
}

impl Future for WaitTimer<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();

        this.timer.poll_after(this.delay, cx)
    }
}
