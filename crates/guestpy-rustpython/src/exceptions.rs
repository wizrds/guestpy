use guestpy_core::backend::{BackendExceptions, Tok, Val};
use rustpython_vm::builtins::PyBaseExceptionRef;

use crate::engine::RustPython;

impl BackendExceptions for RustPython {
    fn traceback<'py>(vm: Tok<'py, Self>, exception: &Val<'py, Self>) -> Option<String> {
        let exception: PyBaseExceptionRef = exception.clone().downcast().ok()?;
        let mut traceback = String::new();

        vm.write_exception(&mut traceback, &exception)
            .ok()?;

        (!traceback.is_empty()).then_some(traceback)
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::RustPython;

    guestpy_core::backend::exceptions::fixtures::tests!(RustPython);
}
