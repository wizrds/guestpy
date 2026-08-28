use std::rc::Rc;

use crate::{
    backend::{Backend, BackendExceptions, BackendValues, Val},
    errors::Error,
    host::declaration::{DeclarationContext, DeclareMember},
    scope::Enter,
};

struct ExceptionRealiser<'py, 'e, B: Backend> {
    enter: &'e Enter<'py, B>,
}

impl<'py, 'e, B> ExceptionRealiser<'py, 'e, B>
where
    B: Backend + BackendValues + BackendExceptions,
{
    fn new(enter: &'e Enter<'py, B>) -> Self {
        Self { enter }
    }

    fn realise(&self, spec: &Rc<ExceptionSpec>) -> Result<Val<'py, B>, Error> {
        let realisation = self.enter.guest().realisation();

        if !realisation.exception_registered(spec.module(), spec.name()) {
            return Err(Error::unexpected("host exception was not registered"));
        }

        if let Some(owned) = realisation.realised_exception(spec.module(), spec.name()) {
            return Ok(B::attach(self.enter.token(), &owned));
        }

        let base = match spec.base() {
            ExceptionBase::Exception => B::exception_class(self.enter.token(), "Exception")?,
            ExceptionBase::Builtin(name) => B::exception_class(self.enter.token(), name)?,
            ExceptionBase::Named(name) => {
                let sibling = realisation
                    .exception_spec(spec.module(), name)
                    .ok_or_else(|| {
                        Error::unexpected(format!(
                            "exception {name} is not declared in module {}",
                            spec.module(),
                        ))
                    })?;

                self.realise(&sibling)?
            }
        };
        let class =
            B::new_exception_class(self.enter.token(), spec.module(), spec.name(), Some(&base))?;

        realisation.set_realised_exception(
            spec.module(),
            spec.name(),
            B::detach(self.enter.token(), class.clone()),
        );

        Ok(class)
    }
}

pub(crate) struct ExceptionDeclaration {
    spec: Rc<ExceptionSpec>,
}

impl ExceptionDeclaration {
    pub(crate) fn new(spec: Rc<ExceptionSpec>) -> Self {
        Self { spec }
    }
}

impl<B> DeclareMember<B> for ExceptionDeclaration
where
    B: Backend + BackendValues + BackendExceptions,
{
    fn realise<'py>(
        &self,
        context: &DeclarationContext<'py, '_, B>,
        _name: &str,
    ) -> Result<Val<'py, B>, Error> {
        ExceptionRealiser::new(context.enter()).realise(&self.spec)
    }
}

#[derive(Clone)]
pub enum ExceptionBase {
    Exception,
    Builtin(&'static str),
    Named(&'static str),
}

pub struct ExceptionSpec {
    module: String,
    name: String,
    base: ExceptionBase,
}

impl ExceptionSpec {
    pub(crate) fn new(
        module: impl Into<String>,
        name: impl Into<String>,
        base: ExceptionBase,
    ) -> Self {
        Self {
            module: module.into(),
            name: name.into(),
            base,
        }
    }

    pub(crate) fn module(&self) -> &str {
        &self.module
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn base(&self) -> &ExceptionBase {
        &self.base
    }
}
