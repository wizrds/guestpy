use std::{future::Future, rc::Rc};

use crate::{
    backend::{
        Backend, BackendCallables, BackendCoroutines, BackendExceptions, BackendModules,
        BackendValues, Val,
        callables::{HostAsyncBody, HostBody, PendingValue},
    },
    errors::Error,
    host::declaration::{DeclarationContext, DeclareMember, Member},
    marshal::{ToGuest, args::Args},
    scope::Enter,
};

pub(crate) struct FunctionDeclaration<B: Backend> {
    body: HostBody<B>,
}

impl<B: Backend> FunctionDeclaration<B> {
    pub(crate) fn new(body: HostBody<B>) -> Self {
        Self { body }
    }
}

impl<B> DeclareMember<B> for FunctionDeclaration<B>
where
    B: Backend + BackendValues + BackendCallables,
{
    fn realise<'py>(
        &self,
        context: &DeclarationContext<'py, '_, B>,
        name: &str,
    ) -> Result<Val<'py, B>, Error> {
        B::function(
            context.enter().token(),
            name,
            None,
            context
                .enter()
                .guest()
                .raw_body(self.body.clone()),
        )
    }
}

pub(crate) struct AsyncFunctionDeclaration<B: Backend> {
    body: HostAsyncBody<B>,
}

impl<B: Backend> AsyncFunctionDeclaration<B> {
    pub(crate) fn new(body: HostAsyncBody<B>) -> Self {
        Self { body }
    }
}

impl<B> DeclareMember<B> for AsyncFunctionDeclaration<B>
where
    B: Backend
        + BackendValues
        + BackendCallables
        + BackendModules
        + BackendCoroutines
        + BackendExceptions,
{
    fn realise<'py>(
        &self,
        context: &DeclarationContext<'py, '_, B>,
        name: &str,
    ) -> Result<Val<'py, B>, Error> {
        let body = self.body.clone();

        B::function(
            context.enter().token(),
            name,
            None,
            context
                .enter()
                .guest()
                .raw_body(Rc::new(move |enter, args| {
                    enter
                        .guest()
                        .ensure_async_driver(enter)?
                        .driver()
                        .register_host_future(enter, body(enter, args)?)
                })),
        )
    }
}

pub struct HostFn<B: Backend>(Member<B>);

impl<B> HostFn<B>
where
    B: Backend + BackendValues + BackendCallables,
{
    pub fn new<F, R>(function: F) -> Self
    where
        F: for<'py> Fn(&Enter<'py, B>, Args<'py, B>) -> Result<R, Error> + 'static,
        R: ToGuest<B> + 'static,
    {
        Self(Rc::new(FunctionDeclaration::new(Rc::new(move |enter, args| {
            function(enter, args)?.to_guest(enter)
        }))))
    }
}

impl<B> HostFn<B>
where
    B: Backend
        + BackendValues
        + BackendCallables
        + BackendModules
        + BackendCoroutines
        + BackendExceptions,
{
    pub fn new_async<F, Fut, R>(function: F) -> Self
    where
        F: for<'py> Fn(&Enter<'py, B>, Args<'py, B>) -> Result<Fut, Error> + 'static,
        Fut: Future<Output = Result<R, Error>> + 'static,
        R: ToGuest<B> + 'static,
    {
        Self(Rc::new(AsyncFunctionDeclaration::new(Rc::new(move |enter, args| {
            Ok(PendingValue::<B, R>::into_host_future(function(enter, args)?))
        }))))
    }
}

impl<B: Backend> ToGuest<B> for HostFn<B> {
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        self.0
            .realise(&DeclarationContext::new(enter), "<host_fn>")
    }
}

#[cfg(test)]
mod tests {
    use super::HostFn;
    use crate::{backend::tests::Stub, errors::Error};

    #[allow(dead_code)]
    fn declaration() {
        let _ = HostFn::<Stub>::new(|_, _| Ok::<_, Error>(1));
        let _ = HostFn::<Stub>::new_async(|_, _| Ok::<_, Error>(async { Ok::<_, Error>(1) }));
    }
}
