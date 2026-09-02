//! Guest object handles.

use crate::{
    backend::{
        Backend, BackendCallables, BackendCoroutines, BackendExceptions, BackendInterrupt,
        BackendModules, BackendValues,
    },
    errors::Error,
    handle::{AsyncIter, Handle, traits::HasHandle},
    marshal::{FromGuest, ToGuest},
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

impl<B> Object<B>
where
    B: Backend
        + BackendValues
        + BackendCallables
        + BackendModules
        + BackendCoroutines
        + BackendExceptions
        + BackendInterrupt,
{
    pub fn aiter<T: FromGuest<B>>(&self) -> Result<AsyncIter<B, T>, Error> {
        self.0.with_enter(|enter, object| {
            let aiter_method = B::get_attr(enter.token(), object, "__aiter__")?;
            let async_iterator = B::call(enter.token(), &aiter_method, &[], &[])?;

            Ok(AsyncIter::from_parts(
                B::detach(enter.token(), async_iterator),
                self.0.guest().clone(),
            ))
        })
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
