//! RustPython callable operations.

use std::fmt;

use guestpy_core::{
    backend::{
        BackendCallables, Tok, Val,
        callables::{RawBody, RawCall},
    },
    errors::Error,
};
use rustpython_vm::{
    Context, Py, PyObjectRef, PyPayload, PyResult, VirtualMachine,
    builtins::PyType,
    class::{PyClassImpl, StaticType},
    function::FuncArgs,
    pyclass,
    types::{Callable, GetDescriptor, Representable},
};

use crate::{engine::RustPython, errors::NativeErrors};

trait Invoke {
    fn invoke(
        &self,
        receiver: Option<PyObjectRef>,
        args: FuncArgs,
        vm: &VirtualMachine,
    ) -> PyResult;
}

impl Invoke for RawBody<RustPython> {
    fn invoke(
        &self,
        receiver: Option<PyObjectRef>,
        args: FuncArgs,
        vm: &VirtualMachine,
    ) -> PyResult {
        let mut positional = Vec::with_capacity(args.args.len() + usize::from(receiver.is_some()));

        positional.extend(receiver);
        positional.extend(args.args);

        self(RawCall {
            token: vm,
            positional,
            keyword: args
                .kwargs
                .into_iter()
                .map(|(k, v)| (k.into_string_lossy(), v))
                .collect(),
        })
        .map_err(|error| RustPython::to_native(vm, error))
    }
}

#[pyclass(module = false, name = "guestpy_bound_method")]
struct BoundHostMethod {
    body: RawBody<RustPython>,
    name: String,
    receiver: PyObjectRef,
}

impl PyPayload for BoundHostMethod {
    fn class(_: &Context) -> &'static Py<PyType> {
        let _ = Self::make_static_type();
        Self::static_type()
    }
}

impl fmt::Debug for BoundHostMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoundHostMethod")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

#[pyclass(with(Callable, Representable))]
impl BoundHostMethod {}

impl Callable for BoundHostMethod {
    type Args = FuncArgs;

    fn call(zelf: &Py<Self>, args: FuncArgs, vm: &VirtualMachine) -> PyResult {
        zelf.body
            .invoke(Some(zelf.receiver.clone()), args, vm)
    }
}

impl Representable for BoundHostMethod {
    fn repr_str(zelf: &Py<Self>, vm: &VirtualMachine) -> PyResult<String> {
        Ok(format!(
            "<bound guestpy method {} of {}>",
            zelf.name,
            zelf.receiver
                .repr(vm)?
                .to_string_lossy(),
        ))
    }
}

#[pyclass(module = false, name = "guestpy_method")]
struct HostMethod {
    body: RawBody<RustPython>,
    name: String,
    doc: Option<String>,
}

impl PyPayload for HostMethod {
    fn class(_: &Context) -> &'static Py<PyType> {
        let _ = Self::make_static_type();
        Self::static_type()
    }
}

impl fmt::Debug for HostMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostMethod")
            .field("name", &self.name)
            .field("doc", &self.doc)
            .finish_non_exhaustive()
    }
}

#[pyclass(with(GetDescriptor, Callable, Representable))]
impl HostMethod {
    #[pygetset]
    fn __name__(&self) -> String {
        self.name.clone()
    }

    #[pygetset]
    fn __doc__(&self) -> Option<String> {
        self.doc.clone()
    }
}

impl GetDescriptor for HostMethod {
    fn descr_get(
        zelf: PyObjectRef,
        obj: Option<PyObjectRef>,
        _: Option<PyObjectRef>,
        vm: &VirtualMachine,
    ) -> PyResult {
        let Some(receiver) = obj else {
            return Ok(zelf);
        };
        let method = zelf.downcast_ref::<Self>().unwrap();

        Ok(BoundHostMethod {
            body: method.body.clone(),
            name: method.name.clone(),
            receiver,
        }
        .into_ref(&vm.ctx)
        .into())
    }
}

impl Callable for HostMethod {
    type Args = FuncArgs;

    fn call(zelf: &Py<Self>, args: FuncArgs, vm: &VirtualMachine) -> PyResult {
        zelf.body.invoke(None, args, vm)
    }
}

impl Representable for HostMethod {
    fn repr_str(zelf: &Py<Self>, _: &VirtualMachine) -> PyResult<String> {
        Ok(format!("<guestpy method {}>", zelf.name))
    }
}

#[pyclass(module = false, name = "guestpy_function")]
pub struct HostFunction {
    body: RawBody<RustPython>,
    name: String,
    doc: Option<String>,
}

impl PyPayload for HostFunction {
    fn class(_: &Context) -> &'static Py<PyType> {
        let _ = Self::make_static_type();
        Self::static_type()
    }
}

impl fmt::Debug for HostFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HostFunction")
            .field("name", &self.name)
            .field("doc", &self.doc)
            .finish_non_exhaustive()
    }
}

#[pyclass(with(Callable, Representable))]
impl HostFunction {
    #[pygetset]
    fn __name__(&self) -> String {
        self.name.clone()
    }

    #[pygetset]
    fn __doc__(&self) -> Option<String> {
        self.doc.clone()
    }
}

impl Callable for HostFunction {
    type Args = FuncArgs;

    fn call(zelf: &Py<Self>, args: FuncArgs, vm: &VirtualMachine) -> PyResult {
        zelf.body.invoke(None, args, vm)
    }
}

impl Representable for HostFunction {
    fn repr_str(zelf: &Py<Self>, _: &VirtualMachine) -> PyResult<String> {
        Ok(format!("<guestpy function {}>", zelf.name))
    }
}

impl BackendCallables for RustPython {
    fn function<'py>(
        vm: Tok<'py, Self>,
        name: &str,
        doc: Option<&str>,
        body: RawBody<Self>,
    ) -> Result<Val<'py, Self>, Error> {
        Ok(HostFunction {
            body,
            name: name.to_owned(),
            doc: doc.map(str::to_owned),
        }
        .into_ref(&vm.ctx)
        .into())
    }

    fn method<'py>(
        vm: Tok<'py, Self>,
        name: &str,
        doc: Option<&str>,
        body: RawBody<Self>,
    ) -> Result<Val<'py, Self>, Error> {
        Ok(HostMethod {
            body,
            name: name.to_owned(),
            doc: doc.map(str::to_owned),
        }
        .into_ref(&vm.ctx)
        .into())
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::RustPython;

    guestpy_core::backend::callables::fixtures::tests!(RustPython);
}
