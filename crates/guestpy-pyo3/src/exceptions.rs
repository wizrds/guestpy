use std::ffi::CString;

use guestpy_core::{
    backend::{BackendExceptions, Tok, Val},
    errors::{Error, GuestException},
};
use pyo3::{
    PyErr, PyTypeInfo,
    exceptions::PyBaseException,
    types::{PyAnyMethods, PyType, PyTypeMethods},
};

use crate::{engine::CPython, errors::NativeErrors};

impl BackendExceptions for CPython {
    type Raw = PyErr;

    fn take_error<'py>(py: Tok<'py, Self>, raw: Self::Raw) -> GuestException {
        CPython::from_native(py, raw)
    }

    fn raise<'py>(py: Tok<'py, Self>, error: Error) -> Self::Raw {
        CPython::to_native(py, error)
    }

    fn exception_object<'py>(py: Tok<'py, Self>, error: Error) -> Result<Val<'py, Self>, Error> {
        Ok(CPython::to_native(py, error)
            .value(py)
            .clone()
            .into_any())
    }

    fn exception_class<'py>(py: Tok<'py, Self>, name: &str) -> Result<Val<'py, Self>, Error> {
        let class = py
            .import("builtins")
            .and_then(|builtins| builtins.getattr(name))
            .map_err(|error| CPython::guest(py, error))?;

        if class
            .cast::<PyType>()
            .map_err(|_| Error::unexpected(format!("builtins.{name} is not a type")))?
            .is_subclass(&PyBaseException::type_object(py))
            .map_err(|error| CPython::guest(py, error))?
        {
            Ok(class)
        } else {
            Err(Error::unexpected(format!("builtins.{name} is not an exception class",)))
        }
    }

    fn new_exception_class<'py>(
        py: Tok<'py, Self>,
        module: &str,
        name: &str,
        base: Option<&Val<'py, Self>>,
    ) -> Result<Val<'py, Self>, Error> {
        PyErr::new_type(
            py,
            &CString::new(format!("{module}.{name}")).map_err(|error| {
                Error::sourced_conversion("exception name contains a NUL byte", error)
            })?,
            None,
            base.map(|base| base.cast::<PyType>().cloned())
                .transpose()
                .map_err(|_| Error::type_mismatch("type", "object"))?
                .as_ref(),
            None,
        )
        .map(|class| class.into_bound(py).into_any())
        .map_err(|error| CPython::guest(py, error))
    }
}
