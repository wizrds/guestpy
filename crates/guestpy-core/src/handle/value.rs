//! Guest value handle types.

use crate::{
    backend::Backend,
    errors::Error,
    marshal::{FromGuest, ToGuest},
    scope::{Enter, Scope},
};

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
