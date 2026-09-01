//! Guest class handles.

use std::{
    any::TypeId,
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use crate::{
    backend::{Backend, BackendCallables, BackendClasses, BackendValues},
    errors::Error,
    handle::{
        Handle, Object,
        traits::{Annotated, HasHandle, IsType, Named, ObjectProtocol},
    },
    host::class::{ClassSpec, HostClass, HostClassDefinition},
    marshal::{FromGuest, FromGuestMut, FromGuestRef, ToGuest, args::ToGuestArgs},
    scope::Enter,
};

pub struct Ref<'a, B: BackendClasses, C: 'static>(B::Ref<'a, C>);

impl<'a, B: BackendClasses, C: 'static> Deref for Ref<'a, B, C> {
    type Target = C;

    fn deref(&self) -> &C {
        &self.0
    }
}

pub struct RefMut<'a, B: BackendClasses, C: 'static>(B::RefMut<'a, C>);

impl<'a, B: BackendClasses, C: 'static> Deref for RefMut<'a, B, C> {
    type Target = C;

    fn deref(&self) -> &C {
        &self.0
    }
}

impl<'a, B: BackendClasses, C: 'static> DerefMut for RefMut<'a, B, C> {
    fn deref_mut(&mut self) -> &mut C {
        &mut self.0
    }
}

pub struct Class<B: Backend, R = Instance<B>> {
    handle: Handle<B>,
    marker: PhantomData<fn() -> R>,
}

impl<B: Backend, R> Clone for Class<B, R> {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
            marker: PhantomData,
        }
    }
}

impl<B: Backend, R> Class<B, R> {
    pub(crate) fn from_handle(handle: Handle<B>) -> Self {
        Self { handle, marker: PhantomData }
    }

    pub fn with_result<O>(&self) -> Class<B, O> {
        Class::<B, O>::from_handle(self.handle.clone())
    }

    pub fn into_result<O>(self) -> Class<B, O> {
        Class::<B, O>::from_handle(self.handle)
    }
}

impl<B: Backend, R> HasHandle<B> for Class<B, R> {
    fn handle(&self) -> &Handle<B> {
        &self.handle
    }
}

impl<B: Backend, R> IsType<B> for Class<B, R> {}

impl<B, R> Named<B> for Class<B, R> where B: Backend + BackendValues {}

impl<B, R> Annotated<B> for Class<B, R> where B: Backend + BackendValues {}

impl<B> Class<B>
where
    B: Backend + BackendValues + BackendCallables + BackendClasses,
{
    pub fn of<C>(enter: &Enter<'_, B>) -> Result<Class<B, Instance<B, C>>, Error>
    where
        C: HostClass + HostClassDefinition<B>,
    {
        Class::from_guest(enter, ClassSpec::realise_registered::<C>(enter)?)
    }
}

impl<B, R> Class<B, R>
where
    B: Backend + BackendValues,
{
    pub fn construct<A>(&self, args: A) -> Result<R::Owned, Error>
    where
        A: ToGuestArgs<B>,
        R: FromGuest<B>,
    {
        self.construct_as::<A, R>(args)
    }

    pub fn construct_as<A, O>(&self, args: A) -> Result<O::Owned, Error>
    where
        A: ToGuestArgs<B>,
        O: FromGuest<B>,
    {
        self.handle.with_enter(|enter, class| {
            O::from_guest(enter, B::call(enter.token(), class, &args.into_args(enter)?, &[])?)
        })
    }
}

impl<B, R> FromGuest<B> for Class<B, R>
where
    B: Backend + BackendValues,
    R: 'static,
{
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        if !B::is_class(enter.token(), &value) {
            return Err(Error::type_mismatch("class", &B::type_name(enter.token(), &value)));
        }

        Ok(Self::from_handle(Handle::from_value(enter, value)))
    }
}

impl<B: Backend, R> ToGuest<B> for Class<B, R> {
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        Ok(B::attach(enter.token(), self.handle.owned()))
    }
}

pub struct Instance<B: Backend, T = Object<B>> {
    object: Object<B>,
    marker: PhantomData<fn() -> T>,
}

impl<B: Backend, T> Clone for Instance<B, T> {
    fn clone(&self) -> Self {
        Self {
            object: self.object.clone(),
            marker: PhantomData,
        }
    }
}

impl<B: Backend, T> Instance<B, T> {
    fn from_object(object: Object<B>) -> Self {
        Self { object, marker: PhantomData }
    }

    pub fn as_untyped(&self) -> Instance<B> {
        Instance::<B>::from_object(self.object.clone())
    }

    pub fn into_untyped(self) -> Instance<B> {
        Instance::<B>::from_object(self.object)
    }
}

impl<B: Backend, T> HasHandle<B> for Instance<B, T> {
    fn handle(&self) -> &Handle<B> {
        self.object.handle()
    }
}

impl<B: Backend, T> Deref for Instance<B, T> {
    type Target = Object<B>;

    fn deref(&self) -> &Object<B> {
        &self.object
    }
}

impl<B, T> Instance<B, T>
where
    B: Backend + BackendValues + BackendClasses,
{
    pub fn as_typed<C>(&self) -> Result<Instance<B, C>, Error>
    where
        C: HostClass,
    {
        self.object.cast::<Instance<B, C>>()
    }

    pub fn into_typed<C>(self) -> Result<Instance<B, C>, Error>
    where
        C: HostClass,
    {
        self.handle()
            .with_enter(|enter, instance| {
                drop(B::borrow::<C>(enter.token(), instance)?);

                Ok(())
            })?;

        Ok(Instance::<B, C>::from_object(self.object))
    }

    pub fn borrow_as_with<C, F, R>(&self, f: F) -> Result<R, Error>
    where
        C: 'static,
        F: FnOnce(&C) -> R,
    {
        self.handle()
            .with_enter(|enter, instance| Ok(f(&*B::borrow::<C>(enter.token(), instance)?)))
    }

    pub fn borrow_as_with_mut<C, F, R>(&self, f: F) -> Result<R, Error>
    where
        C: 'static,
        F: FnOnce(&mut C) -> R,
    {
        self.handle()
            .with_enter(|enter, instance| Ok(f(&mut *B::borrow_mut::<C>(enter.token(), instance)?)))
    }
}

impl<B, C> Instance<B, C>
where
    B: Backend + BackendValues + BackendClasses,
    C: HostClass,
{
    pub fn borrow_with<F, R>(&self, f: F) -> Result<R, Error>
    where
        F: FnOnce(&C) -> R,
    {
        self.borrow_as_with::<C, F, R>(f)
    }

    pub fn borrow_with_mut<F, R>(&self, f: F) -> Result<R, Error>
    where
        F: FnOnce(&mut C) -> R,
    {
        self.borrow_as_with_mut::<C, F, R>(f)
    }
}

impl<B: Backend> FromGuest<B> for Instance<B> {
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        Ok(Self::from_object(Object::from_guest(enter, value)?))
    }
}

impl<B, C> FromGuest<B> for Instance<B, C>
where
    B: Backend + BackendValues + BackendClasses,
    C: HostClass,
{
    type Owned = Self;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        drop(B::borrow::<C>(enter.token(), &value)?);

        Ok(Self::from_object(Object::from_guest(enter, value)?))
    }
}

impl<B: Backend, T> ToGuest<B> for Instance<B, T> {
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        self.object.to_guest(enter)
    }
}

impl<'py, B, C> FromGuestRef<'py, B> for C
where
    B: Backend + BackendValues + BackendClasses,
    C: HostClass,
{
    type Ref<'a>
        = Ref<'a, B, C>
    where
        C: 'a;

    fn from_guest_ref<'a>(
        enter: &Enter<'py, B>,
        value: &'a B::Value<'py>,
    ) -> Result<Self::Ref<'a>, Error> {
        Ok(Ref(B::borrow::<C>(enter.token(), value)?))
    }
}

impl<'py, B, C> FromGuestMut<'py, B> for C
where
    B: Backend + BackendValues + BackendClasses,
    C: HostClass,
{
    type Mut<'a>
        = RefMut<'a, B, C>
    where
        C: 'a;

    fn from_guest_mut<'a>(
        enter: &Enter<'py, B>,
        value: &'a B::Value<'py>,
    ) -> Result<Self::Mut<'a>, Error> {
        Ok(RefMut(B::borrow_mut::<C>(enter.token(), value)?))
    }
}

impl<B, C> ToGuest<B> for C
where
    B: Backend + BackendValues + BackendClasses,
    C: HostClass,
{
    fn to_guest<'py>(self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        let realised = enter.guest().realisation();
        let payload = TypeId::of::<C>();

        if !realised.class_registered(payload) {
            return Err(Error::unexpected("host class was not registered"));
        }

        let class = realised
            .realised_class(payload)
            .ok_or_else(|| Error::unexpected("host class was not realised"))?;

        B::instantiate::<C>(enter.token(), &B::attach(enter.token(), &class), self)
    }
}

impl<B, C> FromGuest<B> for C
where
    B: Backend + BackendValues + BackendClasses,
    C: HostClass,
{
    type Owned = Instance<B, C>;

    fn from_guest<'py>(enter: &Enter<'py, B>, value: B::Value<'py>) -> Result<Self::Owned, Error> {
        Instance::<B, C>::from_guest(enter, value)
    }
}
