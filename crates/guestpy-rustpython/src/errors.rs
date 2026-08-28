//! RustPython error conversion.

use guestpy_core::errors::{ErasedOwned, Error, GuestException};
use rustpython_vm::{
    AsObject, VirtualMachine,
    builtins::{PyBaseException, PyBaseExceptionRef},
};

use crate::engine::RustPython;

pub(crate) trait NativeErrors {
    fn from_native(vm: &VirtualMachine, raw: PyBaseExceptionRef) -> GuestException;
    fn to_native(vm: &VirtualMachine, error: Error) -> PyBaseExceptionRef;

    fn guest(vm: &VirtualMachine, raw: PyBaseExceptionRef) -> Error {
        Error::Guest(Self::from_native(vm, raw))
    }
}

impl NativeErrors for RustPython {
    fn from_native(vm: &VirtualMachine, raw: PyBaseExceptionRef) -> GuestException {
        let mut traceback_buf = String::new();
        let type_name = raw.class().name().to_owned();
        let module = raw
            .class()
            .as_object()
            .get_attr("__module__", vm)
            .ok()
            .and_then(|module| module.str(vm).ok())
            .map(|module| module.to_string_lossy().into_owned());

        GuestException::new(
            type_name.clone(),
            module
                .map(|module| format!("{module}.{type_name}"))
                .unwrap_or_else(|| type_name.clone()),
            raw.as_object()
                .str(vm)
                .map(|message| message.to_string_lossy().into_owned())
                .unwrap_or_else(|_| type_name.clone()),
            raw.as_object()
                .get_attr("name", vm)
                .ok()
                .filter(|name| !vm.is_none(name))
                .and_then(|name| name.str(vm).ok())
                .map(|name| name.to_string_lossy().into_owned()),
            raw.class()
                .mro_collect()
                .into_iter()
                .map(|class| {
                    class
                        .as_object()
                        .get_attr("__module__", vm)
                        .ok()
                        .and_then(|module| module.str(vm).ok())
                        .map(|module| format!("{}.{}", module.to_string_lossy(), class.name()))
                        .unwrap_or_else(|| class.name().to_owned())
                })
                .collect(),
            vm.write_exception(&mut traceback_buf, &raw)
                .ok()
                .map(|()| traceback_buf)
                .filter(|traceback| !traceback.is_empty()),
            Some(ErasedOwned::new::<RustPython>(raw.into())),
        )
    }

    fn to_native(vm: &VirtualMachine, error: Error) -> PyBaseExceptionRef {
        match error {
            Error::Guest(exception) => exception
                .object::<RustPython>()
                .and_then(|object| object.clone().downcast().ok())
                .unwrap_or_else(|| vm.new_runtime_error(exception.message())),
            Error::Conversion { message, .. } => vm.new_type_error(message),
            Error::Import { name, message } => vm.new_import_error(message, vm.ctx.new_str(name)),
            Error::Attribute { name } => {
                vm.new_attribute_error(format!("no attribute named '{name}'"))
            }
            Error::Bundle { message, .. } => {
                vm.new_import_error(message, vm.ctx.new_str("guestpy"))
            }
            Error::AmbiguousBundle { roots } => vm.new_import_error(
                format!("bundle has {roots} top-level modules"),
                vm.ctx.new_str("guestpy"),
            ),
            Error::NameInUse { name } => vm.new_import_error(
                format!("module {name} is already loaded in this guest"),
                vm.ctx.new_str(name),
            ),
            Error::Borrow { class, kind } => {
                vm.new_runtime_error(format!("host class {class} is already borrowed ({kind})"))
            }
            Error::Unsupported { message } => vm.new_not_implemented_error(message),
            Error::Host(error) => vm.new_runtime_error(error.to_string()),
            Error::Timeout => Self::fatal(vm, "TimeoutError", "execution timed out"),
            Error::Cancelled => Self::fatal(vm, "CancelledError", "execution cancelled"),
            Error::Interrupted => Self::fatal(vm, "InterruptedError", "execution interrupted"),
            Error::Closed => Self::fatal(vm, "ClosedError", "guest is closed"),
            Error::StopIteration => vm.new_stop_iteration(None),
            Error::StopAsyncIteration => vm.new_exception_empty(
                vm.ctx
                    .exceptions
                    .stop_async_iteration
                    .to_owned(),
            ),
            Error::Io(error) => vm.new_os_error(error.to_string()),
            Error::Engine { message, .. } | Error::Unexpected { message, .. } => {
                vm.new_system_error(message)
            }
        }
    }
}

impl RustPython {
    fn fatal(vm: &VirtualMachine, name: &str, message: &str) -> PyBaseExceptionRef {
        vm.ctx
            .new_exception_type(
                "guestpy",
                name,
                Some(vec![
                    vm.ctx
                        .exceptions
                        .base_exception_type
                        .to_owned(),
                ]),
            )
            .as_object()
            .call((message,), vm)
            .unwrap()
            .downcast::<PyBaseException>()
            .unwrap()
    }
}
