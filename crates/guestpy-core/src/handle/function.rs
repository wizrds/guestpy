//! Guest function handles.

use crate::{
    backend::{Backend, BackendValues},
    errors::Error,
    handle::{Handle, Value},
    marshal::{
        FromGuest, ToGuest,
        args::{ToGuestArgs, ToGuestKwargs},
    },
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

impl<B> Function<B>
where
    B: Backend + BackendValues,
{
    pub fn call<A, R>(&self, args: A) -> Result<R::Owned, Error>
    where
        A: ToGuestArgs<B>,
        R: FromGuest<B>,
    {
        self.call_with::<A, (), R>(args, ())
    }

    pub fn call_with<A, K, R>(&self, args: A, kwargs: K) -> Result<R::Owned, Error>
    where
        A: ToGuestArgs<B>,
        K: ToGuestKwargs<B>,
        R: FromGuest<B>,
    {
        self.0.with_enter(|enter, function| {
            let kwargs = kwargs.into_kwargs(enter)?;

            R::from_guest(
                enter,
                B::call(
                    enter.token(),
                    function,
                    &args.into_args(enter)?,
                    &kwargs
                        .iter()
                        .map(|(name, value)| (name.as_str(), value.clone()))
                        .collect::<Vec<_>>(),
                )?,
            )
        })
    }

    pub fn name(&self) -> Result<String, Error> {
        self.0.with_enter(|enter, function| {
            B::as_str(enter.token(), &B::get_attr(enter.token(), function, "__name__")?)
        })
    }

    pub fn doc(&self) -> Result<Option<String>, Error> {
        self.0.with_enter(|enter, function| {
            let value = B::get_attr(enter.token(), function, "__doc__")?;

            if B::is_none(enter.token(), &value) {
                Ok(None)
            } else {
                Ok(Some(B::as_str(enter.token(), &value)?))
            }
        })
    }

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

    pub fn value(&self) -> Value<B> {
        self.0.value()
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

        Ok(Self(Handle::from_value(enter, value)))
    }
}

impl<B: Backend> ToGuest<B> for Function<B> {
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        Ok(B::attach(enter.token(), self.0.owned()))
    }
}
