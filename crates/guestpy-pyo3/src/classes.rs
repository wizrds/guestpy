use std::{
    any::Any,
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use guestpy_core::{
    backend::{BackendClasses, Tok, Val},
    errors::{BorrowKind, Error},
};
use pyo3::{
    Bound, PyRef, PyRefMut, pyclass, pymethods,
    types::{PyAnyMethods, PyGenericAlias, PyTuple, PyType, PyTypeMethods},
};

use crate::{engine::CPython, errors::NativeErrors, marker::GilSerialized};

#[pyclass(module = "guestpy", name = "_guestpy_object", subclass)]
#[derive(Default)]
pub struct HostObject {
    payload: Option<GilSerialized<Box<dyn Any>>>,
}

#[pymethods]
impl HostObject {
    #[new]
    fn new() -> Self {
        Self::default()
    }
}

impl HostObject {
    fn of<'py>(value: &Val<'py, CPython>) -> Result<Bound<'py, Self>, Error> {
        value
            .cast::<Self>().cloned()
            .map_err(|_| Error::type_mismatch("host class instance", "object"))
    }

    fn missing_payload(value: &Val<'_, CPython>) -> Error {
        Error::conversion(format!(
            "host class {} has no payload; its __init__ was never called \
             (did a subclass forget super().__init__()?)",
            value
                .get_type()
                .name()
                .map(|name| name.to_string())
                .unwrap_or_default(),
        ))
    }
}

pub struct PayloadRef<'py, C> {
    guard: PyRef<'py, HostObject>,
    marker: PhantomData<fn() -> C>,
}

impl<'py, C: 'static> Deref for PayloadRef<'py, C> {
    type Target = C;

    fn deref(&self) -> &C {
        self.guard
            .payload
            .as_ref()
            .and_then(|payload| payload.downcast_ref::<C>())
            .expect("payload was validated as C in BackendClasses::borrow")
    }
}

pub struct PayloadRefMut<'py, C> {
    guard: PyRefMut<'py, HostObject>,
    marker: PhantomData<fn() -> C>,
}

impl<'py, C: 'static> Deref for PayloadRefMut<'py, C> {
    type Target = C;

    fn deref(&self) -> &C {
        self.guard
            .payload
            .as_ref()
            .and_then(|payload| payload.downcast_ref::<C>())
            .expect("payload was validated as C in BackendClasses::borrow_mut")
    }
}

impl<'py, C: 'static> DerefMut for PayloadRefMut<'py, C> {
    fn deref_mut(&mut self) -> &mut C {
        self.guard
            .payload
            .as_mut()
            .and_then(|payload| payload.downcast_mut::<C>())
            .expect("payload was validated as C in BackendClasses::borrow_mut")
    }
}

impl BackendClasses for CPython {
    type Ref<'a, C: 'static> = PayloadRef<'a, C>;
    type RefMut<'a, C: 'static> = PayloadRefMut<'a, C>;

    fn new_class<'py>(
        py: Tok<'py, Self>,
        name: &str,
        bases: &[Val<'py, Self>],
        namespace: &Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error> {
        py.get_type::<PyType>()
            .call1((
                name,
                PyTuple::new(
                    py,
                    if bases.is_empty() {
                        vec![py.get_type::<HostObject>().into_any()]
                    } else {
                        bases.to_vec()
                    },
                )
                .map_err(|error| CPython::guest(py, error))?,
                namespace,
            ))
            .map_err(|error| CPython::guest(py, error))
    }

    fn alloc<'py, C: 'static>(
        py: Tok<'py, Self>,
        class: &Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error> {
        py.get_type::<HostObject>()
            .call_method1("__new__", (class,)) // _guestpy_object.__new__(Vector2) — cooperative super().__new__(cls)
            .map(|instance| instance.into_any())
            .map_err(|error| CPython::guest(py, error))
    }

    fn set_payload<'py, C: 'static>(
        _: Tok<'py, Self>,
        instance: &Val<'py, Self>,
        payload: C,
    ) -> Result<(), Error> {
        let mut guard =
            PyRefMut::try_from(&HostObject::of(instance)?).map_err(|_| Error::Borrow {
                class: "host class",
                kind: BorrowKind::Exclusive,
            })?;

        guard.payload = Some(GilSerialized::new(Box::new(payload)));

        Ok(())
    }

    fn borrow<'py, 'a, C: 'static>(
        _: Tok<'py, Self>,
        instance: &'a Val<'py, Self>,
    ) -> Result<Self::Ref<'a, C>, Error> {
        let guard = PyRef::try_from(&HostObject::of(instance)?).map_err(|_| Error::Borrow {
            class: "host class",
            kind: BorrowKind::Shared,
        })?;

        if !guard
            .payload
            .as_ref()
            .is_some_and(|payload| payload.is::<C>())
        {
            return Err(HostObject::missing_payload(instance));
        }

        Ok(PayloadRef { guard, marker: PhantomData })
    }

    fn borrow_mut<'py, 'a, C: 'static>(
        _: Tok<'py, Self>,
        instance: &'a Val<'py, Self>,
    ) -> Result<Self::RefMut<'a, C>, Error> {
        let guard = PyRefMut::try_from(&HostObject::of(instance)?).map_err(|_| Error::Borrow {
            class: "host class",
            kind: BorrowKind::Exclusive,
        })?;

        if !guard
            .payload
            .as_ref()
            .is_some_and(|payload| payload.is::<C>())
        {
            return Err(HostObject::missing_payload(instance));
        }

        Ok(PayloadRefMut { guard, marker: PhantomData })
    }

    fn is_host_instance<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.is_instance_of::<HostObject>()
    }

    fn generic_alias<'py>(
        py: Tok<'py, Self>,
        origin: &Val<'py, Self>,
        arguments: &[Val<'py, Self>],
    ) -> Result<Val<'py, Self>, Error> {
        Ok(PyGenericAlias::new(
            py,
            origin,
            &PyTuple::new(py, arguments.to_vec())
                .map_err(|error| CPython::guest(py, error))?
                .into_any(),
        )
        .map_err(|error| CPython::guest(py, error))?
        .into_any())
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::CPython;

    guestpy_core::backend::classes::fixtures::tests!(CPython);
}
