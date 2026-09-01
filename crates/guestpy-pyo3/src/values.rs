use std::ptr;

use guestpy_core::{
    backend::{BackendValues, Step, Tok, Val},
    errors::Error,
};
use pyo3::{
    Bound, IntoPyObject, PyAny, PyErr,
    exceptions::{PyIndexError, PyKeyError, PyStopIteration},
    ffi,
    types::{
        PyAnyMethods, PyBool, PyBytes, PyBytesMethods, PyDict, PyDictMethods, PyFloat, PyInt,
        PyIterator, PyList, PyListMethods, PySet, PyString, PyTuple, PyType, PyTypeMethods,
    },
};

use crate::{engine::CPython, errors::NativeErrors};

pub(crate) trait AsDict<'py> {
    fn as_dict(&self) -> Result<Bound<'py, PyDict>, Error>;
}

impl<'py> AsDict<'py> for Bound<'py, PyAny> {
    fn as_dict(&self) -> Result<Bound<'py, PyDict>, Error> {
        self.cast::<PyDict>()
            .cloned()
            .map_err(|_| Error::conversion("value is not a dict"))
    }
}

impl BackendValues for CPython {
    fn none<'py>(py: Tok<'py, Self>) -> Val<'py, Self> {
        py.None().into_bound(py)
    }

    fn bool<'py>(py: Tok<'py, Self>, value: bool) -> Val<'py, Self> {
        PyBool::new(py, value)
            .to_owned()
            .into_any()
    }

    fn int<'py>(py: Tok<'py, Self>, value: i64) -> Val<'py, Self> {
        value
            .into_pyobject(py)
            .unwrap()
            .into_any()
    }

    fn uint<'py>(py: Tok<'py, Self>, value: u64) -> Val<'py, Self> {
        value
            .into_pyobject(py)
            .unwrap()
            .into_any()
    }

    fn float<'py>(py: Tok<'py, Self>, value: f64) -> Val<'py, Self> {
        PyFloat::new(py, value).into_any()
    }

    fn str<'py>(py: Tok<'py, Self>, value: &str) -> Val<'py, Self> {
        PyString::new(py, value).into_any()
    }

    fn bytes<'py>(py: Tok<'py, Self>, value: &[u8]) -> Val<'py, Self> {
        PyBytes::new(py, value).into_any()
    }

    fn list<'py>(py: Tok<'py, Self>, items: Vec<Val<'py, Self>>) -> Result<Val<'py, Self>, Error> {
        PyList::new(py, items)
            .map(|list| list.into_any())
            .map_err(|error| CPython::guest(py, error))
    }

    fn tuple<'py>(py: Tok<'py, Self>, items: Vec<Val<'py, Self>>) -> Result<Val<'py, Self>, Error> {
        PyTuple::new(py, items)
            .map(|tuple| tuple.into_any())
            .map_err(|error| CPython::guest(py, error))
    }

    fn dict<'py>(
        py: Tok<'py, Self>,
        pairs: Vec<(Val<'py, Self>, Val<'py, Self>)>,
    ) -> Result<Val<'py, Self>, Error> {
        let dict = PyDict::new(py);

        for (key, value) in pairs {
            dict.set_item(key, value)
                .map_err(|error| CPython::guest(py, error))?;
        }

        Ok(dict.into_any())
    }

    fn set<'py>(py: Tok<'py, Self>, items: Vec<Val<'py, Self>>) -> Result<Val<'py, Self>, Error> {
        PySet::new(py, items)
            .map(|set| set.into_any())
            .map_err(|error| CPython::guest(py, error))
    }

    fn new_dict<'py>(py: Tok<'py, Self>) -> Result<Val<'py, Self>, Error> {
        Ok(PyDict::new(py).into_any())
    }

    fn is_bool<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.is_exact_instance_of::<PyBool>()
    }

    fn is_int<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.is_instance_of::<PyInt>()
    }

    fn is_float<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.is_exact_instance_of::<PyFloat>()
    }

    fn is_str<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.is_exact_instance_of::<PyString>()
    }

    fn is_bytes<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.is_exact_instance_of::<PyBytes>()
    }

    fn is_list<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.is_exact_instance_of::<PyList>()
    }

    fn is_tuple<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.is_exact_instance_of::<PyTuple>()
    }

    fn is_dict<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.is_exact_instance_of::<PyDict>()
    }

    fn is_set<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.is_exact_instance_of::<PySet>()
    }

    fn is_callable<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.is_callable()
    }

    fn is_class<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.cast::<PyType>().is_ok()
    }

    fn is_instance<'py>(
        py: Tok<'py, Self>,
        value: &Val<'py, Self>,
        class: &Val<'py, Self>,
    ) -> Result<bool, Error> {
        value
            .is_instance(class)
            .map_err(|error| CPython::guest(py, error))
    }

    fn is_subclass<'py>(
        py: Tok<'py, Self>,
        first: &Val<'py, Self>,
        second: &Val<'py, Self>,
    ) -> Result<bool, Error> {
        first
            .cast::<PyType>()
            .map_err(|_| Error::type_mismatch("type", "object"))?
            .is_subclass(second)
            .map_err(|error| CPython::guest(py, error))
    }

    fn is_iterable<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.is_instance_of::<PyIterator>()
            || value
                .hasattr("__iter__")
                .unwrap_or(false)
    }

    fn is_none<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.is_none()
    }

    fn as_bool<'py>(py: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<bool, Error> {
        value
            .extract()
            .map_err(|error| CPython::guest(py, error))
    }

    fn as_i64<'py>(py: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<i64, Error> {
        value
            .extract()
            .map_err(|error| CPython::guest(py, error))
    }

    fn as_u64<'py>(py: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<u64, Error> {
        value
            .extract()
            .map_err(|error| CPython::guest(py, error))
    }

    fn as_f64<'py>(py: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<f64, Error> {
        value
            .extract()
            .map_err(|error| CPython::guest(py, error))
    }

    fn as_str<'py>(py: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<String, Error> {
        value
            .extract()
            .map_err(|error| CPython::guest(py, error))
    }

    fn as_bytes<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<Vec<u8>, Error> {
        value
            .cast::<PyBytes>()
            .map(|bytes| bytes.as_bytes().to_vec())
            .map_err(|_| Error::type_mismatch("bytes", "object"))
    }

    fn len<'py>(py: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<usize, Error> {
        value
            .len()
            .map_err(|error| CPython::guest(py, error))
    }

    fn type_name<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> String {
        value
            .get_type()
            .name()
            .expect("a type object always has a name")
            .to_string()
    }

    fn identity<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> usize {
        value.as_ptr() as usize
    }

    fn truthy<'py>(py: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<bool, Error> {
        value
            .is_truthy()
            .map_err(|error| CPython::guest(py, error))
    }

    fn repr<'py>(py: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<String, Error> {
        value
            .repr()
            .map(|value| value.to_string())
            .map_err(|error| CPython::guest(py, error))
    }

    fn display<'py>(py: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<String, Error> {
        value
            .str()
            .map(|value| value.to_string())
            .map_err(|error| CPython::guest(py, error))
    }

    fn dir<'py>(py: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<Vec<String>, Error> {
        value
            .dir()
            .map_err(|error| CPython::guest(py, error))?
            .iter()
            .map(|value| {
                value
                    .extract::<String>()
                    .map_err(|error| CPython::guest(py, error))
            })
            .collect()
    }

    fn get_attr<'py>(
        py: Tok<'py, Self>,
        value: &Val<'py, Self>,
        name: &str,
    ) -> Result<Val<'py, Self>, Error> {
        value
            .getattr(name)
            .map_err(|error| CPython::guest(py, error))
    }

    fn set_attr<'py>(
        py: Tok<'py, Self>,
        value: &Val<'py, Self>,
        name: &str,
        attribute: Val<'py, Self>,
    ) -> Result<(), Error> {
        value
            .setattr(name, attribute)
            .map_err(|error| CPython::guest(py, error))
    }

    fn del_attr<'py>(py: Tok<'py, Self>, value: &Val<'py, Self>, name: &str) -> Result<(), Error> {
        value
            .delattr(name)
            .map_err(|error| CPython::guest(py, error))
    }

    fn has_attr<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>, name: &str) -> bool {
        value.hasattr(name).unwrap_or(false)
    }

    fn get_item<'py>(
        py: Tok<'py, Self>,
        value: &Val<'py, Self>,
        key: &Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error> {
        value
            .get_item(key)
            .map_err(|error| CPython::guest(py, error))
    }

    fn get_item_opt<'py>(
        py: Tok<'py, Self>,
        value: &Val<'py, Self>,
        key: &Val<'py, Self>,
    ) -> Result<Option<Val<'py, Self>>, Error> {
        if let Ok(dict) = value.cast::<PyDict>() {
            return dict
                .get_item(key)
                .map_err(|error| CPython::guest(py, error));
        }

        match value.get_item(key) {
            Ok(value) => Ok(Some(value)),
            Err(error)
                if error.is_instance_of::<PyKeyError>(py)
                    || error.is_instance_of::<PyIndexError>(py) =>
            {
                Ok(None)
            }
            Err(error) => Err(CPython::guest(py, error)),
        }
    }

    fn set_item<'py>(
        py: Tok<'py, Self>,
        value: &Val<'py, Self>,
        key: Val<'py, Self>,
        item: Val<'py, Self>,
    ) -> Result<(), Error> {
        value
            .set_item(key, item)
            .map_err(|error| CPython::guest(py, error))
    }

    fn del_item<'py>(
        py: Tok<'py, Self>,
        value: &Val<'py, Self>,
        key: &Val<'py, Self>,
    ) -> Result<(), Error> {
        value
            .del_item(key)
            .map_err(|error| CPython::guest(py, error))
    }

    fn copy_dict<'py>(py: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<Val<'py, Self>, Error> {
        value
            .as_dict()?
            .copy()
            .map(|dict| dict.into_any())
            .map_err(|error| CPython::guest(py, error))
    }

    fn call<'py>(
        py: Tok<'py, Self>,
        callable: &Val<'py, Self>,
        args: &[Val<'py, Self>],
        kwargs: &[(&str, Val<'py, Self>)],
    ) -> Result<Val<'py, Self>, Error> {
        let positional = PyTuple::new(py, args).map_err(|error| CPython::guest(py, error))?;
        let named = PyDict::new(py);

        for (name, value) in kwargs {
            named
                .set_item(name, value)
                .map_err(|error| CPython::guest(py, error))?;
        }

        callable
            .call(positional, Some(&named))
            .map_err(|error| CPython::guest(py, error))
    }

    fn iter<'py>(py: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<Val<'py, Self>, Error> {
        value
            .try_iter()
            .map(|iterator| iterator.into_any())
            .map_err(|error| CPython::guest(py, error))
    }

    fn next<'py>(
        py: Tok<'py, Self>,
        iterator: &Val<'py, Self>,
    ) -> Result<Option<Val<'py, Self>>, Error> {
        match iterator
            .cast::<PyIterator>()
            .map_err(|_| Error::type_mismatch("iterator", "object"))?
            .clone()
            .next()
        {
            Some(Ok(value)) => Ok(Some(value)),
            Some(Err(error)) => Err(CPython::guest(py, error)),
            None => Ok(None),
        }
    }

    fn send<'py>(
        py: Tok<'py, Self>,
        generator: &Val<'py, Self>,
        value: Val<'py, Self>,
    ) -> Result<Step<Val<'py, Self>>, Error> {
        let mut result = ptr::null_mut();

        match unsafe { ffi::PyIter_Send(generator.as_ptr(), value.as_ptr(), &mut result) } {
            ffi::PySendResult::PYGEN_NEXT => {
                Ok(Step::Yielded(unsafe { Bound::from_owned_ptr(py, result) }))
            }
            ffi::PySendResult::PYGEN_RETURN => {
                Ok(Step::Returned(unsafe { Bound::from_owned_ptr(py, result) }))
            }
            _ => Err(CPython::guest(py, PyErr::fetch(py))),
        }
    }

    fn throw<'py>(
        py: Tok<'py, Self>,
        generator: &Val<'py, Self>,
        exception: Val<'py, Self>,
    ) -> Result<Step<Val<'py, Self>>, Error> {
        match generator.call_method1("throw", (exception,)) {
            Ok(value) => Ok(Step::Yielded(value)),
            Err(error) if error.is_instance_of::<PyStopIteration>(py) => Ok(Step::Returned(
                error
                    .value(py)
                    .getattr("value")
                    .unwrap_or_else(|_| py.None().into_bound(py)),
            )),
            Err(error) => Err(CPython::guest(py, error)),
        }
    }

    fn close<'py>(py: Tok<'py, Self>, generator: &Val<'py, Self>) -> Result<(), Error> {
        generator
            .call_method0("close")
            .map(|_| ())
            .map_err(|error| CPython::guest(py, error))
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::CPython;

    guestpy_core::backend::values::fixtures::tests!(CPython);
}
