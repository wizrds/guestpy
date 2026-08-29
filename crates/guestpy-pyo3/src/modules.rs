use std::ffi::CString;

use guestpy_core::{
    backend::{BackendModules, Tok, Val},
    errors::Error,
};
use pyo3::{
    Bound, ffi,
    types::{PyAnyMethods, PyCode, PyCodeInput, PyDictMethods, PyModule, PyModuleMethods},
};

use crate::{
    engine::{CPython, Engine},
    errors::NativeErrors,
    values::AsDict,
};

impl CPython {
    fn compile_as<'py>(
        py: Tok<'py, Self>,
        source: &str,
        filename: &str,
        mode: PyCodeInput,
    ) -> Result<Val<'py, Self>, Error> {
        PyCode::compile(
            py,
            &CString::new(source)
                .map_err(|error| Error::sourced_conversion("source contains a NUL byte", error))?,
            &CString::new(filename).map_err(|error| {
                Error::sourced_conversion("filename contains a NUL byte", error)
            })?,
            mode,
        )
        .map(|code| code.into_any())
        .map_err(|error| CPython::guest(py, error))
    }

    fn eval_code<'py>(
        py: Tok<'py, Self>,
        code: &Val<'py, Self>,
        globals: &Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error> {
        let globals = globals.as_dict()?;

        unsafe {
            Bound::from_owned_ptr_or_err(
                py,
                ffi::PyEval_EvalCode(
                    code.cast::<PyCode>()
                        .map_err(|_| Error::type_mismatch("code object", "object"))?
                        .as_ptr(),
                    globals.as_ptr(),
                    globals.as_ptr(),
                ),
            )
        }
        .map_err(|error| CPython::guest(py, error))
    }
}

impl BackendModules for CPython {
    fn new_module<'py>(
        py: Tok<'py, Self>,
        name: &str,
        dict: Val<'py, Self>,
        doc: Option<&str>,
    ) -> Result<Val<'py, Self>, Error> {
        let module = PyModule::new(py, name).map_err(|error| CPython::guest(py, error))?;

        module
            .dict()
            .update(dict.as_dict()?.as_mapping())
            .map_err(|error| CPython::guest(py, error))?;

        if let Some(doc) = doc {
            module
                .setattr("__doc__", doc)
                .map_err(|error| CPython::guest(py, error))?;
        }

        Ok(module.into_any())
    }

    fn compile<'py>(
        py: Tok<'py, Self>,
        source: &str,
        filename: &str,
    ) -> Result<Val<'py, Self>, Error> {
        CPython::compile_as(py, source, filename, PyCodeInput::File)
    }

    fn exec_code<'py>(
        py: Tok<'py, Self>,
        code: &Val<'py, Self>,
        globals: &Val<'py, Self>,
    ) -> Result<(), Error> {
        CPython::eval_code(py, code, globals).map(|_| ())
    }

    fn eval<'py>(
        py: Tok<'py, Self>,
        source: &str,
        filename: &str,
        globals: &Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error> {
        CPython::eval_code(
            py,
            &CPython::compile_as(py, source, filename, PyCodeInput::Eval)?,
            globals,
        )
    }

    fn builtins_dict<'py>(py: Tok<'py, Self>) -> Result<Val<'py, Self>, Error> {
        py.import("builtins")
            .map(|builtins| builtins.dict().into_any())
            .map_err(|error| CPython::guest(py, error))
    }

    fn install_dispatcher<'py>(
        _: Tok<'py, Self>,
        _: &Engine,
        _: Val<'py, Self>,
    ) -> Result<(), Error> {
        Ok(())
    }

    fn real_import<'py>(py: Tok<'py, Self>) -> Result<Val<'py, Self>, Error> {
        py.import("builtins")
            .and_then(|builtins| builtins.getattr("__import__"))
            .map_err(|error| CPython::guest(py, error))
    }
}

#[cfg(test)]
mod tests {
    use guestpy_core::{
        bundle::Bundle,
        errors::{Error, GuestException},
        runtime::Runtime,
    };

    use crate::engine::CPython;

    struct Raises;

    impl Raises {
        fn guest(error: Error) -> GuestException {
            match error {
                Error::Guest(exception) => *exception,
                other => {
                    panic!("expected a guest exception, got: {other}")
                }
            }
        }
    }

    #[test]
    fn failed_import_does_not_cache_a_partial_module() {
        let guest = Runtime::<CPython>::builder()
            .bundle(
                Bundle::single(
                    "broken",
                    r#"
raise RuntimeError("broken import")
"#,
                )
                .unwrap(),
            )
            .build()
            .unwrap()
            .guest()
            .build()
            .unwrap();

        for _ in 0..2 {
            let error = Raises::guest(guest.exec("import broken").unwrap_err());

            assert!(error.matches("RuntimeError"));
            assert!(
                error
                    .message()
                    .contains("broken import")
            );
        }
    }
}
