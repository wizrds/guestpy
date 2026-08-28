//! Guest construction and ownership.

mod activity;
mod builder;
mod registry;

pub(crate) use activity::ActiveGuest;
pub use builder::GuestBuilder;
pub(crate) use registry::GuestRegistry;

use std::{
    cell::{Cell, Ref},
    num::NonZeroU64,
    rc::Rc,
};

use activity::{Activation, GuestActivity};

use crate::{
    backend::{
        Backend, BackendCallables, BackendCoroutines, BackendExceptions, BackendModules,
        BackendValues,
        callables::{HostBody, RawBody, RawCall},
    },
    bundle::Bundle,
    catalog::RealisationCache,
    driver::{
        AsyncDriver, AsyncDriverSlot, AsyncRuntime, HostFutureReady, Progress, Timer, WaitTimer,
    },
    errors::Error,
    handle::{Module, Object},
    imports::{GuestBindings, Imports},
    marshal::{FromGuest, args::Args},
    policy::ExecutionPolicy,
    runtime::{Runtime, RuntimeInner},
    scope::{Enter, Scope},
};

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct GuestId(NonZeroU64);

impl GuestId {
    fn new(value: u64) -> Self {
        Self(NonZeroU64::new(value).expect("guest IDs begin at one"))
    }

    fn from_u64(value: u64) -> Option<Self> {
        NonZeroU64::new(value).map(Self)
    }

    fn value(self) -> u64 {
        self.0.get()
    }
}

pub(crate) struct GuestInner<B: Backend> {
    id: GuestId,
    runtime: Rc<RuntimeInner<B>>,
    context: B::Context,
    closed: Cell<bool>,
    activity: GuestActivity,
    bindings: GuestBindings<B>,
    async_driver: AsyncDriverSlot<B>,
}

impl<B: Backend> Drop for GuestInner<B> {
    fn drop(&mut self) {
        self.runtime
            .registry()
            .unregister(self.id);
    }
}

impl<B> GuestInner<B>
where
    B: Backend,
{
    fn new(
        id: GuestId,
        runtime: Rc<RuntimeInner<B>>,
        context: B::Context,
        policy: ExecutionPolicy,
        bindings: GuestBindings<B>,
    ) -> Self {
        Self {
            id,
            runtime,
            context,
            closed: Cell::new(false),
            activity: GuestActivity::new(policy),
            bindings,
            async_driver: AsyncDriverSlot::new(),
        }
    }

    fn enter_with<F, R>(self: &Rc<Self>, activation: Activation, f: F) -> Result<R, Error>
    where
        F: for<'py> FnOnce(&Enter<'py, B>) -> Result<R, Error>,
    {
        if self.closed.get() {
            return Err(Error::Closed);
        }

        let _active = if self
            .runtime
            .registry()
            .is_innermost(self)
        {
            None
        } else {
            Some(match activation {
                Activation::Operation => ActiveGuest::operation(self)?,
                Activation::Cleanup => ActiveGuest::cleanup(self)?,
            })
        };

        B::enter(self.runtime.engine(), |token| {
            f(&Enter::new(token, Guest { inner: self.clone() }))
        })
    }

    pub(crate) fn id(&self) -> GuestId {
        self.id
    }

    pub(crate) fn runtime(&self) -> &Rc<RuntimeInner<B>> {
        &self.runtime
    }

    pub(crate) fn activity(&self) -> &GuestActivity {
        &self.activity
    }

    pub(crate) fn enter<F, R>(self: &Rc<Self>, f: F) -> Result<R, Error>
    where
        F: for<'py> FnOnce(&Enter<'py, B>) -> Result<R, Error>,
    {
        self.enter_with(Activation::Operation, f)
    }

    pub(crate) fn enter_cleanup<F, R>(self: &Rc<Self>, f: F) -> Result<R, Error>
    where
        F: for<'py> FnOnce(&Enter<'py, B>) -> Result<R, Error>,
    {
        self.enter_with(Activation::Cleanup, f)
    }
}

impl<B> GuestInner<B>
where
    B: Backend + BackendValues,
{
    pub(crate) fn raw_body(
        runtime: &Rc<RuntimeInner<B>>,
        guest_id: GuestId,
        body: HostBody<B>,
    ) -> RawBody<B> {
        let runtime = Rc::downgrade(runtime);

        Rc::new(move |call| {
            let RawCall { token, positional, keyword } = call;
            let guest = runtime
                .upgrade()
                .ok_or(Error::Closed)?
                .registry()
                .get(guest_id)?;

            let _active = ActiveGuest::operation(&guest)?;

            body(&Enter::new(token, Guest { inner: guest }), Args::new(positional, keyword))
        })
    }
}

impl<B> GuestInner<B>
where
    B: Backend + BackendValues + BackendCallables + BackendModules,
{
    fn active(runtime: &Rc<RuntimeInner<B>>) -> Option<Rc<Self>> {
        runtime.registry().innermost()
    }

    fn attribute<'py>(
        runtime: &Rc<RuntimeInner<B>>,
        token: B::Token<'py>,
        globals: Option<&B::Value<'py>>,
    ) -> Option<Rc<Self>> {
        globals
            .filter(|globals| !B::is_none(token, globals))
            .and_then(|globals| {
                B::get_item_opt(token, globals, &B::str(token, "__guestpy_id__")).ok()?
            })
            .and_then(|value| B::as_u64(token, &value).ok())
            .and_then(GuestId::from_u64)
            .and_then(|id| runtime.registry().get(id).ok())
            .or_else(|| Self::active(runtime))
    }

    fn delegate<'py>(
        token: B::Token<'py>,
        runtime: &Rc<RuntimeInner<B>>,
        positional: &[B::Value<'py>],
        keyword: &[(String, B::Value<'py>)],
    ) -> Result<B::Value<'py>, Error> {
        B::call(
            token,
            &B::attach(token, runtime.real_import()),
            positional,
            &keyword
                .iter()
                .map(|(name, value)| (name.as_str(), value.clone()))
                .collect::<Vec<_>>(),
        )
    }

    pub(crate) fn import_body(runtime: &Rc<RuntimeInner<B>>) -> RawBody<B> {
        let runtime = Rc::downgrade(runtime);

        Rc::new(move |call| {
            let RawCall { token, positional, keyword } = call;
            let runtime = runtime.upgrade().ok_or(Error::Closed)?;

            let Some(guest) = Self::attribute(&runtime, token, positional.get(1)) else {
                return Self::delegate(token, &runtime, &positional, &keyword);
            };
            let _active = ActiveGuest::operation(&guest)?;

            Imports::new(&Enter::new(token, Guest { inner: guest }))
                .dispatch(&Args::new(positional, keyword))
        })
    }
}

pub struct Guest<B: Backend> {
    inner: Rc<GuestInner<B>>,
}

impl<B: Backend> Clone for Guest<B> {
    fn clone(&self) -> Self {
        Self { inner: self.inner.clone() }
    }
}

impl<B: Backend> Guest<B> {
    pub(crate) fn enter<F, R>(&self, f: F) -> Result<R, Error>
    where
        F: for<'py> FnOnce(&Enter<'py, B>) -> Result<R, Error>,
    {
        self.inner.enter(f)
    }

    pub(crate) fn enter_cleanup<F, R>(&self, f: F) -> Result<R, Error>
    where
        F: for<'py> FnOnce(&Enter<'py, B>) -> Result<R, Error>,
    {
        self.inner.enter_cleanup(f)
    }

    pub(crate) fn begin_operation(&self) -> Result<ActiveGuest<B>, Error> {
        ActiveGuest::operation(&self.inner)
    }

    pub(crate) fn context(&self) -> &B::Context {
        &self.inner.context
    }

    pub(crate) fn policy(&self) -> &ExecutionPolicy {
        self.inner.activity.policy()
    }

    pub(crate) fn engine(&self) -> &B::Engine {
        self.inner.runtime.engine()
    }

    pub(crate) fn real_import(&self) -> &B::Owned {
        self.inner.runtime.real_import()
    }

    pub(crate) fn realisation(&self) -> &RealisationCache<B> {
        self.inner
            .runtime
            .realisation()
    }

    pub(crate) fn bindings(&self) -> &GuestBindings<B> {
        &self.inner.bindings
    }

    pub fn id(&self) -> GuestId {
        self.inner.id
    }

    pub fn runtime(&self) -> Runtime<B> {
        Runtime { inner: self.inner.runtime.clone() }
    }

    pub fn is_closed(&self) -> bool {
        self.inner.closed.get()
    }
}

impl<B> Guest<B>
where
    B: Backend + BackendValues,
{
    pub(crate) fn raw_body(&self, body: HostBody<B>) -> RawBody<B> {
        GuestInner::raw_body(&self.inner.runtime, self.inner.id, body)
    }

    pub(crate) fn real_module<'py>(
        &self,
        enter: &Enter<'py, B>,
        name: &str,
    ) -> Result<B::Value<'py>, Error> {
        let import = B::attach(enter.token(), self.real_import());
        let mut module = B::call(
            enter.token(),
            &import,
            &[
                B::str(enter.token(), name),
                B::none(enter.token()),
                B::none(enter.token()),
                B::none(enter.token()),
                B::uint(enter.token(), 0),
            ],
            &[],
        )?;

        for part in name.split('.').skip(1) {
            module = B::get_attr(enter.token(), &module, part)?;
        }

        Ok(module)
    }
}

impl<B> Guest<B>
where
    B: Backend + BackendValues + BackendCallables + BackendModules,
{
    fn clear_context<'py>(&self, enter: &Enter<'py, B>) -> Result<(), Error> {
        B::call(
            enter.token(),
            &B::get_attr(
                enter.token(),
                &B::context_globals(enter.token(), &self.inner.context),
                "clear",
            )?,
            &[],
            &[],
        )?;

        Ok(())
    }

    fn realised(&self, dotted: &str) -> Result<Module<B>, Error> {
        self.enter(|enter| Module::from_guest(enter, Imports::new(enter).module(dotted)?))
    }

    pub fn load(&self, bundle: &Bundle) -> Result<Module<B>, Error> {
        let root = bundle
            .root()
            .ok_or(Error::AmbiguousBundle { roots: bundle.roots() })?
            .to_owned();

        self.enter(|enter| Imports::new(enter).mount(bundle, &root))?;
        self.realised(&root)
    }

    pub fn guest_module(&self, name: &str, source: &str) -> Result<Module<B>, Error> {
        self.load(&Bundle::single(name, source)?)
    }

    pub fn host_module(&self, name: &str) -> Result<Module<B>, Error> {
        if !self.enter(|enter| Ok(Imports::new(enter).is_host_module(name)))? {
            return Err(Error::import(name, "no host module of that name is bound to this guest"));
        }

        self.realised(name)
    }

    pub fn exec(&self, source: &str) -> Result<(), Error> {
        self.enter(|enter| {
            B::exec(
                enter.token(),
                source,
                "<guest>",
                &B::context_globals(enter.token(), enter.guest().context()),
            )
        })
    }

    pub fn eval<T: FromGuest<B>>(&self, source: &str) -> Result<T::Owned, Error> {
        self.enter(|enter| {
            T::from_guest(
                enter,
                B::eval(
                    enter.token(),
                    source,
                    "<guest>",
                    &B::context_globals(enter.token(), enter.guest().context()),
                )?,
            )
        })
    }

    pub fn globals(&self) -> Result<Object<B>, Error> {
        self.enter(|enter| {
            Object::from_guest(enter, B::context_globals(enter.token(), enter.guest().context()))
        })
    }

    pub async fn scope<F, R>(&self, f: F) -> Result<R, Error>
    where
        F: for<'a> AsyncFnOnce(Scope<'a, B>) -> Result<R, Error>,
        R: 'static,
    {
        if self.is_closed() {
            return Err(Error::Closed);
        }

        let _active = ActiveGuest::operation(&self.inner)?;
        f(Scope::new(self)).await
    }

    pub fn close(&self) -> Result<(), Error> {
        if self.inner.closed.get() {
            return Ok(());
        }

        if self.inner.async_driver.is_initialized() {
            return Err(Error::unexpected(
                "cannot close guest while its async driver is active",
            ));
        }

        self.enter_cleanup(|enter| self.clear_context(enter))?;
        self.inner.closed.set(true);

        Ok(())
    }
}

impl<B> Guest<B>
where
    B: Backend
        + BackendValues
        + BackendCallables
        + BackendModules
        + BackendCoroutines
        + BackendExceptions,
{
    const CLOSE_DRIVE_BUDGET: usize = 1000;

    pub(crate) fn ensure_async_driver<'a, 'py>(
        &'a self,
        enter: &Enter<'py, B>,
    ) -> Result<ActiveAsyncDriver<'a, B>, Error> {
        self.inner
            .async_driver
            .ensure(|| AsyncRuntime::new(enter))?;

        Ok(ActiveAsyncDriver { guest: self })
    }

    fn advance_entered<'py>(&self, enter: &Enter<'py, B>) -> Result<Progress, Error> {
        let advanced = self
            .async_driver()
            .expect("async driver initialized")
            .driver()
            .advance(enter);
        let restored = B::set_running_loop(enter.token(), None);
        let progress = advanced?;

        restored?;

        Ok(progress)
    }

    fn advance_with(&self, activation: Activation) -> Result<Progress, Error> {
        let _active = ActiveGuest::new(&self.inner, activation)?;

        self.async_driver()
            .expect("async driver initialized")
            .driver()
            .poll_all_now();

        match activation {
            Activation::Operation => self.enter(|enter| self.advance_entered(enter)),
            Activation::Cleanup => self.enter_cleanup(|enter| self.advance_entered(enter)),
        }
    }

    async fn drive(&self, budget: Option<usize>, activation: Activation) -> Result<(), Error> {
        let mut sleep = Timer::new();
        let mut remaining = budget;

        loop {
            if remaining == Some(0) {
                return Ok(());
            }

            match self.advance_with(activation)? {
                Progress::Idle => return Ok(()),
                Progress::Ready => tokio::task::yield_now().await,
                Progress::Blocked => {
                    let async_driver = self
                        .async_driver()
                        .expect("async driver initialized");
                    let driver = async_driver.driver();

                    HostFutureReady::new(&*driver).await;
                }
                Progress::Waiting(delay) => WaitTimer::new(&mut sleep, delay).await,
            }

            remaining = remaining.map(|left| left - 1);
        }
    }

    pub fn async_driver(&self) -> Option<ActiveAsyncDriver<'_, B>> {
        self.inner
            .async_driver
            .is_initialized()
            .then_some(ActiveAsyncDriver { guest: self })
    }

    pub fn advance(&self) -> Result<Progress, Error> {
        self.enter(|enter| {
            drop(self.ensure_async_driver(enter)?);

            Ok(())
        })?;

        self.advance_with(Activation::Operation)
    }

    pub async fn run_until_idle(&self) -> Result<(), Error> {
        self.enter(|enter| {
            drop(self.ensure_async_driver(enter)?);

            Ok(())
        })?;

        self.drive(None, Activation::Operation)
            .await
    }
}

pub struct ActiveAsyncDriver<'a, B: Backend> {
    guest: &'a Guest<B>,
}

impl<'a, B> ActiveAsyncDriver<'a, B>
where
    B: Backend
        + BackendValues
        + BackendCallables
        + BackendModules
        + BackendCoroutines
        + BackendExceptions,
{
    pub(crate) fn driver(&self) -> Ref<'_, dyn AsyncDriver<B>> {
        self.guest
            .inner
            .async_driver
            .get()
            .expect("active async driver exists")
    }

    pub async fn close(self) -> Result<(), Error> {
        self.guest
            .enter_cleanup(|enter| self.driver().cancel(enter))?;
        self.guest
            .drive(Some(Guest::<B>::CLOSE_DRIVE_BUDGET), Activation::Cleanup)
            .await?;
        self.guest
            .enter_cleanup(|enter| self.driver().close(enter))?;

        drop(self.guest.inner.async_driver.take());

        Ok(())
    }
}
