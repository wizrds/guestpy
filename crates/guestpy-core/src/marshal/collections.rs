use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::{
    backend::{Backend, values::BackendValues},
    errors::Error,
    marshal::{FromGuest, ToGuest},
    scope::Enter,
};

macro_rules! tuple {
    ($($type:ident:$index:tt),+ $(,)?) => {
        impl<B, $($type),+> ToGuest<B> for ($($type,)+)
        where
            B: Backend + BackendValues,
            $($type: ToGuest<B>,)+
        {
            fn to_guest<'py>(
                self,
                enter: &Enter<'py, B>,
            ) -> Result<B::Value<'py>, Error> {
                B::tuple(
                    enter.token(),
                    vec![$(self.$index.to_guest(enter)?,)+],
                )
            }
        }

        impl<B, $($type),+> FromGuest<B> for ($($type,)+)
        where
            B: Backend + BackendValues,
            $($type: FromGuest<B>,)+
        {
            type Owned = ($($type::Owned,)+);

            fn from_guest<'py>(
                enter: &Enter<'py, B>,
                value: B::Value<'py>,
            ) -> Result<Self::Owned, Error> {
                if !B::is_tuple(enter.token(), &value) {
                    return Err(Error::type_mismatch(
                        "tuple",
                        &B::type_name(enter.token(), &value),
                    ));
                }

                if B::len(enter.token(), &value)? != tuple!(@count $($type)+) {
                    return Err(Error::conversion("tuple has an unexpected length"));
                }

                Ok((
                    $(
                        <$type as FromGuest<B>>::from_guest(
                            enter,
                            B::get_item(
                                enter.token(),
                                &value,
                                &B::int(enter.token(), $index),
                            )?,
                        )?,
                    )+
                ))
            }
        }

    };
    (@count $($type:ident)+) => {
        <[()]>::len(&[$(tuple!(@replace $type ()),)+])
    };
    (@replace $_type:ident $value:expr) => {
        $value
    };
}

impl<B, T> ToGuest<B> for Vec<T>
where
    B: Backend + BackendValues,
    T: ToGuest<B>,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        B::list(
            enter.token(),
            self.into_iter()
                .map(|value| value.to_guest(enter))
                .collect::<Result<Vec<_>, _>>()?,
        )
    }
}

impl<B, T> FromGuest<B> for Vec<T>
where
    B: Backend + BackendValues,
    T: FromGuest<B>,
{
    type Owned = Vec<T::Owned>;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        if !B::is_list(enter.token(), &value) {
            return Err(Error::type_mismatch("list", &B::type_name(enter.token(), &value)));
        }

        let iterator = B::iter(enter.token(), &value)?;
        let mut values = Vec::new();

        while let Some(value) = B::next(enter.token(), &iterator)? {
            values.push(T::from_guest(enter, value)?);
        }

        Ok(values)
    }
}

impl<B, T, const N: usize> ToGuest<B> for [T; N]
where
    B: Backend + BackendValues,
    T: ToGuest<B>,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        B::list(
            enter.token(),
            self.into_iter()
                .map(|value| value.to_guest(enter))
                .collect::<Result<Vec<_>, _>>()?,
        )
    }
}

impl<B, T, const N: usize> FromGuest<B> for [T; N]
where
    B: Backend + BackendValues,
    T: FromGuest<B>,
{
    type Owned = [T::Owned; N];

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        if !B::is_list(enter.token(), &value) {
            return Err(Error::type_mismatch("list", &B::type_name(enter.token(), &value)));
        }

        if B::len(enter.token(), &value)? != N {
            return Err(Error::conversion(format!("expected a list of length {N}")));
        }

        Vec::<T>::from_guest(enter, value)?
            .try_into()
            .map_err(|_| Error::conversion(format!("expected a list of length {N}")))
    }
}

tuple!(A1: 0);
tuple!(A1: 0, A2: 1);
tuple!(A1: 0, A2: 1, A3: 2);
tuple!(A1: 0, A2: 1, A3: 2, A4: 3);
tuple!(A1: 0, A2: 1, A3: 2, A4: 3, A5: 4);
tuple!(A1: 0, A2: 1, A3: 2, A4: 3, A5: 4, A6: 5);
tuple!(A1: 0, A2: 1, A3: 2, A4: 3, A5: 4, A6: 5, A7: 6);
tuple!(A1: 0, A2: 1, A3: 2, A4: 3, A5: 4, A6: 5, A7: 6, A8: 7);
tuple!(
    A1: 0,
    A2: 1,
    A3: 2,
    A4: 3,
    A5: 4,
    A6: 5,
    A7: 6,
    A8: 7,
    A9: 8,
);
tuple!(
    A1: 0,
    A2: 1,
    A3: 2,
    A4: 3,
    A5: 4,
    A6: 5,
    A7: 6,
    A8: 7,
    A9: 8,
    A10: 9,
);
tuple!(
    A1: 0,
    A2: 1,
    A3: 2,
    A4: 3,
    A5: 4,
    A6: 5,
    A7: 6,
    A8: 7,
    A9: 8,
    A10: 9,
    A11: 10,
);
tuple!(
    A1: 0,
    A2: 1,
    A3: 2,
    A4: 3,
    A5: 4,
    A6: 5,
    A7: 6,
    A8: 7,
    A9: 8,
    A10: 9,
    A11: 10,
    A12: 11,
);

impl<B, K, V> ToGuest<B> for HashMap<K, V>
where
    B: Backend + BackendValues,
    K: ToGuest<B>,
    V: ToGuest<B>,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        B::dict(
            enter.token(),
            self.into_iter()
                .map(|(key, value)| Ok((key.to_guest(enter)?, value.to_guest(enter)?)))
                .collect::<Result<Vec<_>, Error>>()?,
        )
    }
}

impl<B, K, V> FromGuest<B> for HashMap<K, V>
where
    B: Backend + BackendValues,
    K: FromGuest<B>,
    K::Owned: Eq + std::hash::Hash,
    V: FromGuest<B>,
{
    type Owned = HashMap<K::Owned, V::Owned>;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        if !B::is_dict(enter.token(), &value) {
            return Err(Error::type_mismatch("dict", &B::type_name(enter.token(), &value)));
        }

        let iterator = B::iter(enter.token(), &value)?;
        let mut entries = HashMap::new();

        while let Some(key) = B::next(enter.token(), &iterator)? {
            entries.insert(
                K::from_guest(enter, key.clone())?,
                V::from_guest(enter, B::get_item(enter.token(), &value, &key)?)?,
            );
        }

        Ok(entries)
    }
}

impl<B, K, V> ToGuest<B> for BTreeMap<K, V>
where
    B: Backend + BackendValues,
    K: ToGuest<B>,
    V: ToGuest<B>,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        B::dict(
            enter.token(),
            self.into_iter()
                .map(|(key, value)| Ok((key.to_guest(enter)?, value.to_guest(enter)?)))
                .collect::<Result<Vec<_>, Error>>()?,
        )
    }
}

impl<B, K, V> FromGuest<B> for BTreeMap<K, V>
where
    B: Backend + BackendValues,
    K: FromGuest<B>,
    K::Owned: Ord,
    V: FromGuest<B>,
{
    type Owned = BTreeMap<K::Owned, V::Owned>;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        if !B::is_dict(enter.token(), &value) {
            return Err(Error::type_mismatch("dict", &B::type_name(enter.token(), &value)));
        }

        let iterator = B::iter(enter.token(), &value)?;
        let mut entries = BTreeMap::new();

        while let Some(key) = B::next(enter.token(), &iterator)? {
            entries.insert(
                K::from_guest(enter, key.clone())?,
                V::from_guest(enter, B::get_item(enter.token(), &value, &key)?)?,
            );
        }

        Ok(entries)
    }
}

impl<B, T> ToGuest<B> for HashSet<T>
where
    B: Backend + BackendValues,
    T: ToGuest<B>,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        B::set(
            enter.token(),
            self.into_iter()
                .map(|value| value.to_guest(enter))
                .collect::<Result<Vec<_>, _>>()?,
        )
    }
}

impl<B, T> FromGuest<B> for HashSet<T>
where
    B: Backend + BackendValues,
    T: FromGuest<B>,
    T::Owned: Eq + std::hash::Hash,
{
    type Owned = HashSet<T::Owned>;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        if !B::is_set(enter.token(), &value) {
            return Err(Error::type_mismatch("set", &B::type_name(enter.token(), &value)));
        }

        let iterator = B::iter(enter.token(), &value)?;
        let mut values = HashSet::new();

        while let Some(value) = B::next(enter.token(), &iterator)? {
            values.insert(T::from_guest(enter, value)?);
        }

        Ok(values)
    }
}

impl<B, T> ToGuest<B> for BTreeSet<T>
where
    B: Backend + BackendValues,
    T: ToGuest<B>,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        B::set(
            enter.token(),
            self.into_iter()
                .map(|value| value.to_guest(enter))
                .collect::<Result<Vec<_>, _>>()?,
        )
    }
}

impl<B, T> FromGuest<B> for BTreeSet<T>
where
    B: Backend + BackendValues,
    T: FromGuest<B>,
    T::Owned: Ord,
{
    type Owned = BTreeSet<T::Owned>;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        if !B::is_set(enter.token(), &value) {
            return Err(Error::type_mismatch("set", &B::type_name(enter.token(), &value)));
        }

        let iterator = B::iter(enter.token(), &value)?;
        let mut values = BTreeSet::new();

        while let Some(value) = B::next(enter.token(), &iterator)? {
            values.insert(T::from_guest(enter, value)?);
        }

        Ok(values)
    }
}

impl<B, T> ToGuest<B> for Option<T>
where
    B: Backend + BackendValues,
    T: ToGuest<B>,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        match self {
            Some(value) => value.to_guest(enter),
            None => Ok(B::none(enter.token())),
        }
    }
}

impl<B, T> FromGuest<B> for Option<T>
where
    B: Backend + BackendValues,
    T: FromGuest<B>,
{
    type Owned = Option<T::Owned>;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        if B::is_none(enter.token(), &value) {
            Ok(None)
        } else {
            Ok(Some(T::from_guest(enter, value)?))
        }
    }
}

pub struct Iterable<T>(pub T);

impl<B, T> FromGuest<B> for Iterable<Vec<T>>
where
    B: Backend + BackendValues,
    T: FromGuest<B, Owned = T> + 'static,
{
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        let iterator = B::iter(enter.token(), &value)?;
        let mut values = Vec::new();

        while let Some(value) = B::next(enter.token(), &iterator)? {
            values.push(T::from_guest(enter, value)?);
        }

        Ok(Self(values))
    }
}
