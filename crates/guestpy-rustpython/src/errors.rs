//! RustPython error conversion.

use guestpy_core::{
    errors::{Error, GuestException},
    marshal::FromException,
};
use rustpython_vm::{VirtualMachine, builtins::PyBaseExceptionRef};

use crate::engine::RustPython;

pub(crate) trait NativeErrors {
    fn from_native(vm: &VirtualMachine, raw: PyBaseExceptionRef) -> GuestException;
    fn to_native(vm: &VirtualMachine, error: Error) -> PyBaseExceptionRef;

    fn guest(vm: &VirtualMachine, raw: PyBaseExceptionRef) -> Error {
        Error::guest(Self::from_native(vm, raw))
    }
}

impl NativeErrors for RustPython {
    fn from_native(vm: &VirtualMachine, raw: PyBaseExceptionRef) -> GuestException {
        <GuestException as FromException<RustPython>>::from_exception(vm, raw.into())
    }

    fn to_native(vm: &VirtualMachine, error: Error) -> PyBaseExceptionRef {
        match error {
            Error::Guest(exception) => exception
                .object::<RustPython>()
                .and_then(|object| object.clone().downcast().ok())
                .unwrap_or_else(|| vm.new_system_error(exception.to_string())),
            error => vm.new_system_error(error.to_string()),
        }
    }
}
