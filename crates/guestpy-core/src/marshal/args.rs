use std::{
    cell::Cell,
    collections::HashMap,
    fmt::{Debug, Formatter, Result as FmtResult},
};

use crate::{
    backend::Backend,
    errors::Error,
    marshal::{FromGuest, FromGuestMut, FromGuestRef, ToGuest},
    scope::Enter,
};

enum Argument<'a> {
    Positional(usize),
    Named { index: usize, name: &'a str },
    Keyword(&'a str),
}

pub struct Args<'py, B: Backend> {
    positional: Vec<B::Value<'py>>,
    keyword: Vec<(String, B::Value<'py>)>,
    consumed_positional: Vec<Cell<bool>>,
    consumed_keyword: Vec<Cell<bool>>,
}

impl<'py, B: Backend> Args<'py, B> {
    pub(crate) fn new(
        positional: Vec<B::Value<'py>>,
        keyword: Vec<(String, B::Value<'py>)>,
    ) -> Self {
        Self {
            consumed_positional: vec![Cell::new(false); positional.len()],
            consumed_keyword: vec![Cell::new(false); keyword.len()],
            positional,
            keyword,
        }
    }

    fn positional(&self, index: usize) -> Option<&B::Value<'py>> {
        let value = self.positional.get(index)?;
        self.consumed_positional[index].set(true);
        Some(value)
    }

    fn keyword(&self, name: &str) -> Option<&B::Value<'py>> {
        let index = self
            .keyword
            .iter()
            .position(|(keyword, _)| keyword == name)?;

        self.consumed_keyword[index].set(true);
        Some(&self.keyword[index].1)
    }

    fn value(&self, argument: Argument<'_>) -> Option<&B::Value<'py>> {
        match argument {
            Argument::Positional(index) => self.positional(index),
            Argument::Named { index, name } => self
                .positional(index)
                .or_else(|| self.keyword(name)),
            Argument::Keyword(name) => self.keyword(name),
        }
    }

    fn rest_values(&self, index: usize) -> &[B::Value<'py>] {
        let Some(values) = self.positional.get(index..) else {
            return &[];
        };

        for consumed in &self.consumed_positional[index..] {
            consumed.set(true);
        }

        values
    }

    pub fn positional_len(&self) -> usize {
        self.positional.len()
    }

    pub fn keyword_len(&self) -> usize {
        self.keyword.len()
    }

    pub fn is_empty(&self) -> bool {
        self.positional.is_empty() && self.keyword.is_empty()
    }

    pub fn finish(&self) -> Result<(), Error> {
        if self
            .consumed_positional
            .iter()
            .any(|consumed| !consumed.get())
        {
            return Err(Error::conversion(format!(
                "expected at most {} positional arguments, got {}",
                self.consumed_positional
                    .iter()
                    .filter(|consumed| consumed.get())
                    .count(),
                self.positional.len(),
            )));
        }

        if let Some((name, _)) = self
            .keyword
            .iter()
            .zip(&self.consumed_keyword)
            .find_map(|((name, value), consumed)| (!consumed.get()).then_some((name, value)))
        {
            return Err(Error::conversion(format!("unexpected keyword argument '{name}'",)));
        }

        Ok(())
    }

    pub fn required<T>(
        &self,
        enter: &Enter<'py, B>,
        index: usize,
        name: &str,
    ) -> Result<T::Owned, Error>
    where
        T: FromGuest<B>,
    {
        T::from_guest(
            enter,
            self.value(Argument::Named { index, name })
                .cloned()
                .ok_or_else(|| Error::conversion(format!("missing required argument '{name}'",)))?,
        )
    }

    pub fn optional<T>(
        &self,
        enter: &Enter<'py, B>,
        index: usize,
        name: &str,
    ) -> Result<Option<T::Owned>, Error>
    where
        T: FromGuest<B>,
    {
        self.value(Argument::Named { index, name })
            .cloned()
            .map(|value| T::from_guest(enter, value))
            .transpose()
    }

    pub fn required_positional<T>(
        &self,
        enter: &Enter<'py, B>,
        index: usize,
    ) -> Result<T::Owned, Error>
    where
        T: FromGuest<B>,
    {
        T::from_guest(
            enter,
            self.value(Argument::Positional(index))
                .cloned()
                .ok_or_else(|| {
                    Error::conversion(format!("missing required positional argument {index}",))
                })?,
        )
    }

    pub fn optional_positional<T>(
        &self,
        enter: &Enter<'py, B>,
        index: usize,
    ) -> Result<Option<T::Owned>, Error>
    where
        T: FromGuest<B>,
    {
        self.value(Argument::Positional(index))
            .cloned()
            .map(|value| T::from_guest(enter, value))
            .transpose()
    }

    pub fn required_keyword<T>(&self, enter: &Enter<'py, B>, name: &str) -> Result<T::Owned, Error>
    where
        T: FromGuest<B>,
    {
        T::from_guest(
            enter,
            self.value(Argument::Keyword(name))
                .cloned()
                .ok_or_else(|| {
                    Error::conversion(format!("missing required keyword argument '{name}'",))
                })?,
        )
    }

    pub fn optional_keyword<T>(
        &self,
        enter: &Enter<'py, B>,
        name: &str,
    ) -> Result<Option<T::Owned>, Error>
    where
        T: FromGuest<B>,
    {
        self.value(Argument::Keyword(name))
            .cloned()
            .map(|value| T::from_guest(enter, value))
            .transpose()
    }

    pub fn rest<T>(&self, enter: &Enter<'py, B>, index: usize) -> Result<Vec<T::Owned>, Error>
    where
        T: FromGuest<B>,
    {
        self.rest_values(index)
            .iter()
            .cloned()
            .map(|value| T::from_guest(enter, value))
            .collect()
    }

    pub fn rest_keywords<T>(&self, enter: &Enter<'py, B>) -> Result<Vec<(String, T::Owned)>, Error>
    where
        T: FromGuest<B>,
    {
        self.keyword
            .iter()
            .zip(&self.consumed_keyword)
            .filter_map(|((name, value), consumed)| {
                if consumed.replace(true) {
                    None
                } else {
                    Some((name.clone(), value.clone()))
                }
            })
            .map(|(name, value)| Ok((name, T::from_guest(enter, value)?)))
            .collect()
    }

    pub fn borrow<C>(&self, enter: &Enter<'py, B>, index: usize) -> Result<C::Ref<'_>, Error>
    where
        C: FromGuestRef<'py, B>,
    {
        C::from_guest_ref(
            enter,
            self.value(Argument::Positional(index))
                .ok_or_else(|| {
                    Error::conversion(format!("missing required positional argument {index}",))
                })?,
        )
    }

    pub fn borrow_mut<C>(&self, enter: &Enter<'py, B>, index: usize) -> Result<C::Mut<'_>, Error>
    where
        C: FromGuestMut<'py, B>,
    {
        C::from_guest_mut(
            enter,
            self.value(Argument::Positional(index))
                .ok_or_else(|| {
                    Error::conversion(format!("missing required positional argument {index}",))
                })?,
        )
    }

    pub(crate) fn split_receiver(mut self) -> Result<(B::Value<'py>, Self), Error> {
        if self.positional.is_empty() {
            return Err(Error::conversion("missing required receiver"));
        }

        let receiver = self.positional.remove(0);
        self.consumed_positional.remove(0);

        Ok((receiver, self))
    }
}

impl<B: Backend> Debug for Args<'_, B> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_struct("Args")
            .field("positional", &self.positional)
            .field("keyword", &self.keyword)
            .finish()
    }
}

pub trait ToGuestArgs<B: Backend> {
    fn into_args<'py>(self, enter: &Enter<'py, B>) -> Result<Vec<B::Value<'py>>, Error>;
}

pub trait ToGuestKwargs<B: Backend> {
    fn into_kwargs<'py>(self, enter: &Enter<'py, B>)
    -> Result<Vec<(String, B::Value<'py>)>, Error>;
}

macro_rules! args {
    ($($type:ident:$index:tt),+ $(,)?) => {
        impl<B, $($type),+> ToGuestArgs<B> for ($($type,)+)
        where
            B: Backend,
            $($type: ToGuest<B>,)+
        {
            fn into_args<'py>(
                self,
                enter: &Enter<'py, B>,
            ) -> Result<Vec<B::Value<'py>>, Error> {
                Ok(vec![$(self.$index.to_guest(enter)?,)+])
            }
        }
    };
}

impl<B> ToGuestArgs<B> for ()
where
    B: Backend,
{
    fn into_args<'py>(self, _: &Enter<'py, B>) -> Result<Vec<B::Value<'py>>, Error> {
        Ok(Vec::new())
    }
}

impl<B, V> ToGuestArgs<B> for Vec<V>
where
    B: Backend,
    V: ToGuest<B>,
{
    fn into_args<'py>(self, enter: &Enter<'py, B>) -> Result<Vec<B::Value<'py>>, Error> {
        self.into_iter()
            .map(|value| value.to_guest(enter))
            .collect()
    }
}

args!(A1: 0);
args!(A1: 0, A2: 1);
args!(A1: 0, A2: 1, A3: 2);
args!(A1: 0, A2: 1, A3: 2, A4: 3);
args!(A1: 0, A2: 1, A3: 2, A4: 3, A5: 4);
args!(A1: 0, A2: 1, A3: 2, A4: 3, A5: 4, A6: 5);
args!(A1: 0, A2: 1, A3: 2, A4: 3, A5: 4, A6: 5, A7: 6);
args!(A1: 0, A2: 1, A3: 2, A4: 3, A5: 4, A6: 5, A7: 6, A8: 7);
args!(
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
args!(
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
args!(
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
args!(
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

impl<B> ToGuestKwargs<B> for ()
where
    B: Backend,
{
    fn into_kwargs<'py>(self, _: &Enter<'py, B>) -> Result<Vec<(String, B::Value<'py>)>, Error> {
        Ok(Vec::new())
    }
}

impl<B, V, const N: usize> ToGuestKwargs<B> for [(&str, V); N]
where
    B: Backend,
    V: ToGuest<B>,
{
    fn into_kwargs<'py>(
        self,
        enter: &Enter<'py, B>,
    ) -> Result<Vec<(String, B::Value<'py>)>, Error> {
        self.into_iter()
            .map(|(name, value)| Ok((name.to_owned(), value.to_guest(enter)?)))
            .collect()
    }
}

impl<B, V> ToGuestKwargs<B> for Vec<(String, V)>
where
    B: Backend,
    V: ToGuest<B>,
{
    fn into_kwargs<'py>(
        self,
        enter: &Enter<'py, B>,
    ) -> Result<Vec<(String, B::Value<'py>)>, Error> {
        self.into_iter()
            .map(|(name, value)| Ok((name, value.to_guest(enter)?)))
            .collect()
    }
}

impl<B, V> ToGuestKwargs<B> for HashMap<String, V>
where
    B: Backend,
    V: ToGuest<B>,
{
    fn into_kwargs<'py>(
        self,
        enter: &Enter<'py, B>,
    ) -> Result<Vec<(String, B::Value<'py>)>, Error> {
        self.into_iter()
            .map(|(name, value)| Ok((name, value.to_guest(enter)?)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{Args, Argument};
    use crate::backend::tests::{Stub, StubValue};

    #[test]
    fn tracks_out_of_order_positional_arguments_exactly() {
        let args = Args::<Stub>::new(
            vec![StubValue::Int(1), StubValue::Int(2), StubValue::Int(3)],
            Vec::new(),
        );

        assert_eq!(args.value(Argument::Positional(2)), Some(&StubValue::Int(3)),);
        assert_eq!(
            args.consumed_positional
                .iter()
                .map(Cell::get)
                .collect::<Vec<_>>(),
            vec![false, false, true],
        );
        assert_eq!(
            args.finish().unwrap_err().to_string(),
            "conversion error: expected at most 1 positional arguments, got 3",
        );
    }

    #[test]
    fn rest_does_not_consume_earlier_arguments() {
        let args = Args::<Stub>::new(
            vec![StubValue::Int(1), StubValue::Int(2), StubValue::Int(3)],
            Vec::new(),
        );

        assert_eq!(args.rest_values(1), &[StubValue::Int(2), StubValue::Int(3)],);
        assert_eq!(
            args.consumed_positional
                .iter()
                .map(Cell::get)
                .collect::<Vec<_>>(),
            vec![false, true, true],
        );
        assert!(args.finish().is_err());
    }

    #[test]
    fn tracks_keyword_slots_without_cloning_names() {
        let args = Args::<Stub>::new(
            Vec::new(),
            vec![
                (String::from("first"), StubValue::Int(1)),
                (String::from("second"), StubValue::Int(2)),
            ],
        );

        assert_eq!(args.value(Argument::Keyword("second")), Some(&StubValue::Int(2)),);
        assert_eq!(
            args.consumed_keyword
                .iter()
                .map(Cell::get)
                .collect::<Vec<_>>(),
            vec![false, true],
        );
        assert_eq!(
            args.finish().unwrap_err().to_string(),
            "conversion error: unexpected keyword argument 'first'",
        );
    }

    #[test]
    fn split_receiver_reindexes_remaining_arguments() {
        let (receiver, args) = Args::<Stub>::new(
            vec![StubValue::Int(0), StubValue::Int(1), StubValue::Int(2)],
            Vec::new(),
        )
        .split_receiver()
        .unwrap();

        assert_eq!(receiver, StubValue::Int(0));
        assert_eq!(args.positional_len(), 2);
        assert_eq!(args.value(Argument::Positional(0)), Some(&StubValue::Int(1)),);
    }

    #[test]
    fn empty_arguments_report_empty() {
        let args = Args::<Stub>::new(Vec::new(), Vec::new());

        assert_eq!(args.positional_len(), 0);
        assert_eq!(args.keyword_len(), 0);
        assert!(args.is_empty());
        assert!(args.finish().is_ok());
    }
}
