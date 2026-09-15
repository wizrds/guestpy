//! Host-authored, guest-visible iterators and async iterators.

use std::{cell::RefCell, future::{Future, poll_fn}, task::Poll, pin::Pin, rc::Rc};

use futures::Stream;

use crate::{
    backend::{
        Backend, BackendCallables, BackendClasses, BackendCoroutines, BackendModules,
        BackendValues, callables::PendingValue,
    },
    errors::Error,
    handle::Value,
    host::dunder::Dunder,
    marshal::{FromGuest, ToGuest},
    scope::Enter,
};

type HostIterInner<T> = Box<dyn Iterator<Item = Result<T, Error>>>;

pub struct HostIter<T>(RefCell<HostIterInner<T>>);

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
            &B::call(
                enter.token(),
                &B::get_attr(enter.token(), &B::native_base(enter.token()), "__class__")?,
                &[
                    B::str(enter.token(), "HostIter"),
                    B::tuple(enter.token(), vec![B::native_base(enter.token())])?,
                    namespace,
                ],
                &[],
            )?,
            self,
        )
    }
}

type HostStreamInner<T> = Pin<Box<dyn Stream<Item = Result<T, Error>>>>;

pub struct HostStream<T>(Rc<RefCell<HostStreamInner<T>>>);

impl<T: 'static> HostStream<T> {
    pub fn new<S>(stream: S) -> Self
    where
        S: Stream<Item = Result<T, Error>> + 'static,
    {
        Self(Rc::new(RefCell::new(Box::pin(stream))))
    }

    fn next(&self) -> impl Future<Output = Result<T, Error>> + 'static {
        let stream = self.0.clone();

        poll_fn(move |context| {
            let Ok(mut stream) = stream.try_borrow_mut() else {
                return Poll::Ready(Err(Error::unexpected(
                    "host stream is already being polled",
                )));
            };

            match stream.as_mut().poll_next(context) {
                Poll::Ready(Some(Ok(value))) => Poll::Ready(Ok(value)),
                Poll::Ready(Some(Err(error))) => Poll::Ready(Err(error)),
                Poll::Ready(None) => Poll::Ready(Err(Error::StopAsyncIteration)),
                Poll::Pending => Poll::Pending,
            }
        })
    }
}

impl<B, T> ToGuest<B> for HostStream<T>
where
    B: Backend
        + BackendValues
        + BackendCallables
        + BackendClasses
        + BackendModules
        + BackendCoroutines,
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
                        enter
                            .guest()
                            .ensure_async_driver(enter)?
                            .driver()
                            .register_host_future(
                                enter,
                                PendingValue::<B, T>::into_host_future(
                                    B::borrow::<Self>(enter.token(), &args.split_receiver()?.0)?
                                        .next(),
                                )
                            )
                    })),
            )?,
        )?;

        B::instantiate::<Self>(
            enter.token(),
            &B::call(
                enter.token(),
                &B::get_attr(enter.token(), &B::native_base(enter.token()), "__class__")?,
                &[
                    B::str(enter.token(), "HostStream"),
                    B::tuple(enter.token(), vec![B::native_base(enter.token())])?,
                    namespace,
                ],
                &[],
            )?,
            self,
        )
    }
}
