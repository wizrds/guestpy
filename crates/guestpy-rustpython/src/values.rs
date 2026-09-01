//! RustPython value operations.

use guestpy_core::{
    backend::{BackendValues, Step, Tok, Val},
    errors::Error,
};
use rustpython_vm::{
    AsObject, Py, PyObjectRef, PyPayload,
    builtins::{PyBytes, PyDict, PySet, PyStr, PyType},
    function::{FuncArgs, KwArgs},
    protocol::{PyIter, PyIterReturn},
};

use crate::{engine::RustPython, errors::NativeErrors};

pub(crate) trait AsDict {
    fn as_dict(&self) -> Result<&Py<PyDict>, Error>;
}

impl AsDict for PyObjectRef {
    fn as_dict(&self) -> Result<&Py<PyDict>, Error> {
        self.downcast_ref::<PyDict>()
            .ok_or_else(|| Error::type_mismatch("dict", &self.class().name()))
    }
}

impl BackendValues for RustPython {
    fn none<'py>(vm: Tok<'py, Self>) -> Val<'py, Self> {
        vm.ctx.none()
    }

    fn bool<'py>(vm: Tok<'py, Self>, value: bool) -> Val<'py, Self> {
        vm.ctx.new_bool(value).into()
    }

    fn int<'py>(vm: Tok<'py, Self>, value: i64) -> Val<'py, Self> {
        vm.ctx.new_int(value).into()
    }

    fn uint<'py>(vm: Tok<'py, Self>, value: u64) -> Val<'py, Self> {
        vm.ctx.new_int(value).into()
    }

    fn float<'py>(vm: Tok<'py, Self>, value: f64) -> Val<'py, Self> {
        vm.ctx.new_float(value).into()
    }

    fn str<'py>(vm: Tok<'py, Self>, value: &str) -> Val<'py, Self> {
        vm.ctx.new_str(value).into()
    }

    fn bytes<'py>(vm: Tok<'py, Self>, value: &[u8]) -> Val<'py, Self> {
        vm.ctx.new_bytes(value.to_vec()).into()
    }

    fn list<'py>(vm: Tok<'py, Self>, items: Vec<Val<'py, Self>>) -> Result<Val<'py, Self>, Error> {
        Ok(vm.ctx.new_list(items).into())
    }

    fn tuple<'py>(vm: Tok<'py, Self>, items: Vec<Val<'py, Self>>) -> Result<Val<'py, Self>, Error> {
        Ok(vm.ctx.new_tuple(items).into())
    }

    fn dict<'py>(
        vm: Tok<'py, Self>,
        pairs: Vec<(Val<'py, Self>, Val<'py, Self>)>,
    ) -> Result<Val<'py, Self>, Error> {
        let dict = vm.ctx.new_dict();

        for (key, value) in pairs {
            dict.set_item(&*key, value, vm)
                .map_err(|error| RustPython::guest(vm, error))?;
        }

        Ok(dict.into())
    }

    fn set<'py>(vm: Tok<'py, Self>, items: Vec<Val<'py, Self>>) -> Result<Val<'py, Self>, Error> {
        let set = PySet::default().into_ref(&vm.ctx);

        for value in items {
            set.add(value, vm)
                .map_err(|error| RustPython::guest(vm, error))?;
        }

        Ok(set.into())
    }

    fn new_dict<'py>(vm: Tok<'py, Self>) -> Result<Val<'py, Self>, Error> {
        Ok(vm.ctx.new_dict().into())
    }

    fn is_bool<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.class().is(vm.ctx.types.bool_type)
    }

    fn is_int<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.fast_isinstance(vm.ctx.types.int_type)
    }

    fn is_float<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value
            .class()
            .is(vm.ctx.types.float_type)
    }

    fn is_str<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.class().is(vm.ctx.types.str_type)
    }

    fn is_bytes<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value
            .class()
            .is(vm.ctx.types.bytes_type)
    }

    fn is_list<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.class().is(vm.ctx.types.list_type)
    }

    fn is_tuple<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value
            .class()
            .is(vm.ctx.types.tuple_type)
    }

    fn is_dict<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.class().is(vm.ctx.types.dict_type)
    }

    fn is_set<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.class().is(vm.ctx.types.set_type)
    }

    fn is_callable<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.is_callable()
    }

    fn is_class<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.downcast_ref::<PyType>().is_some()
    }

    fn is_instance<'py>(
        vm: Tok<'py, Self>,
        value: &Val<'py, Self>,
        class: &Val<'py, Self>,
    ) -> Result<bool, Error> {
        value
            .is_instance(class, vm)
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn is_subclass<'py>(
        vm: Tok<'py, Self>,
        first: &Val<'py, Self>,
        second: &Val<'py, Self>,
    ) -> Result<bool, Error> {
        first
            .is_subclass(second, vm)
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn is_iterable<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value.class().is(vm.ctx.types.iter_type)
            || value
                .class()
                .has_attr(vm.ctx.intern_str("__iter__"))
    }

    fn is_none<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        vm.is_none(value)
    }

    fn as_bool<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<bool, Error> {
        if value.is(&vm.ctx.true_value) {
            Ok(true)
        } else if value.is(&vm.ctx.false_value) {
            Ok(false)
        } else {
            Err(Error::type_mismatch("bool", &value.class().name()))
        }
    }

    fn as_i64<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<i64, Error> {
        value
            .try_index(vm)
            .and_then(|value| value.try_to_primitive(vm))
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn as_u64<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<u64, Error> {
        value
            .try_index(vm)
            .and_then(|value| value.try_to_primitive(vm))
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn as_f64<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<f64, Error> {
        value
            .try_float(vm)
            .map(|value| value.to_f64())
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn as_str<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<String, Error> {
        let string = value
            .downcast_ref::<PyStr>()
            .ok_or_else(|| Error::type_mismatch("str", &value.class().name()))?;

        string
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| Error::conversion("str is not valid UTF-8"))
    }

    fn as_bytes<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<Vec<u8>, Error> {
        value
            .downcast_ref::<PyBytes>()
            .map(|value| value.as_bytes().to_vec())
            .ok_or_else(|| Error::type_mismatch("bytes", &value.class().name()))
    }

    fn len<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<usize, Error> {
        value
            .length(vm)
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn type_name<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> String {
        value.class().name().to_owned()
    }

    fn identity<'py>(_: Tok<'py, Self>, value: &Val<'py, Self>) -> usize {
        value.get_id()
    }

    fn truthy<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<bool, Error> {
        value
            .clone()
            .is_true(vm)
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn repr<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<String, Error> {
        value
            .repr(vm)
            .map(|value| value.to_string_lossy().into_owned())
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn display<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<String, Error> {
        value
            .str(vm)
            .map(|value| value.to_string_lossy().into_owned())
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn dir<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<Vec<String>, Error> {
        vm.dir(Some(value.clone()))
            .and_then(|values| {
                values
                    .borrow_vec()
                    .iter()
                    .map(|value| {
                        value
                            .downcast_ref::<PyStr>()
                            .map(|value| value.to_string_lossy().into_owned())
                            .ok_or_else(|| vm.new_type_error("dir returned a non-string"))
                    })
                    .collect()
            })
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn get_attr<'py>(
        vm: Tok<'py, Self>,
        value: &Val<'py, Self>,
        name: &str,
    ) -> Result<Val<'py, Self>, Error> {
        value
            .get_attr(&vm.ctx.new_str(name), vm)
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn set_attr<'py>(
        vm: Tok<'py, Self>,
        value: &Val<'py, Self>,
        name: &str,
        attribute: Val<'py, Self>,
    ) -> Result<(), Error> {
        value
            .set_attr(&vm.ctx.new_str(name), attribute, vm)
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn del_attr<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>, name: &str) -> Result<(), Error> {
        value
            .del_attr(&vm.ctx.new_str(name), vm)
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn has_attr<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>, name: &str) -> bool {
        value
            .has_attr(&vm.ctx.new_str(name), vm)
            .unwrap_or(false)
    }

    fn get_item<'py>(
        vm: Tok<'py, Self>,
        value: &Val<'py, Self>,
        key: &Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error> {
        value
            .get_item(&**key, vm)
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn get_item_opt<'py>(
        vm: Tok<'py, Self>,
        value: &Val<'py, Self>,
        key: &Val<'py, Self>,
    ) -> Result<Option<Val<'py, Self>>, Error> {
        if let Some(dict) = value.downcast_ref::<PyDict>() {
            return dict
                .get_item_opt(&**key, vm)
                .map_err(|error| RustPython::guest(vm, error));
        }

        match value.get_item(&**key, vm) {
            Ok(value) => Ok(Some(value)),
            Err(error)
                if error
                    .class()
                    .is(vm.ctx.exceptions.key_error)
                    || error
                        .class()
                        .is(vm.ctx.exceptions.index_error) =>
            {
                Ok(None)
            }
            Err(error) => Err(RustPython::guest(vm, error)),
        }
    }

    fn set_item<'py>(
        vm: Tok<'py, Self>,
        value: &Val<'py, Self>,
        key: Val<'py, Self>,
        item: Val<'py, Self>,
    ) -> Result<(), Error> {
        value
            .set_item(&*key, item, vm)
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn del_item<'py>(
        vm: Tok<'py, Self>,
        value: &Val<'py, Self>,
        key: &Val<'py, Self>,
    ) -> Result<(), Error> {
        value
            .del_item(&**key, vm)
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn copy_dict<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<Val<'py, Self>, Error> {
        Ok(value
            .as_dict()?
            .copy()
            .into_ref(&vm.ctx)
            .into())
    }

    fn call<'py>(
        vm: Tok<'py, Self>,
        callable: &Val<'py, Self>,
        args: &[Val<'py, Self>],
        kwargs: &[(&str, Val<'py, Self>)],
    ) -> Result<Val<'py, Self>, Error> {
        callable
            .call(
                FuncArgs {
                    args: args.to_vec(),
                    kwargs: kwargs
                        .iter()
                        .map(|(name, value)| ((*name).to_owned(), value.clone()))
                        .collect::<KwArgs>(),
                },
                vm,
            )
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn iter<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> Result<Val<'py, Self>, Error> {
        value
            .get_iter(vm)
            .map(Into::into)
            .map_err(|error| RustPython::guest(vm, error))
    }

    fn next<'py>(
        vm: Tok<'py, Self>,
        iterator: &Val<'py, Self>,
    ) -> Result<Option<Val<'py, Self>>, Error> {
        match PyIter::new(iterator.clone())
            .next(vm)
            .map_err(|error| RustPython::guest(vm, error))?
        {
            PyIterReturn::Return(value) => Ok(Some(value)),
            PyIterReturn::StopIteration(_) => Ok(None),
        }
    }

    fn send<'py>(
        vm: Tok<'py, Self>,
        generator: &Val<'py, Self>,
        value: Val<'py, Self>,
    ) -> Result<Step<Val<'py, Self>>, Error> {
        match vm.call_method(generator, "send", (value,)) {
            Ok(yielded) => Ok(Step::Yielded(yielded)),
            Err(error) if error.fast_isinstance(vm.ctx.exceptions.stop_iteration) => {
                Ok(Step::Returned(
                    error
                        .get_arg(0)
                        .unwrap_or_else(|| vm.ctx.none()),
                ))
            }
            Err(error) => Err(RustPython::guest(vm, error)),
        }
    }

    fn throw<'py>(
        vm: Tok<'py, Self>,
        generator: &Val<'py, Self>,
        exception: Val<'py, Self>,
    ) -> Result<Step<Val<'py, Self>>, Error> {
        match vm.call_method(generator, "throw", (exception,)) {
            Ok(yielded) => Ok(Step::Yielded(yielded)),
            Err(error) if error.fast_isinstance(vm.ctx.exceptions.stop_iteration) => {
                Ok(Step::Returned(
                    error
                        .get_arg(0)
                        .unwrap_or_else(|| vm.ctx.none()),
                ))
            }
            Err(error) => Err(RustPython::guest(vm, error)),
        }
    }

    fn close<'py>(vm: Tok<'py, Self>, generator: &Val<'py, Self>) -> Result<(), Error> {
        vm.call_method(generator, "close", ())
            .map(|_| ())
            .map_err(|error| RustPython::guest(vm, error))
    }
}

#[cfg(test)]
mod tests {
    use guestpy_core::{
        backend::{Backend, BackendModules, BackendValues, Step},
        errors::Error,
    };

    use crate::engine::{Config, RustPython};

    guestpy_core::backend::values::fixtures::tests!(RustPython);

    #[test]
    fn send_steps_a_generator() {
        let engine = RustPython::engine(Config::default()).unwrap();

        RustPython::enter(&engine, |vm| {
            let globals = RustPython::new_dict(vm)?;

            RustPython::set_item(
                vm,
                &globals,
                RustPython::str(vm, "__builtins__"),
                RustPython::builtins_dict(vm)?,
            )?;
            RustPython::exec(
                vm,
                r#"
def gen():
    value = yield 1
    yield value
    return 3
"#,
                "<test>",
                &globals,
            )?;

            let generator = RustPython::eval(vm, "gen()", "<test>", &globals)?;

            match RustPython::send(vm, &generator, vm.ctx.none())? {
                Step::Yielded(value) => {
                    assert_eq!(RustPython::as_i64(vm, &value)?, 1);
                }
                Step::Returned(_) => {
                    panic!("generator returned instead of yielding");
                }
            }

            match RustPython::send(vm, &generator, RustPython::int(vm, 2))? {
                Step::Yielded(value) => {
                    assert_eq!(RustPython::as_i64(vm, &value)?, 2);
                }
                Step::Returned(_) => {
                    panic!("generator returned instead of yielding");
                }
            }

            match RustPython::send(vm, &generator, vm.ctx.none())? {
                Step::Yielded(_) => {
                    panic!("generator yielded instead of returning");
                }
                Step::Returned(value) => {
                    assert_eq!(RustPython::as_i64(vm, &value)?, 3);
                }
            }

            Ok::<_, Error>(())
        })
        .unwrap();
    }
}
