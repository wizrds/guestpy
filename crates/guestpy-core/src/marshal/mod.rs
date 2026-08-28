pub mod args;
pub mod collections;
pub mod primitives;

#[cfg(feature = "serde")]
pub mod serde;

use crate::{backend::Backend, errors::Error, scope::Enter};

pub trait ToGuest<B: Backend> {
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error>;
}

pub trait FromGuest<B: Backend> {
    type Owned: 'static;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error>;
}

pub trait FromGuestRef<'py, B: Backend> {
    type Ref<'a>
    where
        Self: 'a;

    fn from_guest_ref<'a>(
        enter: &Enter<'py, B>,
        value: &'a B::Value<'py>,
    ) -> Result<Self::Ref<'a>, Error>;
}

pub trait FromGuestMut<'py, B: Backend> {
    type Mut<'a>
    where
        Self: 'a;

    fn from_guest_mut<'a>(
        enter: &Enter<'py, B>,
        value: &'a B::Value<'py>,
    ) -> Result<Self::Mut<'a>, Error>;
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
    use bytes::Bytes;

    use super::{
        FromGuest, ToGuest,
        args::{ToGuestArgs, ToGuestKwargs},
        collections::Iterable,
    };
    use crate::{
        backend::tests::Stub,
        handle::{Class, Function, Instance, Module, Object, Value},
    };

    #[allow(dead_code)]
    fn conversions() {
        fn to<T: ToGuest<Stub>>() {}
        fn from<T: FromGuest<Stub>>() {}
        fn args<T: ToGuestArgs<Stub>>() {}
        fn kwargs<T: ToGuestKwargs<Stub>>() {}

        to::<()>();
        from::<()>();
        to::<bool>();
        from::<bool>();
        to::<i8>();
        from::<i8>();
        to::<i16>();
        from::<i16>();
        to::<i32>();
        from::<i32>();
        to::<i64>();
        from::<i64>();
        to::<isize>();
        from::<isize>();
        to::<u8>();
        from::<u8>();
        to::<u16>();
        from::<u16>();
        to::<u32>();
        from::<u32>();
        to::<u64>();
        from::<u64>();
        to::<usize>();
        from::<usize>();
        to::<f32>();
        from::<f32>();
        to::<f64>();
        from::<f64>();
        to::<char>();
        from::<char>();
        to::<String>();
        from::<String>();
        to::<&str>();
        to::<Bytes>();
        from::<Bytes>();
        to::<&Bytes>();
        to::<&[u8]>();
        to::<Vec<i64>>();
        from::<Vec<i64>>();
        to::<[i64; 2]>();
        from::<[i64; 2]>();
        to::<(i64, String)>();
        from::<(i64, String)>();
        to::<HashMap<String, i64>>();
        from::<HashMap<String, i64>>();
        to::<BTreeMap<String, i64>>();
        from::<BTreeMap<String, i64>>();
        to::<HashSet<i64>>();
        from::<HashSet<i64>>();
        to::<BTreeSet<i64>>();
        from::<BTreeSet<i64>>();
        to::<Option<i64>>();
        from::<Option<i64>>();
        from::<Iterable<Vec<i64>>>();
        to::<Value<Stub>>();
        from::<Value<Stub>>();
        to::<Object<Stub>>();
        from::<Object<Stub>>();
        to::<Function<Stub>>();
        from::<Function<Stub>>();
        to::<Module<Stub>>();
        from::<Module<Stub>>();
        to::<Class<Stub>>();
        from::<Class<Stub>>();
        to::<Class<Stub, String>>();
        from::<Class<Stub, String>>();
        to::<Instance<Stub>>();
        from::<Instance<Stub>>();
        args::<()>();
        args::<(i64, String)>();
        args::<Vec<i64>>();
        kwargs::<()>();
        kwargs::<[(&str, i64); 1]>();
        kwargs::<Vec<(String, i64)>>();
        kwargs::<HashMap<String, i64>>();
    }
}
