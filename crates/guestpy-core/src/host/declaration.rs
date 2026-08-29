use std::rc::Rc;

use crate::{
    backend::{Backend, BackendCallables, BackendValues, Val, callables::RawBody},
    errors::Error,
    host::class::MethodBody,
    scope::Enter,
};

pub(crate) type ModuleGetter<B> =
    Rc<dyn for<'py, 'e> Fn(&DeclarationContext<'py, 'e, B>) -> Result<Val<'py, B>, Error>>;

pub(crate) trait DeclareMember<B: Backend> {
    fn realise<'py>(
        &self,
        context: &DeclarationContext<'py, '_, B>,
        name: &str,
    ) -> Result<Val<'py, B>, Error>;

    fn module_getter(&self) -> Option<&ModuleGetter<B>> {
        None
    }
}

pub(crate) type Member<B> = Rc<dyn DeclareMember<B>>;

pub(crate) struct DeclarationContext<'py, 'e, B: Backend> {
    enter: &'e Enter<'py, B>,
}

impl<'py, 'e, B: Backend> DeclarationContext<'py, 'e, B> {
    pub(crate) fn new(enter: &'e Enter<'py, B>) -> Self {
        Self { enter }
    }

    pub(crate) fn enter(&self) -> &'e Enter<'py, B> {
        self.enter
    }
}

impl<'py, 'e, B> DeclarationContext<'py, 'e, B>
where
    B: Backend + BackendValues,
{
    pub(crate) fn builtin(&self, name: &str) -> Result<Val<'py, B>, Error> {
        B::get_item(
            self.enter.token(),
            &B::context_builtins(self.enter.token(), self.enter.guest().context()),
            &B::str(self.enter.token(), name),
        )
    }

    pub(crate) fn wrap_builtin(
        &self,
        name: &str,
        value: Val<'py, B>,
    ) -> Result<Val<'py, B>, Error> {
        B::call(self.enter.token(), &self.builtin(name)?, &[value], &[])
    }
}

impl<'py, 'e, B> DeclarationContext<'py, 'e, B>
where
    B: Backend + BackendValues + BackendCallables,
{
    pub(crate) fn method_raw_body(&self, body: MethodBody<B>) -> RawBody<B> {
        self.enter()
            .guest()
            .raw_body(Rc::new(move |enter, args| {
                let (receiver, args) = args.split_receiver()?;

                body(enter, receiver, args)
            }))
    }

    pub(crate) fn property(
        &self,
        getter: Option<Val<'py, B>>,
        setter: Option<Val<'py, B>>,
        deleter: Option<Val<'py, B>>,
    ) -> Result<Val<'py, B>, Error> {
        B::call(
            self.enter.token(),
            &self.builtin("property")?,
            &[
                getter.unwrap_or_else(|| B::none(self.enter.token())),
                setter.unwrap_or_else(|| B::none(self.enter.token())),
                deleter.unwrap_or_else(|| B::none(self.enter.token())),
            ],
            &[],
        )
    }
}
