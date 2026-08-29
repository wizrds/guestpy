//! Guest work scopes and engine entries.

use std::marker::PhantomData;

#[cfg(feature = "serde")]
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    backend::{Backend, BackendCallables, BackendModules, BackendValues},
    bundle::Bundle,
    errors::Error,
    guest::Guest,
    handle::{Module, Object},
    marshal::FromGuest,
};

#[cfg(feature = "serde")]
use crate::marshal::serde::{Deserializer, Serializer};

pub struct Scope<'a, B: Backend> {
    guest: &'a Guest<B>,
    _brand: PhantomData<&'a mut &'a ()>,
}

impl<'a, B: Backend> Scope<'a, B> {
    pub(crate) fn new(guest: &'a Guest<B>) -> Self {
        Self { guest, _brand: PhantomData }
    }

    pub fn guest(&self) -> &'a Guest<B> {
        self.guest
    }

    pub fn hold<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        B::enter(self.guest.engine(), |_| f())
    }
}

impl<'a, B> Scope<'a, B>
where
    B: Backend + BackendValues + BackendCallables + BackendModules,
{
    pub fn load(&self, bundle: &Bundle) -> Result<Module<B>, Error> {
        self.guest.load(bundle)
    }

    pub fn guest_module(&self, name: &str, source: &str) -> Result<Module<B>, Error> {
        self.guest.guest_module(name, source)
    }

    pub fn host_module(&self, name: &str) -> Result<Module<B>, Error> {
        self.guest.host_module(name)
    }

    pub fn exec(&self, source: &str) -> Result<(), Error> {
        self.guest.exec(source)
    }

    pub fn eval<T: FromGuest<B>>(&self, source: &str) -> Result<T::Owned, Error> {
        self.guest.eval::<T>(source)
    }

    pub fn globals(&self) -> Result<Object<B>, Error> {
        self.guest.globals()
    }

    pub fn import(&self, dotted: &str) -> Result<Module<B>, Error> {
        self.guest.import(dotted)
    }
}

pub struct Enter<'py, B: Backend> {
    token: B::Token<'py>,
    guest: Guest<B>,
}

impl<'py, B: Backend> Enter<'py, B> {
    pub(crate) fn new(token: B::Token<'py>, guest: Guest<B>) -> Self {
        Self { token, guest }
    }

    pub fn token(&self) -> B::Token<'py> {
        self.token
    }

    pub fn guest(&self) -> &Guest<B> {
        &self.guest
    }
}

#[cfg(feature = "serde")]
impl<'py, B> Enter<'py, B>
where
    B: Backend + BackendValues,
{
    pub fn to_value<T>(&self, value: &T) -> Result<B::Value<'py>, Error>
    where
        T: Serialize,
    {
        value.serialize(Serializer::<B>::new(self.token()))
    }

    pub fn from_value<T>(&self, value: B::Value<'py>) -> Result<T, Error>
    where
        T: DeserializeOwned,
    {
        T::deserialize(Deserializer::<B>::new(self.token(), value))
    }
}
