use std::{any::type_name, marker::PhantomData, ops::Deref, rc::Rc};

use crate::{
    backend::{Backend, BackendValues},
    errors::Error,
    host::{
        context::{CallContext, FromContext, Requirement, Requirements},
        module::ModuleSpec,
    },
};

pub struct ModuleState<S>(Rc<S>);

impl<S> Clone for ModuleState<S> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<S> Deref for ModuleState<S> {
    type Target = S;

    fn deref(&self) -> &S {
        &self.0
    }
}

impl<B: Backend + BackendValues, S: 'static> FromContext<B> for ModuleState<S> {
    fn from_context<'py>(context: &CallContext<'py, '_, B>) -> Result<Self, Error> {
        let module = context.owning_module()?;

        module
            .state_of::<S>()
            .map(Self)
            .ok_or_else(|| ModuleStateRequirement::<S>::missing(&module))
    }

    fn declare(requirements: &mut Requirements<B>) {
        requirements.push(ModuleStateRequirement::<S>(PhantomData));
    }
}

struct ModuleStateRequirement<S>(PhantomData<fn() -> S>);

impl<S: 'static> ModuleStateRequirement<S> {
    fn missing<B: Backend + BackendValues>(module: &ModuleSpec<B>) -> Error {
        Error::unsupported(format!(
            "module {} has no state of type {}",
            module.name(),
            type_name::<S>(),
        ))
    }
}

impl<B: Backend + BackendValues, S: 'static> Requirement<B> for ModuleStateRequirement<S> {
    fn check(&self, module: &ModuleSpec<B>) -> Result<(), Error> {
        module
            .state_of::<S>()
            .map(|_| ())
            .ok_or_else(|| Self::missing(module))
    }
}
