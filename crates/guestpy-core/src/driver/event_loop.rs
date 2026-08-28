use std::{cell::Cell, rc::Rc, time::Duration};

use crate::{
    backend::{
        Backend, BackendCallables, BackendCoroutines, BackendModules, BackendValues,
        callables::{RawBody, RawCall},
    },
    driver::progress::Progress,
    errors::Error,
    scope::Enter,
};

pub(crate) struct EventLoop<B: Backend> {
    asyncio_loop: B::Owned,
    started: Cell<bool>,
    selector_timeout: Rc<Cell<Option<f64>>>,
}

impl<B> EventLoop<B>
where
    B: Backend + BackendValues + BackendCallables + BackendModules + BackendCoroutines,
{
    pub(crate) fn new<'py>(enter: &Enter<'py, B>) -> Result<Self, Error> {
        let selector_timeout = Rc::new(Cell::new(None));
        let base_events = enter
            .guest()
            .real_module(enter, "asyncio.base_events")?;
        let asyncio_loop = B::call(
            enter.token(),
            &B::get_attr(enter.token(), &base_events, "BaseEventLoop")?,
            &[],
            &[],
        )?;

        B::set_attr(
            enter.token(),
            &asyncio_loop,
            "_selector",
            NullSelector::realise(enter, &selector_timeout)?,
        )?;
        B::set_attr(
            enter.token(),
            &asyncio_loop,
            "_process_events",
            Self::noop(enter, "_process_events")?,
        )?;
        B::set_attr(
            enter.token(),
            &asyncio_loop,
            "_write_to_self",
            Self::noop(enter, "_write_to_self")?,
        )?;

        Ok(Self {
            asyncio_loop: B::detach(enter.token(), asyncio_loop),
            started: Cell::new(false),
            selector_timeout,
        })
    }

    fn noop<'py>(enter: &Enter<'py, B>, name: &str) -> Result<B::Value<'py>, Error> {
        let body: RawBody<B> = Rc::new(|call| Ok(B::none(call.token)));

        B::function(enter.token(), name, None, body)
    }

    pub(crate) fn step<'py>(&self, enter: &Enter<'py, B>) -> Result<Progress, Error> {
        let asyncio_loop = B::attach(enter.token(), &self.asyncio_loop);

        if self.started.get() {
            B::set_running_loop(enter.token(), Some(&asyncio_loop))?;
        } else {
            B::call(
                enter.token(),
                &B::get_attr(enter.token(), &asyncio_loop, "_run_forever_setup")?,
                &[],
                &[],
            )?;
            self.started.set(true);
        }

        self.selector_timeout.set(None);
        B::call(enter.token(), &B::get_attr(enter.token(), &asyncio_loop, "_run_once")?, &[], &[])?;

        Ok(match self.selector_timeout.get() {
            Some(delay) if delay <= 0.0 => Progress::Ready,
            Some(delay) => Progress::Waiting(Duration::from_secs_f64(delay)),
            None => Progress::Idle,
        })
    }
}

impl<B: Backend> EventLoop<B> {
    pub(crate) fn asyncio_loop<'py>(&self, enter: &Enter<'py, B>) -> B::Value<'py> {
        B::attach(enter.token(), &self.asyncio_loop)
    }
}

struct NullSelector;

impl NullSelector {
    fn realise<'py, B>(
        enter: &Enter<'py, B>,
        selector_timeout: &Rc<Cell<Option<f64>>>,
    ) -> Result<B::Value<'py>, Error>
    where
        B: Backend + BackendValues + BackendCallables,
    {
        let recorder = selector_timeout.clone();
        let select: RawBody<B> = Rc::new(move |call| {
            let RawCall { token, positional, .. } = call;

            recorder.set(match positional.last() {
                Some(value) if !B::is_none(token, value) => Some(B::as_f64(token, value)?),
                _ => None,
            });

            B::list(token, Vec::new())
        });
        let close: RawBody<B> = Rc::new(|call| Ok(B::none(call.token)));
        let members = B::new_dict(enter.token())?;

        B::set_item(
            enter.token(),
            &members,
            B::str(enter.token(), "select"),
            B::function(enter.token(), "select", None, select)?,
        )?;
        B::set_item(
            enter.token(),
            &members,
            B::str(enter.token(), "close"),
            B::function(enter.token(), "close", None, close)?,
        )?;

        let builtins = B::context_builtins(enter.token(), enter.guest().context());
        let class = B::call(
            enter.token(),
            &B::get_item(enter.token(), &builtins, &B::str(enter.token(), "type"))?,
            &[
                B::str(enter.token(), "NullSelector"),
                B::tuple(enter.token(), Vec::new())?,
                members,
            ],
            &[],
        )?;

        B::call(enter.token(), &class, &[], &[])
    }
}
