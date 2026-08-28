use guestpy_core::{
    backend::{BackendCoroutines, Tok, Val},
    errors::Error,
};
use pyo3::types::PyAnyMethods;

use crate::{engine::CPython, errors::NativeErrors};

impl BackendCoroutines for CPython {
    fn is_coroutine<'py>(py: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        py.import("types")
            .and_then(|types| types.getattr("CoroutineType"))
            .and_then(|coroutine_type| value.is_instance(&coroutine_type))
            .unwrap_or(false)
    }

    fn is_awaitable<'py>(py: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        CPython::is_coroutine(py, value)
            || value
                .hasattr("__await__")
                .unwrap_or(false)
    }

    fn anext<'py>(
        py: Tok<'py, Self>,
        async_iterator: &Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error> {
        async_iterator
            .call_method0("__anext__")
            .map_err(|error| CPython::guest(py, error))
    }

    fn asend<'py>(
        py: Tok<'py, Self>,
        async_generator: &Val<'py, Self>,
        value: Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error> {
        async_generator
            .call_method1("asend", (value,))
            .map_err(|error| CPython::guest(py, error))
    }

    fn athrow<'py>(
        py: Tok<'py, Self>,
        async_generator: &Val<'py, Self>,
        exception: Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error> {
        async_generator
            .call_method1("athrow", (exception,))
            .map_err(|error| CPython::guest(py, error))
    }

    fn aclose<'py>(
        py: Tok<'py, Self>,
        async_generator: &Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error> {
        async_generator
            .call_method0("aclose")
            .map_err(|error| CPython::guest(py, error))
    }

    fn set_running_loop<'py>(
        py: Tok<'py, Self>,
        asyncio_loop: Option<&Val<'py, Self>>,
    ) -> Result<(), Error> {
        py.import("_asyncio")
            .and_then(|module| {
                module.call_method1(
                    "_set_running_loop",
                    (asyncio_loop
                        .cloned()
                        .unwrap_or_else(|| py.None().into_bound(py)),),
                )
            })
            .map(|_| ())
            .map_err(|error| CPython::guest(py, error))
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::CPython;

    guestpy_core::backend::coroutines::fixtures::tests!(CPython);
}
