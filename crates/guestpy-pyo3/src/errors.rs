use std::ffi::CString;

use guestpy_core::errors::{ErasedOwned, Error, GuestException};
use pyo3::{
    Py, PyErr, Python,
    exceptions::{
        PyAttributeError, PyBaseException, PyImportError, PyNotImplementedError, PyOSError,
        PyRuntimeError, PyStopAsyncIteration, PyStopIteration, PySystemError, PyTypeError,
    },
    sync::PyOnceLock,
    types::{PyAnyMethods, PyTracebackMethods, PyType, PyTypeMethods},
};

use crate::engine::{CPython, Object};

static TIMEOUT_ERROR: PyOnceLock<Py<PyType>> = PyOnceLock::new();
static CANCELLED_ERROR: PyOnceLock<Py<PyType>> = PyOnceLock::new();
static INTERRUPTED_ERROR: PyOnceLock<Py<PyType>> = PyOnceLock::new();
static CLOSED_ERROR: PyOnceLock<Py<PyType>> = PyOnceLock::new();

pub(crate) trait NativeErrors {
    fn from_native(py: Python<'_>, raw: PyErr) -> GuestException;
    fn to_native(py: Python<'_>, error: Error) -> PyErr;

    fn guest(py: Python<'_>, raw: PyErr) -> Error {
        Error::guest(Self::from_native(py, raw))
    }
}

impl NativeErrors for CPython {
    fn from_native(py: Python<'_>, raw: PyErr) -> GuestException {
        let ty = raw.get_type(py);
        let value = raw.value(py);
        let type_name = ty
            .name()
            .map(|name| name.to_string())
            .unwrap_or_else(|_| "Exception".to_owned());

        GuestException::new(
            type_name.clone(),
            ty.getattr("__module__")
                .ok()
                .and_then(|module| module.extract::<String>().ok())
                .map(|module| format!("{module}.{type_name}"))
                .unwrap_or_else(|| type_name.clone()),
            value
                .str()
                .map(|message| message.to_string())
                .unwrap_or_else(|_| type_name.clone()),
            value
                .getattr("name")
                .ok()
                .filter(|name| !name.is_none())
                .and_then(|name| name.extract::<String>().ok()),
            ty.getattr("__mro__")
                .ok()
                .and_then(|mro| mro.try_iter().ok())
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .filter_map(|class| {
                    Some(format!(
                        "{}.{}",
                        class
                            .getattr("__module__")
                            .ok()?
                            .extract::<String>()
                            .ok()?,
                        class
                            .getattr("__qualname__")
                            .ok()?
                            .extract::<String>()
                            .ok()?,
                    ))
                })
                .collect(),
            raw.traceback(py)
                .and_then(|traceback| traceback.format().ok()),
            Some(ErasedOwned::new::<CPython>(Object::new(value.clone().unbind().into_any()))),
        )
    }

    fn to_native(py: Python<'_>, error: Error) -> PyErr {
        match error {
            Error::Guest(exception) => exception
                .object::<CPython>()
                .map(|object| PyErr::from_value(object.bind(py)))
                .unwrap_or_else(|| PyRuntimeError::new_err(exception.message().to_owned())),
            Error::Conversion { message, .. } => PyTypeError::new_err(message),
            Error::Import { name, message } => {
                let error = PyImportError::new_err(message);

                error
                    .value(py)
                    .setattr("name", name)
                    .ok();

                error
            }
            Error::Attribute { name } => {
                PyAttributeError::new_err(format!("no attribute named '{name}'"))
            }
            Error::Bundle { message, .. } => {
                let error = PyImportError::new_err(message);

                error
                    .value(py)
                    .setattr("name", "guestpy")
                    .ok();

                error
            }
            Error::AmbiguousBundle { roots } => {
                let error =
                    PyImportError::new_err(format!("bundle has {roots} top-level modules",));

                error
                    .value(py)
                    .setattr("name", "guestpy")
                    .ok();

                error
            }
            Error::NameInUse { name } => {
                let error = PyImportError::new_err(format!(
                    "module {name} is already loaded in this guest",
                ));

                error
                    .value(py)
                    .setattr("name", &name)
                    .ok();

                error
            }
            Error::Borrow { class, kind } => {
                PyRuntimeError::new_err(format!("host class {class} is already borrowed ({kind})",))
            }
            Error::Unsupported { message } => PyNotImplementedError::new_err(message),
            Error::Host(error) => PyRuntimeError::new_err(error.to_string()),
            Error::Timeout => {
                CPython::fatal_error(py, &TIMEOUT_ERROR, "TimeoutError", "execution timed out")
            }
            Error::Cancelled => {
                CPython::fatal_error(py, &CANCELLED_ERROR, "CancelledError", "execution cancelled")
            }
            Error::Interrupted => CPython::fatal_error(
                py,
                &INTERRUPTED_ERROR,
                "InterruptedError",
                "execution interrupted",
            ),
            Error::Closed => {
                CPython::fatal_error(py, &CLOSED_ERROR, "ClosedError", "guest is closed")
            }
            Error::StopIteration => PyStopIteration::new_err(()),
            Error::StopAsyncIteration => PyStopAsyncIteration::new_err(()),
            Error::Io(error) => PyOSError::new_err(error.to_string()),
            Error::Engine { message, .. } | Error::Unexpected { message, .. } => {
                PySystemError::new_err(message)
            }
        }
    }
}

impl CPython {
    fn fatal_error(
        py: Python<'_>,
        cell: &'static PyOnceLock<Py<PyType>>,
        name: &str,
        message: &str,
    ) -> PyErr {
        PyErr::from_type(
            cell.get_or_init(py, || {
                PyErr::new_type(
                    py,
                    &CString::new(format!("guestpy.{name}"))
                        .expect("a guestpy exception name never contains a NUL byte"),
                    None,
                    Some(&py.get_type::<PyBaseException>()),
                    None,
                )
                .expect("guestpy's own fatal exception classes are always constructible")
            })
            .bind(py)
            .clone(),
            (message.to_owned(),),
        )
    }
}
