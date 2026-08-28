//! Guest module handles.

use crate::{
    backend::{Backend, BackendValues},
    errors::Error,
    handle::{Class, Function, Handle, Iter, Object, Value},
    marshal::{FromGuest, ToGuest, args::ToGuestArgs},
    scope::Enter,
};

pub struct Module<B: Backend>(Handle<B>);

impl<B: Backend> Clone for Module<B> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<B> Module<B>
where
    B: Backend + BackendValues,
{
    fn as_object(&self) -> Object<B> {
        Object::from_handle(self.0.clone())
    }

    pub fn get<T: FromGuest<B>>(&self, name: &str) -> Result<T::Owned, Error> {
        self.as_object().get::<T>(name)
    }

    pub fn set<T: ToGuest<B>>(&self, name: &str, value: T) -> Result<(), Error> {
        self.as_object().set::<T>(name, value)
    }

    pub fn delete(&self, name: &str) -> Result<(), Error> {
        self.as_object().delete(name)
    }

    pub fn has(&self, name: &str) -> Result<bool, Error> {
        self.as_object().has(name)
    }

    pub fn dir(&self) -> Result<Vec<String>, Error> {
        self.as_object().dir()
    }

    pub fn item<T, K>(&self, key: K) -> Result<T::Owned, Error>
    where
        T: FromGuest<B>,
        K: ToGuest<B>,
    {
        self.as_object().item::<T, K>(key)
    }

    pub fn set_item<K, T>(&self, key: K, value: T) -> Result<(), Error>
    where
        K: ToGuest<B>,
        T: ToGuest<B>,
    {
        self.as_object()
            .set_item::<K, T>(key, value)
    }

    pub fn del_item<K: ToGuest<B>>(&self, key: K) -> Result<(), Error> {
        self.as_object().del_item::<K>(key)
    }

    pub fn len(&self) -> Result<usize, Error> {
        self.as_object().len()
    }

    pub fn is_empty(&self) -> Result<bool, Error> {
        self.as_object().is_empty()
    }

    pub fn function(&self, name: &str) -> Result<Function<B>, Error> {
        self.as_object().function(name)
    }

    pub fn object(&self, name: &str) -> Result<Object<B>, Error> {
        self.as_object().object(name)
    }

    pub fn class(&self, name: &str) -> Result<Class<B>, Error> {
        self.as_object().class(name)
    }

    pub fn call<A, R>(&self, name: &str, args: A) -> Result<R::Owned, Error>
    where
        A: ToGuestArgs<B>,
        R: FromGuest<B>,
    {
        self.as_object()
            .call::<A, R>(name, args)
    }

    pub fn iter(&self) -> Result<Iter<B>, Error> {
        self.as_object().iter()
    }

    pub fn cast<T: FromGuest<B>>(&self) -> Result<T::Owned, Error> {
        self.as_object().cast::<T>()
    }

    pub fn type_name(&self) -> Result<String, Error> {
        self.as_object().type_name()
    }

    pub fn repr(&self) -> Result<String, Error> {
        self.as_object().repr()
    }

    pub fn str(&self) -> Result<String, Error> {
        self.as_object().str()
    }

    pub fn id(&self) -> Result<usize, Error> {
        self.as_object().id()
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        self.0.ptr_eq(&other.0)
    }

    pub fn value(&self) -> Value<B> {
        self.0.value()
    }
}

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
