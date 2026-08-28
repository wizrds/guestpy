use std::rc::Rc;

use guestpy_core::{
    backend::{BackendLibrary, Tok, Val},
    errors::Error,
};
use pyo3::{Py, PyAny, Python};

use crate::engine::CPython;

impl BackendLibrary for CPython {
    type NativeModule = Rc<dyn Fn(Python<'_>) -> Result<Py<PyAny>, Error>>;

    fn declare_native<'py>(
        py: Tok<'py, Self>,
        native: &Self::NativeModule,
        _: &str,
    ) -> Result<Val<'py, Self>, Error> {
        native(py).map(|module| module.into_bound(py).into_any())
    }
}
