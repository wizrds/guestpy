//! Guest function handles.

use crate::{
    backend::{Backend, BackendValues},
    errors::Error,
    handle::{
        Handle,
        traits::{Annotated, HasHandle, Named},
    },
    marshal::{FromGuest, ToGuest},
    scope::Enter,
};

pub struct Function<B: Backend>(Handle<B>);

impl<B: Backend> Clone for Function<B> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<B: Backend> Function<B> {
    pub(crate) fn from_handle(handle: Handle<B>) -> Self {
        Self(handle)
    }
}

impl<B: Backend> HasHandle<B> for Function<B> {
    fn handle(&self) -> &Handle<B> {
        &self.0
    }
}

impl<B> Named<B> for Function<B> where B: Backend + BackendValues {}

impl<B> Annotated<B> for Function<B> where B: Backend + BackendValues {}

impl<B> Function<B>
where
    B: Backend + BackendValues,
{
    pub fn is_coroutine_function(&self) -> Result<bool, Error> {
        self.0.with_enter(|enter, function| {
            Ok(B::as_i64(
                enter.token(),
                &B::get_attr(
                    enter.token(),
                    &B::get_attr(enter.token(), function, "__code__")?,
                    "co_flags",
                )?,
            )? & 0x80
                != 0)
        })
    }
}

impl<B> FromGuest<B> for Function<B>
where
    B: Backend + BackendValues,
{
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        if !B::is_callable(enter.token(), &value) {
            return Err(Error::type_mismatch("callable", &B::type_name(enter.token(), &value)));
        }

        Ok(Self::from_handle(Handle::from_value(enter, value)))
    }
}

impl<B: Backend> ToGuest<B> for Function<B> {
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        Ok(B::attach(enter.token(), self.0.owned()))
    }
}
