use guestpy_core::{
    backend::{
        BackendCallables, Tok, Val,
        callables::{RawBody, RawCall},
    },
    errors::Error,
};
use pyo3::{
    Bound, Py, PyAny, PyResult, Python, pyclass, pymethods,
    types::{PyAnyMethods, PyDict, PyDictMethods, PyTuple, PyTupleMethods},
};

use crate::{engine::CPython, errors::NativeErrors, marker::GilSerialized};

trait Invoke {
    fn invoke(
        &self,
        receiver: Option<Bound<'_, PyAny>>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>>;
}

impl Invoke for RawBody<CPython> {
    fn invoke(
        &self,
        receiver: Option<Bound<'_, PyAny>>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        let py = args.py();
        let mut positional = Vec::with_capacity(args.len() + usize::from(receiver.is_some()));

        positional.extend(receiver);
        positional.extend(args.iter());

        self(RawCall {
            token: py,
            positional,
            keyword: kwargs
                .map(|kwargs| {
                    kwargs
                        .iter()
                        .map(|(name, value)| Ok((name.extract::<String>()?, value)))
                        .collect::<PyResult<Vec<_>>>()
                })
                .transpose()?
                .unwrap_or_default(),
        })
        .map(|value| value.unbind())
        .map_err(|error| CPython::to_native(py, error))
    }
}

#[pyclass(module = "guestpy", name = "guestpy_bound_method")]
struct BoundHostMethod {
    body: GilSerialized<RawBody<CPython>>,
    name: String,
    receiver: Py<PyAny>,
}

#[pymethods]
impl BoundHostMethod {
    #[pyo3(signature = (*args, **kwargs))]
    fn __call__(
        &self,
        py: Python<'_>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        self.body
            .invoke(Some(self.receiver.bind(py).clone()), args, kwargs)
    }

    #[getter]
    fn __name__(&self) -> String {
        self.name.clone()
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "<bound guestpy method {} of {}>",
            self.name,
            self.receiver.bind(py).repr()?,
        ))
    }
}

#[pyclass(module = "guestpy", name = "guestpy_method")]
struct HostMethod {
    body: GilSerialized<RawBody<CPython>>,
    name: String,
    doc: Option<String>,
}

#[pymethods]
impl HostMethod {
    fn __get__(
        slf: Bound<'_, Self>,
        obj: Option<Bound<'_, PyAny>>,
        _objtype: Option<Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let Some(receiver) = obj.filter(|obj| !obj.is_none()) else {
            return Ok(slf.into_any().unbind());
        };

        let this = slf.borrow();

        Ok(Bound::new(
            slf.py(),
            BoundHostMethod {
                body: this.body.clone().into(),
                name: this.name.clone(),
                receiver: receiver.unbind(),
            },
        )?
        .into_any()
        .unbind())
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn __call__(
        &self,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        self.body.invoke(None, args, kwargs)
    }

    #[getter]
    fn __name__(&self) -> String {
        self.name.clone()
    }

    #[getter]
    fn __doc__(&self) -> Option<String> {
        self.doc.clone()
    }

    fn __repr__(&self) -> String {
        format!("<guestpy method {}>", self.name)
    }
}

#[pyclass(module = "guestpy", name = "guestpy_function")]
pub struct HostFunction {
    body: GilSerialized<RawBody<CPython>>,
    name: String,
    doc: Option<String>,
}

#[pymethods]
impl HostFunction {
    #[pyo3(signature = (*args, **kwargs))]
    fn __call__(
        &self,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        self.body.invoke(None, args, kwargs)
    }

    #[getter]
    fn __name__(&self) -> String {
        self.name.clone()
    }

    #[getter]
    fn __doc__(&self) -> Option<String> {
        self.doc.clone()
    }

    fn __repr__(&self) -> String {
        format!("<guestpy function {}>", self.name)
    }
}

impl BackendCallables for CPython {
    fn function<'py>(
        py: Tok<'py, Self>,
        name: &str,
        doc: Option<&str>,
        body: RawBody<Self>,
    ) -> Result<Val<'py, Self>, Error> {
        Bound::new(
            py,
            HostFunction {
                body: body.into(),
                name: name.to_owned(),
                doc: doc.map(str::to_owned),
            },
        )
        .map(|function| function.into_any())
        .map_err(|error| CPython::guest(py, error))
    }

    fn method<'py>(
        py: Tok<'py, Self>,
        name: &str,
        doc: Option<&str>,
        body: RawBody<Self>,
    ) -> Result<Val<'py, Self>, Error> {
        Bound::new(
            py,
            HostMethod {
                body: body.into(),
                name: name.to_owned(),
                doc: doc.map(str::to_owned),
            },
        )
        .map(|method| method.into_any())
        .map_err(|error| CPython::guest(py, error))
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::CPython;

    guestpy_core::backend::callables::fixtures::tests!(CPython);
}
