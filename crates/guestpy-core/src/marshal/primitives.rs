use crate::{
    backend::{Backend, BackendValues},
    errors::Error,
    marshal::{FromGuest, ToGuest},
    scope::Enter,
};

macro_rules! signed_int {
    ($($type:ty),* $(,)?) => {
        $(
            impl<B> ToGuest<B> for $type
            where
                B: Backend + BackendValues,
            {
                fn to_guest<'py>(
                    self,
                    enter: &Enter<'py, B>,
                ) -> Result<B::Value<'py>, Error> {
                    Ok(B::int(enter.token(), self as i64))
                }
            }

            impl<B> FromGuest<B> for $type
            where
                B: Backend + BackendValues,
            {
                type Owned = Self;

                fn from_guest<'py>(
                    enter: &Enter<'py, B>,
                    value: B::Value<'py>,
                ) -> Result<Self::Owned, Error> {
                    if !B::is_int(enter.token(), &value) {
                        return Err(Error::type_mismatch(
                            "int",
                            &B::type_name(enter.token(), &value),
                        ));
                    }

                    let value = B::as_i64(enter.token(), &value)?;

                    value
                        .try_into()
                        .map_err(|_| {
                            Error::conversion(format!(
                                "{} does not fit in {}",
                                value,
                                stringify!($type),
                            ))
                        })
                }
            }
        )*
    };
}

macro_rules! unsigned_int {
    ($($type:ty),* $(,)?) => {
        $(
            impl<B> ToGuest<B> for $type
            where
                B: Backend + BackendValues,
            {
                fn to_guest<'py>(
                    self,
                    enter: &Enter<'py, B>,
                ) -> Result<B::Value<'py>, Error> {
                    Ok(B::uint(enter.token(), self as u64))
                }
            }

            impl<B> FromGuest<B> for $type
            where
                B: Backend + BackendValues,
            {
                type Owned = Self;

                fn from_guest<'py>(
                    enter: &Enter<'py, B>,
                    value: B::Value<'py>,
                ) -> Result<Self::Owned, Error> {
                    if !B::is_int(enter.token(), &value) {
                        return Err(Error::type_mismatch(
                            "int",
                            &B::type_name(enter.token(), &value),
                        ));
                    }

                    B::as_u64(enter.token(), &value)
                        .map_err(|_| {
                            Error::conversion(
                                "a negative int cannot be converted to an unsigned integer",
                            )
                        })?
                        .try_into()
                        .map_err(|value| {
                            Error::conversion(format!(
                                "{value} does not fit in {}",
                                stringify!($type),
                            ))
                        })
                }
            }
        )*
    };
}

impl<B> ToGuest<B> for ()
where
    B: Backend + BackendValues,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        Ok(B::none(enter.token()))
    }
}

impl<B> FromGuest<B> for ()
where
    B: Backend + BackendValues,
{
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        if B::is_none(enter.token(), &value) {
            Ok(())
        } else {
            Err(Error::type_mismatch("None", &B::type_name(enter.token(), &value)))
        }
    }
}

impl<B> ToGuest<B> for bool
where
    B: Backend + BackendValues,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        Ok(B::bool(enter.token(), self))
    }
}

impl<B> FromGuest<B> for bool
where
    B: Backend + BackendValues,
{
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        if B::is_bool(enter.token(), &value) {
            B::as_bool(enter.token(), &value)
        } else {
            Err(Error::type_mismatch("bool", &B::type_name(enter.token(), &value)))
        }
    }
}

signed_int!(i8, i16, i32, i64, isize);
unsigned_int!(u8, u16, u32, u64, usize);

impl<B> ToGuest<B> for f32
where
    B: Backend + BackendValues,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        Ok(B::float(enter.token(), self.into()))
    }
}

impl<B> FromGuest<B> for f32
where
    B: Backend + BackendValues,
{
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        if B::is_float(enter.token(), &value) {
            return Ok(B::as_f64(enter.token(), &value)? as Self);
        }

        if B::is_int(enter.token(), &value) {
            return Ok(B::as_i64(enter.token(), &value)? as Self);
        }

        Err(Error::type_mismatch("float", &B::type_name(enter.token(), &value)))
    }
}

impl<B> ToGuest<B> for f64
where
    B: Backend + BackendValues,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        Ok(B::float(enter.token(), self))
    }
}

impl<B> FromGuest<B> for f64
where
    B: Backend + BackendValues,
{
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        if B::is_float(enter.token(), &value) {
            return B::as_f64(enter.token(), &value);
        }

        if B::is_int(enter.token(), &value) {
            return Ok(B::as_i64(enter.token(), &value)? as Self);
        }

        Err(Error::type_mismatch("float", &B::type_name(enter.token(), &value)))
    }
}

impl<B> ToGuest<B> for char
where
    B: Backend + BackendValues,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        Ok(B::str(enter.token(), &self.to_string()))
    }
}

impl<B> FromGuest<B> for char
where
    B: Backend + BackendValues,
{
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        if !B::is_str(enter.token(), &value) {
            return Err(Error::type_mismatch("str", &B::type_name(enter.token(), &value)));
        }

        let value = B::as_str(enter.token(), &value)?;

        if value.chars().count() != 1 {
            return Err(Error::conversion("expected a single-character str"));
        }

        Ok(value
            .chars()
            .next()
            .expect("single-character str"))
    }
}

impl<B> ToGuest<B> for String
where
    B: Backend + BackendValues,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        Ok(B::str(enter.token(), &self))
    }
}

impl<B> FromGuest<B> for String
where
    B: Backend + BackendValues,
{
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        if B::is_str(enter.token(), &value) {
            B::as_str(enter.token(), &value)
        } else {
            Err(Error::type_mismatch("str", &B::type_name(enter.token(), &value)))
        }
    }
}

impl<B> ToGuest<B> for &str
where
    B: Backend + BackendValues,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        Ok(B::str(enter.token(), self))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bytes(pub Vec<u8>);

impl<B> ToGuest<B> for Bytes
where
    B: Backend + BackendValues,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        Ok(B::bytes(enter.token(), &self.0))
    }
}

impl<B> FromGuest<B> for Bytes
where
    B: Backend + BackendValues,
{
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        if B::is_bytes(enter.token(), &value) {
            Ok(Self(B::as_bytes(enter.token(), &value)?))
        } else {
            Err(Error::type_mismatch("bytes", &B::type_name(enter.token(), &value)))
        }
    }
}

impl<B> ToGuest<B> for &[u8]
where
    B: Backend + BackendValues,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        Ok(B::bytes(enter.token(), self))
    }
}
