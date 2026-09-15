//! Guest object handles.

use crate::{
    backend::Backend,
    errors::Error,
    handle::{base::Handle, traits::HasHandle},
    marshal::{
        FromGuest, ToGuest,
        describe::{Describe, Expected},
    },
    scope::Enter,
};

pub struct Object<B: Backend>(Handle<B>);

impl<B: Backend> Clone for Object<B> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<B: Backend> Object<B> {
    pub(crate) fn from_handle(handle: Handle<B>) -> Self {
        Self(handle)
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        self.0.ptr_eq(&other.0)
    }
}

impl<B: Backend> HasHandle<B> for Object<B> {
    fn handle(&self) -> &Handle<B> {
        &self.0
    }
}

impl<B: Backend> Describe for Object<B> {
    fn describe(expected: &mut Expected) {
        expected.push("object");
    }
}

impl<B: Backend> FromGuest<B> for Object<B> {
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        Ok(Self::from_handle(Handle::from_value(enter, value)))
    }
}

impl<B: Backend> ToGuest<B> for Object<B> {
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        Ok(B::attach(enter.token(), self.0.owned()))
    }
}
