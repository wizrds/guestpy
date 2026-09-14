use guestpy_core::{
    errors::{Error, GuestException},
    marshal::FromException,
};
use pyo3::{PyErr, Python, exceptions::PySystemError};

use crate::engine::CPython;

pub(crate) trait NativeErrors {
    fn from_native(py: Python<'_>, raw: PyErr) -> GuestException;
    fn to_native(py: Python<'_>, error: Error) -> PyErr;

    fn guest(py: Python<'_>, raw: PyErr) -> Error {
        Error::guest(Self::from_native(py, raw))
    }
}

impl NativeErrors for CPython {
    fn from_native(py: Python<'_>, raw: PyErr) -> GuestException {
        <GuestException as FromException<CPython>>::from_exception(
            py,
            raw.into_value(py)
                .into_bound(py)
                .into_any(),
        )
    }

    fn to_native(py: Python<'_>, error: Error) -> PyErr {
        match error {
            Error::Guest(exception) => exception
                .object::<CPython>()
                .map(|object| PyErr::from_value(object.bind(py)))
                .unwrap_or_else(|| PySystemError::new_err(exception.to_string())),
            error => PySystemError::new_err(error.to_string()),
        }
    }
}
