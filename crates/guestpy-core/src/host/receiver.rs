use crate::{
    backend::{Backend, BackendClasses, BackendValues},
    errors::Error,
    handle::{Ref, RefMut},
    host::class::HostClass,
    marshal::{FromGuest, FromGuestMut, FromGuestRef},
    scope::Enter,
};

/// Provides access to the guest value through which a host member was invoked.
pub struct Receiver<'a, 'py, B: Backend> {
    enter: &'a Enter<'py, B>,
    value: &'a B::Value<'py>,
}

impl<B: Backend> Clone for Receiver<'_, '_, B> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<B: Backend> Copy for Receiver<'_, '_, B> {}

impl<'a, 'py, B: Backend> Receiver<'a, 'py, B> {
    pub(crate) fn new(enter: &'a Enter<'py, B>, value: &'a B::Value<'py>) -> Self {
        Self { enter, value }
    }

    /// Returns the backend value for the receiver.
    pub fn value(&self) -> &'a B::Value<'py> {
        self.value
    }

    /// Resolves the receiver into an owned guest view.
    pub fn resolve<T: FromGuest<B>>(&self) -> Result<T::Owned, Error> {
        T::from_guest(self.enter, self.value.clone())
    }
}

impl<'a, 'py, B> Receiver<'a, 'py, B>
where
    B: Backend + BackendValues + BackendClasses,
{
    /// Borrows the host payload shared.
    pub fn payload<C: HostClass>(&self) -> Result<Ref<'a, B, C>, Error> {
        C::from_guest_ref(self.enter, self.value)
    }

    /// Borrows the host payload exclusively.
    pub fn payload_mut<C: HostClass>(&self) -> Result<RefMut<'a, B, C>, Error> {
        C::from_guest_mut(self.enter, self.value)
    }
}
