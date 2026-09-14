use crate::{
    backend::{Backend, BackendCoroutines, BackendClasses, BackendModules},
    errors::Error,
    guest::Guest,
    driver::{AsyncStep, CoroutineFuture},
    scope::{Scope, Enter},
    marshal::{FromGuest, ToGuest}
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

impl<B: Backend + BackendCoroutines + BackendClasses + BackendModules> Handle<B> {
    pub(crate) fn with_async_step<R>(
        &self,
        f: impl for<'py> FnOnce(&Enter<'py, B>, &B::Value<'py>) -> Result<B::Value<'py>, Error>,
    ) -> AsyncStep<B, R>
    where
        R: FromGuest<B>,
    {
        AsyncStep::from(self.guest.enter(|enter| {
            f(enter, &B::attach(enter.token(), &self.owned))
                .map(|pending| CoroutineFuture::new(self.guest.clone(), B::detach(enter.token(), pending)))
        }))
    }
}

#[derive(Clone)]
pub struct Value<B: Backend> {
    owned: B::Owned,
}

impl<B: Backend> Value<B> {
    pub(crate) fn new(owned: B::Owned) -> Self {
        Self { owned }
    }

    pub fn as_type<T>(&self, scope: &Scope<'_, B>) -> Result<T::Owned, Error>
    where
        T: FromGuest<B>,
    {
        scope
            .guest()
            .enter(|enter| T::from_guest(enter, B::attach(enter.token(), &self.owned)))
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        B::owned_ptr_eq(&self.owned, &other.owned)
    }
}

impl<B: Backend> ToGuest<B> for Value<B> {
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        Ok(B::attach(enter.token(), &self.owned))
    }
}

impl<B: Backend> FromGuest<B> for Value<B> {
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        Ok(Self::new(B::detach(enter.token(), value)))
    }
}
