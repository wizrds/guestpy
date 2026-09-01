//! Guest handle types.

mod traits;
mod class;
mod coroutine;
mod function;
mod generator;
mod iter;
mod module;
mod object;
mod value;

use crate::{backend::Backend, errors::Error, guest::Guest, scope::Enter};

pub use self::{
    traits::{Annotated, GenericAlias, Named, ObjectProtocol, TypeProtocol},
    class::{Class, Instance, Ref, RefMut},
    coroutine::{Awaitable, Coroutine},
    function::Function,
    generator::{AsyncGenerator, Generator},
    iter::{AsyncIter, Iter},
    module::Module,
    object::Object,
    value::Value,
};

pub struct Handle<B: Backend> {
    owned: B::Owned,
    guest: Guest<B>,
}

impl<B: Backend> Clone for Handle<B> {
    fn clone(&self) -> Self {
        Self {
            owned: self.owned.clone(),
            guest: self.guest.clone(),
        }
    }
}

impl<B: Backend> Handle<B> {
    pub fn new(owned: B::Owned, guest: Guest<B>) -> Self {
        Self { owned, guest }
    }

    pub fn from_value<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Self {
        Self::new(B::detach(enter.token(), value), enter.guest().clone())
    }

    pub fn owned(&self) -> &B::Owned {
        &self.owned
    }

    pub fn guest(&self) -> &Guest<B> {
        &self.guest
    }

    pub fn value(&self) -> Value<B> {
        Value::new(self.owned.clone())
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        B::owned_ptr_eq(&self.owned, &other.owned)
    }

    pub fn with_enter<R>(
        &self,
        f: impl for<'py> FnOnce(&Enter<'py, B>, &B::Value<'py>) -> Result<R, Error>,
    ) -> Result<R, Error> {
        self.guest
            .enter(|enter| f(enter, &B::attach(enter.token(), &self.owned)))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AsyncGenerator, AsyncIter, Class, Function, Generator, Instance, Iter, Module, Object,
        Value,
    };
    use crate::backend::tests::Stub;

    #[allow(dead_code)]
    fn handle_types_outlive_a_scope() {
        fn escapes<T: 'static>() {}

        escapes::<Value<Stub>>();
        escapes::<Object<Stub>>();
        escapes::<Class<Stub>>();
        escapes::<Instance<Stub>>();
        escapes::<Function<Stub>>();
        escapes::<Module<Stub>>();
        escapes::<Iter<Stub>>();
        escapes::<Generator<Stub>>();
        escapes::<AsyncIter<Stub, Value<Stub>>>();
        escapes::<AsyncGenerator<Stub, Value<Stub>>>();
    }
}
