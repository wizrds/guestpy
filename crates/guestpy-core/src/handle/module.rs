//! Guest module handles.

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

pub struct Module<B: Backend>(Handle<B>);

impl<B: Backend> Clone for Module<B> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<B: Backend> Module<B> {
    pub fn ptr_eq(&self, other: &Self) -> bool {
        self.0.ptr_eq(&other.0)
    }
}

impl<B: Backend> HasHandle<B> for Module<B> {
    fn handle(&self) -> &Handle<B> {
        &self.0
    }
}

impl<B> Named<B> for Module<B> where B: Backend + BackendValues {}

impl<B> Annotated<B> for Module<B> where B: Backend + BackendValues {}

impl<B> FromGuest<B> for Module<B>
where
    B: Backend + BackendValues,
{
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        if B::type_name(enter.token(), &value) != "module" {
            return Err(Error::type_mismatch("module", &B::type_name(enter.token(), &value)));
        }

        Ok(Self(Handle::from_value(enter, value)))
    }
}

impl<B: Backend> ToGuest<B> for Module<B> {
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        Ok(B::attach(enter.token(), self.0.owned()))
    }
}
