use guestpy_core::backend::{BackendExceptions, Tok, Val};
use pyo3::types::{PyAnyMethods, PyTraceback, PyTracebackMethods};

use crate::engine::CPython;

impl BackendExceptions for CPython {
    fn traceback<'py>(_: Tok<'py, Self>, exception: &Val<'py, Self>) -> Option<String> {
        exception
            .getattr("__traceback__")
            .ok()
            .filter(|traceback| !traceback.is_none())
            .and_then(|traceback| {
                traceback
                    .cast::<PyTraceback>()
                    .ok()
                    .and_then(|traceback| traceback.format().ok())
            })
    }
}
