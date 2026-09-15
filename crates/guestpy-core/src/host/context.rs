use std::{any::TypeId, rc::Rc};

use crate::{
    backend::Backend,
    errors::Error,
    host::{class::HostClass, module::ModuleSpec},
    scope::Enter,
};

#[derive(Clone, Copy)]
pub enum Owner<'a> {
    Class(TypeId),
    Module(&'a str),
}

pub struct CallContext<'py, 'a, B: Backend> {
    enter: &'a Enter<'py, B>,
    owner: Owner<'a>,
}

impl<'py, 'a, B: Backend> CallContext<'py, 'a, B> {
    pub fn class<C: HostClass>(enter: &'a Enter<'py, B>) -> Self {
        Self {
            enter,
            owner: Owner::Class(TypeId::of::<C>()),
        }
    }

    pub fn module(enter: &'a Enter<'py, B>, name: &'a str) -> Self {
        Self { enter, owner: Owner::Module(name) }
    }

    pub fn enter(&self) -> &'a Enter<'py, B> {
        self.enter
    }

    pub fn resolve<T: FromContext<B>>(&self) -> Result<T, Error> {
        T::from_context(self)
    }

    pub(crate) fn owning_module(&self) -> Result<Rc<ModuleSpec<B>>, Error> {
        let bindings = self.enter.guest().bindings();

        match self.owner {
            Owner::Class(payload) => bindings.class_owner(payload),
            Owner::Module(name) => bindings.spec(name),
        }
        .ok_or_else(|| Error::unexpected("the owner of a host callable is not registered"))
    }
}

pub trait FromContext<B: Backend>: Sized {
    fn from_context<'py>(context: &CallContext<'py, '_, B>) -> Result<Self, Error>;

    fn declare(_requirements: &mut Requirements<B>) {}
}

pub trait Requirement<B: Backend> {
    fn check(&self, module: &ModuleSpec<B>) -> Result<(), Error>;
}

pub struct Requirements<B: Backend> {
    entries: Vec<Rc<dyn Requirement<B>>>,
}

impl<B: Backend> Requirements<B> {
    pub(crate) fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub fn push<R: Requirement<B> + 'static>(&mut self, requirement: R) {
        self.entries.push(Rc::new(requirement));
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn check(&self, module: &ModuleSpec<B>) -> Result<(), Error> {
        self.entries
            .iter()
            .try_for_each(|requirement| requirement.check(module))
    }
}
