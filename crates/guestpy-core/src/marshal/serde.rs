use std::ops::{Deref, DerefMut};

use ::serde::{
    de::{self, DeserializeSeed, Visitor},
    forward_to_deserialize_any,
    ser::{
        self, Serialize, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant,
        SerializeTuple, SerializeTupleStruct, SerializeTupleVariant,
    },
};

use crate::{
    backend::{Backend, BackendValues},
    errors::Error,
    marshal::{FromGuest, ToGuest},
    scope::Enter,
};

pub(crate) struct Serializer<'py, B: Backend + BackendValues> {
    token: B::Token<'py>,
}

impl<'py, B: Backend + BackendValues> Serializer<'py, B> {
    pub(crate) fn new(token: B::Token<'py>) -> Self {
        Self { token }
    }
}

impl<'py, B: Backend + BackendValues> ser::Serializer for Serializer<'py, B> {
    type Ok = B::Value<'py>;
    type Error = Error;
    type SerializeSeq = SeqSerializer<'py, B>;
    type SerializeTuple = TupleSerializer<'py, B>;
    type SerializeTupleStruct = TupleSerializer<'py, B>;
    type SerializeTupleVariant = TupleVariantSerializer<'py, B>;
    type SerializeMap = MapSerializer<'py, B>;
    type SerializeStruct = StructSerializer<'py, B>;
    type SerializeStructVariant = StructVariantSerializer<'py, B>;

    fn serialize_bool(self, value: bool) -> Result<Self::Ok, Error> {
        Ok(B::bool(self.token, value))
    }

    fn serialize_i8(self, value: i8) -> Result<Self::Ok, Error> {
        Ok(B::int(self.token, value.into()))
    }

    fn serialize_i16(self, value: i16) -> Result<Self::Ok, Error> {
        Ok(B::int(self.token, value.into()))
    }

    fn serialize_i32(self, value: i32) -> Result<Self::Ok, Error> {
        Ok(B::int(self.token, value.into()))
    }

    fn serialize_i64(self, value: i64) -> Result<Self::Ok, Error> {
        Ok(B::int(self.token, value))
    }

    fn serialize_i128(self, _: i128) -> Result<Self::Ok, Error> {
        Err(Error::conversion(
            "128-bit integers are not supported by the Python value format",
        ))
    }

    fn serialize_u8(self, value: u8) -> Result<Self::Ok, Error> {
        Ok(B::uint(self.token, value.into()))
    }

    fn serialize_u16(self, value: u16) -> Result<Self::Ok, Error> {
        Ok(B::uint(self.token, value.into()))
    }

    fn serialize_u32(self, value: u32) -> Result<Self::Ok, Error> {
        Ok(B::uint(self.token, value.into()))
    }

    fn serialize_u64(self, value: u64) -> Result<Self::Ok, Error> {
        Ok(B::uint(self.token, value))
    }

    fn serialize_u128(self, _: u128) -> Result<Self::Ok, Error> {
        Err(Error::conversion(
            "128-bit integers are not supported by the Python value format",
        ))
    }

    fn serialize_f32(self, value: f32) -> Result<Self::Ok, Error> {
        Ok(B::float(self.token, value.into()))
    }

    fn serialize_f64(self, value: f64) -> Result<Self::Ok, Error> {
        Ok(B::float(self.token, value))
    }

    fn serialize_char(self, value: char) -> Result<Self::Ok, Error> {
        Ok(B::str(self.token, value.encode_utf8(&mut [0; 4])))
    }

    fn serialize_str(self, value: &str) -> Result<Self::Ok, Error> {
        Ok(B::str(self.token, value))
    }

    fn serialize_bytes(self, value: &[u8]) -> Result<Self::Ok, Error> {
        Ok(B::bytes(self.token, value))
    }

    fn serialize_none(self) -> Result<Self::Ok, Error> {
        Ok(B::none(self.token))
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Self::Ok, Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Error> {
        Ok(B::none(self.token))
    }

    fn serialize_unit_struct(self, _: &'static str) -> Result<Self::Ok, Error> {
        Ok(B::none(self.token))
    }

    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Error> {
        Ok(B::str(self.token, variant))
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Error> {
        B::dict(
            self.token,
            vec![(B::str(self.token, variant), value.serialize(Serializer::<B>::new(self.token))?)],
        )
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, Error> {
        Ok(SeqSerializer { token: self.token, items: vec![] })
    }

    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, Error> {
        Ok(TupleSerializer { token: self.token, items: vec![] })
    }

    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, Error> {
        Ok(TupleSerializer { token: self.token, items: vec![] })
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        Ok(TupleVariantSerializer {
            token: self.token,
            variant,
            items: vec![],
        })
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, Error> {
        Ok(MapSerializer {
            token: self.token,
            pairs: vec![],
            key: None,
        })
    }

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct, Error> {
        Ok(StructSerializer { token: self.token, pairs: vec![] })
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        Ok(StructVariantSerializer {
            token: self.token,
            variant,
            pairs: vec![],
        })
    }

    fn is_human_readable(&self) -> bool {
        true
    }
}

pub(crate) struct SeqSerializer<'py, B: Backend + BackendValues> {
    token: B::Token<'py>,
    items: Vec<B::Value<'py>>,
}

impl<'py, B: Backend + BackendValues> SerializeSeq for SeqSerializer<'py, B> {
    type Ok = B::Value<'py>;
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        self.items
            .push(value.serialize(Serializer::<B>::new(self.token))?);

        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Error> {
        B::list(self.token, self.items)
    }
}

pub(crate) struct TupleSerializer<'py, B: Backend + BackendValues> {
    token: B::Token<'py>,
    items: Vec<B::Value<'py>>,
}

impl<'py, B: Backend + BackendValues> SerializeTuple for TupleSerializer<'py, B> {
    type Ok = B::Value<'py>;
    type Error = Error;

    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        self.items
            .push(value.serialize(Serializer::<B>::new(self.token))?);

        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Error> {
        B::tuple(self.token, self.items)
    }
}

impl<'py, B: Backend + BackendValues> SerializeTupleStruct for TupleSerializer<'py, B> {
    type Ok = B::Value<'py>;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        self.items
            .push(value.serialize(Serializer::<B>::new(self.token))?);

        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Error> {
        B::tuple(self.token, self.items)
    }
}

pub(crate) struct TupleVariantSerializer<'py, B: Backend + BackendValues> {
    token: B::Token<'py>,
    variant: &'static str,
    items: Vec<B::Value<'py>>,
}

impl<'py, B: Backend + BackendValues> SerializeTupleVariant for TupleVariantSerializer<'py, B> {
    type Ok = B::Value<'py>;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        self.items
            .push(value.serialize(Serializer::<B>::new(self.token))?);

        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Error> {
        B::dict(
            self.token,
            vec![(B::str(self.token, self.variant), B::tuple(self.token, self.items)?)],
        )
    }
}

pub(crate) struct MapSerializer<'py, B: Backend + BackendValues> {
    token: B::Token<'py>,
    pairs: Vec<(B::Value<'py>, B::Value<'py>)>,
    key: Option<B::Value<'py>>,
}

impl<'py, B: Backend + BackendValues> SerializeMap for MapSerializer<'py, B> {
    type Ok = B::Value<'py>;
    type Error = Error;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), Error> {
        self.key = Some(key.serialize(Serializer::<B>::new(self.token))?);

        Ok(())
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), Error> {
        self.pairs.push((
            self.key
                .take()
                .ok_or_else(|| Error::conversion("serialize_value called before serialize_key"))?,
            value.serialize(Serializer::<B>::new(self.token))?,
        ));

        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Error> {
        B::dict(self.token, self.pairs)
    }
}

pub(crate) struct StructSerializer<'py, B: Backend + BackendValues> {
    token: B::Token<'py>,
    pairs: Vec<(B::Value<'py>, B::Value<'py>)>,
}

impl<'py, B: Backend + BackendValues> SerializeStruct for StructSerializer<'py, B> {
    type Ok = B::Value<'py>;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.pairs
            .push((B::str(self.token, key), value.serialize(Serializer::<B>::new(self.token))?));

        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Error> {
        B::dict(self.token, self.pairs)
    }
}

pub(crate) struct StructVariantSerializer<'py, B: Backend + BackendValues> {
    token: B::Token<'py>,
    variant: &'static str,
    pairs: Vec<(B::Value<'py>, B::Value<'py>)>,
}

impl<'py, B: Backend + BackendValues> SerializeStructVariant for StructVariantSerializer<'py, B> {
    type Ok = B::Value<'py>;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.pairs
            .push((B::str(self.token, key), value.serialize(Serializer::<B>::new(self.token))?));

        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Error> {
        B::dict(
            self.token,
            vec![(B::str(self.token, self.variant), B::dict(self.token, self.pairs)?)],
        )
    }
}

pub(crate) struct Deserializer<'py, B: Backend + BackendValues> {
    token: B::Token<'py>,
    value: B::Value<'py>,
}

impl<'py, B: Backend + BackendValues> Deserializer<'py, B> {
    pub(crate) fn new(token: B::Token<'py>, value: B::Value<'py>) -> Self {
        Self { token, value }
    }
}

impl<'de, 'py, B: Backend + BackendValues> de::Deserializer<'de> for Deserializer<'py, B> {
    type Error = Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        if B::is_none(self.token, &self.value) {
            visitor.visit_unit()
        } else if B::is_bool(self.token, &self.value) {
            visitor.visit_bool(B::as_bool(self.token, &self.value)?)
        } else if B::is_int(self.token, &self.value) {
            match B::as_i64(self.token, &self.value) {
                Ok(value) => visitor.visit_i64(value),
                Err(_) => visitor.visit_u64(B::as_u64(self.token, &self.value)?),
            }
        } else if B::is_float(self.token, &self.value) {
            visitor.visit_f64(B::as_f64(self.token, &self.value)?)
        } else if B::is_str(self.token, &self.value) {
            visitor.visit_string(B::as_str(self.token, &self.value)?)
        } else if B::is_bytes(self.token, &self.value) {
            visitor.visit_byte_buf(B::as_bytes(self.token, &self.value)?)
        } else if B::is_list(self.token, &self.value)
            || B::is_tuple(self.token, &self.value)
            || B::is_set(self.token, &self.value)
        {
            visitor.visit_seq(SeqAccess::<B>::new(self.token, &self.value)?)
        } else if B::is_dict(self.token, &self.value) {
            visitor.visit_map(MapAccess::<B>::new(self.token, self.value)?)
        } else {
            Err(Error::conversion(format!(
                "cannot deserialize a Python value of type {}",
                B::type_name(self.token, &self.value)
            )))
        }
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        if B::is_none(self.token, &self.value) {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_newtype_struct<V>(self, _: &'static str, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_enum<V>(
        self,
        _: &'static str,
        _: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        if B::is_str(self.token, &self.value) {
            return visitor.visit_enum(EnumAccess::<B> {
                token: self.token,
                variant: self.value,
                payload: None,
            });
        }

        if !B::is_dict(self.token, &self.value) {
            return Err(Error::type_mismatch(
                "str or dict",
                &B::type_name(self.token, &self.value),
            ));
        }

        let iterator = B::iter(self.token, &self.value)?;
        let variant = B::next(self.token, &iterator)?
            .ok_or_else(|| Error::conversion("expected a single-key dict for an enum variant"))?;

        if B::next(self.token, &iterator)?.is_some() {
            return Err(Error::conversion("expected exactly one key for an enum variant"));
        }

        let payload = B::get_item(self.token, &self.value, &variant)?;
        visitor.visit_enum(EnumAccess::<B> {
            token: self.token,
            variant,
            payload: Some(payload),
        })
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf unit unit_struct seq tuple tuple_struct map struct
        identifier ignored_any
    }

    fn is_human_readable(&self) -> bool {
        true
    }
}

struct SeqAccess<'py, B: Backend + BackendValues> {
    token: B::Token<'py>,
    iterator: B::Value<'py>,
}

impl<'py, B: Backend + BackendValues> SeqAccess<'py, B> {
    fn new(token: B::Token<'py>, value: &B::Value<'py>) -> Result<Self, Error> {
        Ok(Self { token, iterator: B::iter(token, value)? })
    }
}

impl<'de, 'py, B: Backend + BackendValues> de::SeqAccess<'de> for SeqAccess<'py, B> {
    type Error = Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Error>
    where
        T: DeserializeSeed<'de>,
    {
        match B::next(self.token, &self.iterator)? {
            Some(value) => seed
                .deserialize(Deserializer::<B>::new(self.token, value))
                .map(Some),
            None => Ok(None),
        }
    }
}

struct MapAccess<'py, B: Backend + BackendValues> {
    token: B::Token<'py>,
    value: B::Value<'py>,
    iterator: B::Value<'py>,
    pending: Option<B::Value<'py>>,
}

impl<'py, B: Backend + BackendValues> MapAccess<'py, B> {
    fn new(token: B::Token<'py>, value: B::Value<'py>) -> Result<Self, Error> {
        let iterator = B::iter(token, &value)?;

        Ok(Self { token, value, iterator, pending: None })
    }
}

impl<'de, 'py, B: Backend + BackendValues> de::MapAccess<'de> for MapAccess<'py, B> {
    type Error = Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Error>
    where
        K: DeserializeSeed<'de>,
    {
        match B::next(self.token, &self.iterator)? {
            Some(key) => {
                self.pending = Some(B::get_item(self.token, &self.value, &key)?);
                seed.deserialize(Deserializer::<B>::new(self.token, key))
                    .map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Error>
    where
        V: DeserializeSeed<'de>,
    {
        seed.deserialize(Deserializer::<B>::new(
            self.token,
            self.pending
                .take()
                .ok_or_else(|| Error::conversion("next_value_seed called before next_key_seed"))?,
        ))
    }
}

struct EnumAccess<'py, B: Backend + BackendValues> {
    token: B::Token<'py>,
    variant: B::Value<'py>,
    payload: Option<B::Value<'py>>,
}

impl<'de, 'py, B: Backend + BackendValues> de::EnumAccess<'de> for EnumAccess<'py, B> {
    type Error = Error;
    type Variant = VariantAccess<'py, B>;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Error>
    where
        V: DeserializeSeed<'de>,
    {
        Ok((
            seed.deserialize(Deserializer::<B>::new(self.token, self.variant))?,
            VariantAccess { token: self.token, payload: self.payload },
        ))
    }
}

struct VariantAccess<'py, B: Backend + BackendValues> {
    token: B::Token<'py>,
    payload: Option<B::Value<'py>>,
}

impl<'py, B: Backend + BackendValues> VariantAccess<'py, B> {
    fn payload(&self) -> Result<B::Value<'py>, Error> {
        self.payload
            .clone()
            .ok_or_else(|| Error::conversion("expected a payload for the enum variant"))
    }
}

impl<'de, 'py, B: Backend + BackendValues> de::VariantAccess<'de> for VariantAccess<'py, B> {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        if self.payload.is_none() {
            Ok(())
        } else {
            Err(Error::conversion("expected a unit enum variant"))
        }
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Error>
    where
        T: DeserializeSeed<'de>,
    {
        seed.deserialize(Deserializer::<B>::new(self.token, self.payload()?))
    }

    fn tuple_variant<V>(self, _: usize, visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_seq(SeqAccess::<B>::new(self.token, &self.payload()?)?)
    }

    fn struct_variant<V>(self, _: &'static [&'static str], visitor: V) -> Result<V::Value, Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_map(MapAccess::<B>::new(self.token, self.payload()?)?)
    }
}

pub struct Serde<T>(pub T);

impl<T> Serde<T> {
    pub fn as_inner(&self) -> &T {
        &self.0
    }

    pub fn as_inner_mut(&mut self) -> &mut T {
        &mut self.0
    }

    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<B, T> ToGuest<B> for Serde<T>
where
    B: Backend + BackendValues,
    T: Serialize,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        enter.to_value(&self.0)
    }
}

impl<B, T> FromGuest<B> for Serde<T>
where
    B: Backend + BackendValues,
    T: ::serde::de::DeserializeOwned + 'static,
{
    type Owned = T;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        enter.from_value(value)
    }
}

impl<T> Deref for Serde<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for Serde<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> From<T> for Serde<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::{Formatter, Result as FmtResult};

    use super::{Deserializer, Serializer, de, ser};
    use crate::{
        backend::tests::{Stub, StubValue},
        errors::Error,
    };
    use ::serde::{Deserialize, Serialize, de::DeserializeOwned};

    #[derive(Debug, Deserialize, PartialEq, Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Point {
        x_coord: i64,
        y_coord: i64,
        label: String,
        tags: Vec<String>,
        maybe: Option<i64>,
    }

    #[derive(Debug, Deserialize, PartialEq, Serialize)]
    enum Shape {
        Unit,
        Newtype(i64),
        Pair(i64, i64),
        Rect { width: i64, height: i64 },
    }

    struct RawBytes(Vec<u8>);

    impl Serialize for RawBytes {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: ser::Serializer,
        {
            serializer.serialize_bytes(&self.0)
        }
    }

    impl<'de> Deserialize<'de> for RawBytes {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: de::Deserializer<'de>,
        {
            struct BytesVisitor;

            impl<'de> de::Visitor<'de> for BytesVisitor {
                type Value = RawBytes;

                fn expecting(&self, formatter: &mut Formatter<'_>) -> FmtResult {
                    formatter.write_str("a byte string")
                }

                fn visit_byte_buf<E>(self, value: Vec<u8>) -> Result<Self::Value, E> {
                    Ok(RawBytes(value))
                }

                fn visit_bytes<E>(self, value: &[u8]) -> Result<Self::Value, E> {
                    Ok(RawBytes(value.to_vec()))
                }
            }

            deserializer.deserialize_bytes(BytesVisitor)
        }
    }

    struct Fixtures;

    impl Fixtures {
        fn point() -> Point {
            Point {
                x_coord: 1,
                y_coord: 2,
                label: "origin".to_owned(),
                tags: vec!["a".to_owned(), "b".to_owned()],
                maybe: None,
            }
        }

        fn to_stub<T: Serialize>(value: &T) -> Result<StubValue, Error> {
            value.serialize(Serializer::<'_, Stub>::new(()))
        }

        fn from_stub<T: DeserializeOwned>(value: StubValue) -> Result<T, Error> {
            T::deserialize(Deserializer::<'_, Stub>::new((), value))
        }
    }

    #[test]
    fn struct_serializes_to_a_camel_case_dict() {
        assert_eq!(
            Fixtures::to_stub(&Fixtures::point()).expect("serializes"),
            StubValue::Dict(vec![
                (StubValue::Str("xCoord".to_owned()), StubValue::Int(1)),
                (StubValue::Str("yCoord".to_owned()), StubValue::Int(2)),
                (StubValue::Str("label".to_owned()), StubValue::Str("origin".to_owned()),),
                (
                    StubValue::Str("tags".to_owned()),
                    StubValue::List(vec![
                        StubValue::Str("a".to_owned()),
                        StubValue::Str("b".to_owned()),
                    ]),
                ),
                (StubValue::Str("maybe".to_owned()), StubValue::None),
            ]),
        );
    }

    #[test]
    fn struct_round_trips() {
        let point = Fixtures::point();
        let back = Fixtures::from_stub(Fixtures::to_stub::<Point>(&point).expect("serializes"))
            .expect("deserializes");

        assert_eq!(point, back);
    }

    #[test]
    fn rust_tuples_serialize_to_stub_tuples() {
        assert!(matches!(Fixtures::to_stub(&(1_i64, "a".to_owned())), Ok(StubValue::Tuple(_))));
    }

    #[test]
    fn vecs_serialize_to_stub_lists() {
        assert!(matches!(Fixtures::to_stub(&vec![1_i64, 2, 3]), Ok(StubValue::List(_))));
    }

    #[test]
    fn enum_variants_use_external_tagging() {
        assert_eq!(Fixtures::to_stub(&Shape::Unit).unwrap(), StubValue::Str("Unit".to_owned()));

        assert_eq!(
            Fixtures::to_stub(&Shape::Newtype(5)).unwrap(),
            StubValue::Dict(vec![(StubValue::Str("Newtype".to_owned()), StubValue::Int(5))]),
        );

        assert_eq!(
            Fixtures::to_stub(&Shape::Pair(1, 2)).unwrap(),
            StubValue::Dict(vec![(
                StubValue::Str("Pair".to_owned()),
                StubValue::Tuple(vec![StubValue::Int(1), StubValue::Int(2)]),
            )]),
        );

        assert_eq!(
            Fixtures::to_stub(&Shape::Rect { width: 3, height: 4 }).unwrap(),
            StubValue::Dict(vec![(
                StubValue::Str("Rect".to_owned()),
                StubValue::Dict(vec![
                    (StubValue::Str("width".to_owned()), StubValue::Int(3)),
                    (StubValue::Str("height".to_owned()), StubValue::Int(4)),
                ]),
            )]),
        );
    }

    #[test]
    fn enum_variants_round_trip() {
        for shape in [
            Shape::Unit,
            Shape::Newtype(5),
            Shape::Pair(1, 2),
            Shape::Rect { width: 3, height: 4 },
        ] {
            let value = Fixtures::to_stub(&shape).expect("serializes");
            let back: Shape = Fixtures::from_stub(value).expect("deserializes");

            assert_eq!(shape, back);
        }
    }

    #[test]
    fn hash_maps_round_trip_through_stub_dicts() {
        let mut map = std::collections::HashMap::new();
        map.insert("a".to_owned(), 1_i64);
        map.insert("b".to_owned(), 2_i64);

        let value = Fixtures::to_stub(&map).expect("serializes");
        let back = Fixtures::from_stub::<std::collections::HashMap<String, i64>>(value)
            .expect("deserializes");

        assert_eq!(map, back);
    }

    #[test]
    fn raw_bytes_round_trip_through_stub_bytes() {
        let value = Fixtures::to_stub(&RawBytes(vec![1, 2, 3])).expect("serializes");

        assert_eq!(value, StubValue::Bytes(vec![1, 2, 3]));

        let back = Fixtures::from_stub::<RawBytes>(value).expect("deserializes");

        assert_eq!(back.0, vec![1, 2, 3]);
    }

    #[test]
    fn i128_is_rejected() {
        assert!(Fixtures::to_stub(&123_i128).is_err());
    }

    #[test]
    fn wrong_shape_is_a_conversion_error() {
        assert!(matches!(
            Fixtures::from_stub::<Point>(StubValue::Int(1)).unwrap_err(),
            Error::Conversion { .. },
        ));
    }
}
