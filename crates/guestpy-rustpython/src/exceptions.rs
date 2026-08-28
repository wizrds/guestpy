use guestpy_core::{
    backend::{BackendExceptions, Tok, Val},
    errors::{Error, GuestException},
};
use rustpython_vm::{AsObject, builtins::PyBaseExceptionRef};

use crate::{engine::RustPython, errors::NativeErrors};

impl BackendExceptions for RustPython {
    type Raw = PyBaseExceptionRef;

    fn take_error<'py>(vm: Tok<'py, Self>, raw: Self::Raw) -> GuestException {
        RustPython::from_native(vm, raw)
    }

    fn raise<'py>(vm: Tok<'py, Self>, error: Error) -> Self::Raw {
        RustPython::to_native(vm, error)
    }

    fn exception_object<'py>(vm: Tok<'py, Self>, error: Error) -> Result<Val<'py, Self>, Error> {
        Ok(RustPython::to_native(vm, error).into())
    }

    fn exception_class<'py>(vm: Tok<'py, Self>, name: &str) -> Result<Val<'py, Self>, Error> {
        let class = vm
            .builtins
            .get_attr(&vm.ctx.new_str(name), vm)
            .map_err(|error| RustPython::guest(vm, error))?;

        if class
            .is_subclass(
                vm.ctx
                    .exceptions
                    .base_exception_type
                    .as_object(),
                vm,
            )
            .map_err(|error| RustPython::guest(vm, error))?
        {
            Ok(class)
        } else {
            Err(Error::unexpected(format!("builtins.{name} is not an exception class")))
        }
    }

    fn new_exception_class<'py>(
        vm: Tok<'py, Self>,
        module: &str,
        name: &str,
        base: Option<&Val<'py, Self>>,
    ) -> Result<Val<'py, Self>, Error> {
        Ok(vm
            .ctx
            .new_exception_type(
                module,
                name,
                base.map(|base| vec![base.clone().downcast().unwrap()]),
            )
            .into())
    }
}
