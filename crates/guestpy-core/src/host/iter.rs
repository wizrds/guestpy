//! Host-authored, guest-visible iterators and async iterators.

use std::{cell::RefCell, pin::Pin, rc::Rc};

use futures::{Stream, StreamExt};

use crate::{
    backend::{
        Backend, BackendCallables, BackendClasses, BackendCoroutines, BackendExceptions,
        BackendModules, BackendValues, callables::PendingValue,
    },
    errors::Error,
    handle::Value,
    host::dunder::Dunder,
    marshal::{FromGuest, ToGuest},
    scope::Enter,
};

pub struct HostIter<T>(RefCell<Box<dyn Iterator<Item = Result<T, Error>>>>);

impl<T: 'static> HostIter<T> {
    pub fn new<I>(iter: I) -> Self
    where
        I: Iterator<Item = Result<T, Error>> + 'static,
    {
        Self(RefCell::new(Box::new(iter)))
    }
}

impl<B, T> ToGuest<B> for HostIter<T>
where
    B: Backend + BackendValues + BackendCallables + BackendClasses,
    T: ToGuest<B> + 'static,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        let namespace = B::new_dict(enter.token())?;

        B::set_item(
            enter.token(),
            &namespace,
            B::str(enter.token(), Dunder::Iter.name()),
            B::method(
                enter.token(),
                Dunder::Iter.name(),
                None,
                enter
                    .guest()
                    .raw_body(Rc::new(|enter, args| {
                        Value::<B>::from_guest(enter, args.split_receiver()?.0)?.to_guest(enter)
                    })),
            )?,
        )?;

        B::set_item(
            enter.token(),
            &namespace,
            B::str(enter.token(), Dunder::Next.name()),
            B::method(
                enter.token(),
                Dunder::Next.name(),
                None,
                enter
                    .guest()
                    .raw_body(Rc::new(|enter, args| {
                        match B::borrow::<Self>(enter.token(), &args.split_receiver()?.0)?
                            .0
                            .borrow_mut()
                            .next()
                        {
                            Some(Ok(value)) => value.to_guest(enter),
                            Some(Err(error)) => Err(error),
                            None => Err(Error::StopIteration),
                        }
                    })),
            )?,
        )?;

        B::instantiate::<Self>(
            enter.token(),
            &B::new_class(enter.token(), "HostIter", &[], &namespace)?,
            self,
        )
    }
}

pub struct HostStream<T>(Rc<RefCell<Pin<Box<dyn Stream<Item = Result<T, Error>>>>>>);

impl<T: 'static> HostStream<T> {
    pub fn new<S>(stream: S) -> Self
    where
        S: Stream<Item = Result<T, Error>> + 'static,
    {
        Self(Rc::new(RefCell::new(Box::pin(stream))))
    }
}

impl<B, T> ToGuest<B> for HostStream<T>
where
    B: Backend
        + BackendValues
        + BackendCallables
        + BackendClasses
        + BackendModules
        + BackendCoroutines
        + BackendExceptions,
    T: ToGuest<B> + 'static,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        let namespace = B::new_dict(enter.token())?;

        B::set_item(
            enter.token(),
            &namespace,
            B::str(enter.token(), Dunder::Aiter.name()),
            B::method(
                enter.token(),
                Dunder::Aiter.name(),
                None,
                enter
                    .guest()
                    .raw_body(Rc::new(|enter, args| {
                        Value::<B>::from_guest(enter, args.split_receiver()?.0)?.to_guest(enter)
                    })),
            )?,
        )?;

        B::set_item(
            enter.token(),
            &namespace,
            B::str(enter.token(), Dunder::Anext.name()),
            B::method(
                enter.token(),
                Dunder::Anext.name(),
                None,
                enter
                    .guest()
                    .raw_body(Rc::new(|enter, args| {
                        let stream = B::borrow::<Self>(enter.token(), &args.split_receiver()?.0)?
                            .0
                            .clone();

                        enter
                            .guest()
                            .ensure_async_driver(enter)?
                            .driver()
                            .register_host_future(
                                enter,
                                PendingValue::<B, T>::into_host_future(async move {
                                    match stream.borrow_mut().next().await {
                                        Some(Ok(value)) => Ok(value),
                                        Some(Err(error)) => Err(error),
                                        None => Err(Error::StopAsyncIteration),
                                    }
                                }),
                            )
                    })),
            )?,
        )?;

        B::instantiate::<Self>(
            enter.token(),
            &B::new_class(enter.token(), "HostStream", &[], &namespace)?,
            self,
        )
    }
}
