//! Guest object handles.

use crate::{
    backend::{
        Backend, BackendCallables, BackendCoroutines, BackendExceptions, BackendInterrupt,
        BackendModules, BackendValues,
    },
    errors::Error,
    handle::{AsyncIter, Class, Function, Handle, Iter, Value},
    marshal::{FromGuest, ToGuest, args::ToGuestArgs},
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

    pub(crate) fn with_enter<R>(
        &self,
        f: impl for<'py> FnOnce(&Enter<'py, B>, &B::Value<'py>) -> Result<R, Error>,
    ) -> Result<R, Error> {
        self.0.with_enter(f)
    }
}

impl<B> Object<B>
where
    B: Backend + BackendValues,
{
    pub fn get<T: FromGuest<B>>(&self, name: &str) -> Result<T::Owned, Error> {
        self.0.with_enter(|enter, object| {
            T::from_guest(enter, B::get_attr(enter.token(), object, name)?)
        })
    }

    pub fn set<T: ToGuest<B>>(&self, name: &str, value: T) -> Result<(), Error> {
        self.0.with_enter(|enter, object| {
            B::set_attr(enter.token(), object, name, value.to_guest(enter)?)
        })
    }

    pub fn delete(&self, name: &str) -> Result<(), Error> {
        self.0
            .with_enter(|enter, object| B::del_attr(enter.token(), object, name))
    }

    pub fn has(&self, name: &str) -> Result<bool, Error> {
        self.0
            .with_enter(|enter, object| Ok(B::has_attr(enter.token(), object, name)))
    }

    pub fn dir(&self) -> Result<Vec<String>, Error> {
        self.0
            .with_enter(|enter, object| B::dir(enter.token(), object))
    }

    pub fn item<T, K>(&self, key: K) -> Result<T::Owned, Error>
    where
        T: FromGuest<B>,
        K: ToGuest<B>,
    {
        self.0.with_enter(|enter, object| {
            T::from_guest(enter, B::get_item(enter.token(), object, &key.to_guest(enter)?)?)
        })
    }

    pub fn set_item<K, T>(&self, key: K, value: T) -> Result<(), Error>
    where
        K: ToGuest<B>,
        T: ToGuest<B>,
    {
        self.0.with_enter(|enter, object| {
            B::set_item(enter.token(), object, key.to_guest(enter)?, value.to_guest(enter)?)
        })
    }

    pub fn del_item<K: ToGuest<B>>(&self, key: K) -> Result<(), Error> {
        self.0
            .with_enter(|enter, object| B::del_item(enter.token(), object, &key.to_guest(enter)?))
    }

    pub fn len(&self) -> Result<usize, Error> {
        self.0
            .with_enter(|enter, object| B::len(enter.token(), object))
    }

    pub fn is_empty(&self) -> Result<bool, Error> {
        Ok(self.len()? == 0)
    }

    pub fn function(&self, name: &str) -> Result<Function<B>, Error> {
        self.0.with_enter(|enter, object| {
            let callable = B::get_attr(enter.token(), object, name)?;

            if !B::is_callable(enter.token(), &callable) {
                return Err(Error::type_mismatch(
                    "callable",
                    &B::type_name(enter.token(), &callable),
                ));
            }

            Ok(Function::from_handle(Handle::new(
                B::detach(enter.token(), callable),
                self.0.guest().clone(),
            )))
        })
    }

    pub fn object(&self, name: &str) -> Result<Self, Error> {
        self.0.with_enter(|enter, object| {
            Ok(Self::from_handle(Handle::new(
                B::detach(enter.token(), B::get_attr(enter.token(), object, name)?),
                self.0.guest().clone(),
            )))
        })
    }

    pub fn class(&self, name: &str) -> Result<Class<B>, Error> {
        self.get::<Class<B>>(name)
    }

    pub fn call<A, R>(&self, name: &str, args: A) -> Result<R::Owned, Error>
    where
        A: ToGuestArgs<B>,
        R: FromGuest<B>,
    {
        self.0.with_enter(|enter, object| {
            R::from_guest(
                enter,
                B::call(
                    enter.token(),
                    &B::get_attr(enter.token(), object, name)?,
                    &args.into_args(enter)?,
                    &[],
                )?,
            )
        })
    }

    pub fn iter(&self) -> Result<Iter<B>, Error> {
        self.0.with_enter(|enter, object| {
            Ok(Iter::from_handle(Handle::new(
                B::detach(enter.token(), B::iter(enter.token(), object)?),
                self.0.guest().clone(),
            )))
        })
    }

    pub fn cast<T: FromGuest<B>>(&self) -> Result<T::Owned, Error> {
        self.0
            .with_enter(|enter, object| T::from_guest(enter, object.clone()))
    }

    pub fn is_none(&self) -> Result<bool, Error> {
        self.0
            .with_enter(|enter, object| Ok(B::is_none(enter.token(), object)))
    }

    pub fn type_name(&self) -> Result<String, Error> {
        self.0
            .with_enter(|enter, object| Ok(B::type_name(enter.token(), object)))
    }

    pub fn repr(&self) -> Result<String, Error> {
        self.0
            .with_enter(|enter, object| B::repr(enter.token(), object))
    }

    pub fn str(&self) -> Result<String, Error> {
        self.0
            .with_enter(|enter, object| B::display(enter.token(), object))
    }

    pub fn id(&self) -> Result<usize, Error> {
        self.0
            .with_enter(|enter, object| Ok(B::identity(enter.token(), object)))
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        self.0.ptr_eq(&other.0)
    }

    pub fn value(&self) -> Value<B> {
        self.0.value()
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
        Ok(Self(Handle::from_value(enter, value)))
    }
}

impl<B: Backend> ToGuest<B> for Object<B> {
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        Ok(B::attach(enter.token(), self.0.owned()))
    }
}
