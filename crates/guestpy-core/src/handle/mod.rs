//! Guest handle types.

mod base;
mod class;
mod coroutine;
mod function;
mod generator;
mod iter;
mod module;
mod object;
mod traits;

pub use self::{
    base::{Handle, Value},
    class::{Class, Instance, Ref, RefMut},
    coroutine::{Awaitable, Coroutine},
    function::Function,
    generator::{AsyncGenerator, Generator},
    iter::{AsyncIter, AsyncIterable, Iter},
    module::Module,
    object::Object,
    traits::{Annotated, GenericAlias, Named, ObjectProtocol, TypeProtocol},
};

#[cfg(test)]
mod tests {
    use super::{
        AsyncGenerator, AsyncIter, AsyncIterable, Class, Function, Generator, Instance, Iter,
        Module, Object, Value,
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
        escapes::<AsyncIterable<Stub, Value<Stub>>>();
        escapes::<AsyncGenerator<Stub, Value<Stub>>>();
    }
}
