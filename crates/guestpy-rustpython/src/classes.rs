//! RustPython host-class operations.

use std::{
    cell::{Ref, RefCell, RefMut},
    fmt,
};

use guestpy_core::{
    backend::{BackendClasses, Tok, Val},
    errors::{BorrowKind, Error},
};
use rustpython_vm::{
    builtins::{PyGenericAlias, PyType},
    class::{PyClassImpl, StaticType},
    object::{MaybeTraverse, TraverseFn},
    pyclass, AsObject, Context, Py, PyObjectRef, PyPayload, PyRef,
};

use crate::{engine::RustPython, errors::NativeErrors};

#[pyclass(module = false, name = "_guestpy_object")]
#[derive(Default)]
struct HostBase;

#[pyclass(flags(BASETYPE, HAS_DICT))]
impl HostBase {}

impl PyPayload for HostBase {
    fn class(_: &Context) -> &'static Py<PyType> {
        let _ = Self::make_static_type();

        Self::static_type()
    }
}

struct HostObject<C> {
    payload: RefCell<Option<C>>,
}

impl<C> fmt::Debug for HostObject<C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HostObject")
            .finish_non_exhaustive()
    }
}

impl<C: 'static> MaybeTraverse for HostObject<C> {
    fn try_traverse(&self, _: &mut TraverseFn<'_>) {}
}

impl<C: 'static> PyPayload for HostObject<C> {
    fn class(ctx: &Context) -> &'static Py<PyType> {
        HostBase::class(ctx)
    }
}

impl<C: 'static> HostObject<C> {
    fn of(value: &PyObjectRef) -> Result<&Py<Self>, Error> {
        value
            .downcast_ref::<Self>()
            .ok_or_else(|| Error::type_mismatch("host class instance", &value.class().name()))
    }
}

impl BackendClasses for RustPython {
    type Ref<'a, C: 'static> = Ref<'a, C>;
    type RefMut<'a, C: 'static> = RefMut<'a, C>;

    fn native_base<'py>(vm: Tok<'py, Self>) -> Val<'py, Self> {
        HostBase::class(&vm.ctx)
            .to_owned()
            .into()
    }

    fn alloc<'py, C: 'static>(
        vm: Tok<'py, Self>,
        class: &Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error> {
        let class_type = class
            .clone()
            .downcast::<PyType>()
            .map_err(|_| Error::type_mismatch("type", &class.class().name()))?;

        Ok(PyRef::new_ref(
            HostObject::<C> { payload: RefCell::new(None) },
            class_type,
            Some(vm.ctx.new_dict()),
        )
        .into())
    }

    fn set_payload<'py, C: 'static>(
        _: Tok<'py, Self>,
        instance: &Val<'py, Self>,
        payload: C,
    ) -> Result<(), Error> {
        *HostObject::<C>::of(instance)?
            .payload
            .try_borrow_mut()
            .map_err(|_| Error::Borrow {
                class: "host class",
                kind: BorrowKind::Exclusive,
            })? = Some(payload);

        Ok(())
    }

    fn borrow<'py, 'a, C: 'static>(
        _: Tok<'py, Self>,
        instance: &'a Val<'py, Self>,
    ) -> Result<Self::Ref<'a, C>, Error> {
        let payload = HostObject::<C>::of(instance)?
            .payload
            .try_borrow()
            .map_err(|_| Error::Borrow {
                class: "host class",
                kind: BorrowKind::Shared,
            })?;

        Ref::filter_map(payload, Option::as_ref).map_err(|_| {
            Error::conversion(format!(
                "host class {} has no payload; its __init__ was never called \
                 (did a subclass forget super().__init__()?)",
                instance.class().name(),
            ))
        })
    }

    fn borrow_mut<'py, 'a, C: 'static>(
        _: Tok<'py, Self>,
        instance: &'a Val<'py, Self>,
    ) -> Result<Self::RefMut<'a, C>, Error> {
        let payload = HostObject::<C>::of(instance)?
            .payload
            .try_borrow_mut()
            .map_err(|_| Error::Borrow {
                class: "host class",
                kind: BorrowKind::Exclusive,
            })?;

        RefMut::filter_map(payload, Option::as_mut).map_err(|_| {
            Error::conversion(format!(
                "host class {} has no payload; its __init__ was never called \
                 (did a subclass forget super().__init__()?)",
                instance.class().name(),
            ))
        })
    }

    fn is_host_instance<'py>(vm: Tok<'py, Self>, value: &Val<'py, Self>) -> bool {
        value
            .class()
            .fast_issubclass(HostBase::class(&vm.ctx))
    }

    fn generic_alias<'py>(
        vm: Tok<'py, Self>,
        origin: &Val<'py, Self>,
        arguments: &[Val<'py, Self>],
    ) -> Result<Val<'py, Self>, Error> {
        Ok(
            PyGenericAlias::new(origin.clone(), vm.ctx.new_tuple(arguments.to_vec()), false, vm)
                .map_err(|error| RustPython::guest(vm, error))?
                .into_pyobject(vm),
        )
    }
}

#[cfg(test)]
mod tests {
    use guestpy_core::{
        backend::{Backend, BackendCallables, BackendClasses, BackendValues},
        errors::Error,
        guest::Guest,
        handle::{ObjectProtocol, Value},
        host::{
            class::{ClassBuilder, HostClass, HostClassDefinition},
            dunder::Dunder,
            module::ModuleSpec,
        },
        runtime::Runtime,
    };

    use crate::engine::RustPython;

    guestpy_core::backend::classes::fixtures::tests!(RustPython);

    struct RootVector;

    impl HostClass for RootVector {
        const NAME: &'static str = "RootVector";
    }

    impl<B> HostClassDefinition<B> for RootVector
    where
        B: Backend + BackendValues + BackendCallables + BackendClasses,
    {
        fn build(_: &mut ClassBuilder<B, Self>) {}
    }

    struct BaseVector;

    impl HostClass for BaseVector {
        const NAME: &'static str = "BaseVector";
    }

    impl<B> HostClassDefinition<B> for BaseVector
    where
        B: Backend + BackendValues + BackendCallables + BackendClasses,
    {
        fn build(builder: &mut ClassBuilder<B, Self>) {
            builder.base::<RootVector>();
        }
    }

    struct Vector2 {
        x: f64,
        y: f64,
    }

    impl HostClass for Vector2 {
        const NAME: &'static str = "Vector2";
    }

    impl<B> HostClassDefinition<B> for Vector2
    where
        B: Backend + BackendValues + BackendCallables + BackendClasses,
    {
        fn build(builder: &mut ClassBuilder<B, Self>) {
            builder
                .constructor(|enter, args| {
                    let x = args.required::<f64>(enter, 0, "x")?;
                    let y = args.required::<f64>(enter, 1, "y")?;

                    args.finish()?;

                    Ok(Self { x, y })
                })
                .method("length", |receiver, _, _| {
                    let vector = receiver.payload::<Self>()?;

                    Ok::<_, Error>(vector.x.hypot(vector.y))
                })
                .getter("x", |receiver, _| Ok::<_, Error>(receiver.payload::<Self>()?.x))
                .setter("x", |receiver, _, value: f64| {
                    receiver.payload_mut::<Self>()?.x = value;

                    Ok::<_, Error>(())
                })
                .class_method("kind", |_, _, _| Ok::<_, Error>("vector"))
                .static_method("zero", |_, _| Ok::<_, Error>((0.0_f64, 0.0_f64)))
                .constant("DIMS", 2_i64)
                .method(Dunder::Len, |_, _, _| Ok::<_, Error>(2_i64))
                .method(Dunder::Repr, |receiver, _, _| {
                    let vector = receiver.payload::<Self>()?;

                    Ok::<_, Error>(format!("Vector2({}, {})", vector.x, vector.y))
                })
                .method(Dunder::Eq, |receiver, enter, args| {
                    let other = args.borrow::<Vector2>(enter, 0)?;
                    let vector = receiver.payload::<Self>()?;

                    Ok::<_, Error>(other.x.eq(&vector.x) && other.y.eq(&vector.y))
                })
                .method(Dunder::GetItem, |receiver, enter, args| {
                    let index = args.required::<i64>(enter, 0, "index")?;
                    let vector = receiver.payload::<Self>()?;

                    match index {
                        0 => Ok::<_, Error>(vector.x),
                        1 => Ok::<_, Error>(vector.y),
                        _ => Err(Error::attribute("index")),
                    }
                })
                .base::<BaseVector>();
        }
    }

    struct Geometry;

    impl Geometry {
        fn module(name: &str) -> ModuleSpec<RustPython> {
            ModuleSpec::new(name)
                .class::<Vector2>()
                .expect("Vector2 registers cleanly")
        }

        fn guest() -> (Runtime<RustPython>, Guest<RustPython>) {
            let runtime = Runtime::<RustPython>::builder()
                .bind(Self::module("geometry"))
                .build()
                .unwrap();
            let guest = runtime.guest().build().unwrap();

            guest.exec("import geometry").unwrap();

            (runtime, guest)
        }
    }

    #[test]
    fn constructs_and_calls() {
        let (_, guest) = Geometry::guest();

        assert_eq!(
            guest
                .eval::<f64>("geometry.Vector2(3, 4).length()")
                .unwrap(),
            5.0
        );
        assert_eq!(
            guest
                .eval::<String>("type(geometry.Vector2(3, 4)).__name__")
                .unwrap(),
            "Vector2"
        );
    }

    #[test]
    fn extra_positional_arguments_raise_type_error() {
        let (_, guest) = Geometry::guest();

        let error = guest
            .eval::<f64>("geometry.Vector2(3, 4, 5).length()")
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("expected at most 2 positional arguments"),);
    }

    #[test]
    fn unexpected_keyword_arguments_raise_type_error() {
        let (_, guest) = Geometry::guest();

        let error = guest
            .eval::<f64>("geometry.Vector2(x=3, y=4, z=5).length()")
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("unexpected keyword argument 'z'"),);
    }

    #[test]
    fn exact_arguments_still_construct_successfully() {
        let (_, guest) = Geometry::guest();

        assert_eq!(
            guest
                .eval::<f64>("geometry.Vector2(3, 4).length()")
                .unwrap(),
            5.0,
        );
    }

    #[test]
    fn private_transitive_bases_are_realised() {
        let (_, guest) = Geometry::guest();

        assert_eq!(
            guest
                .eval::<String>("geometry.Vector2.__bases__[0].__name__")
                .unwrap(),
            "BaseVector",
        );
        assert_eq!(
            guest
                .eval::<String>("geometry.Vector2.__mro__[2].__name__")
                .unwrap(),
            "RootVector",
        );
        assert_eq!(
            guest
                .eval::<String>("geometry.Vector2.__mro__[3].__name__")
                .unwrap(),
            "_guestpy_object",
        );
        assert!(!guest
            .eval::<bool>("hasattr(geometry, 'BaseVector')")
            .unwrap());
        assert!(!guest
            .eval::<bool>("hasattr(geometry, 'RootVector')")
            .unwrap());
    }

    #[test]
    fn binds_the_receiver() {
        let (_, guest) = Geometry::guest();

        assert_eq!(
            guest
                .eval::<f64>("geometry.Vector2(3, 4).length()")
                .unwrap(),
            5.0
        );
        assert_eq!(
            guest
                .eval::<f64>("geometry.Vector2.length(geometry.Vector2(3, 4))")
                .unwrap(),
            5.0
        );
    }

    #[test]
    fn properties_read_and_write() {
        let (_, guest) = Geometry::guest();

        guest
            .exec(
                r#"v = geometry.Vector2(3, 4)
v.x = 1"#,
            )
            .unwrap();

        assert_eq!(guest.eval::<f64>("v.x").unwrap(), 1.0);
        assert!(guest.exec("del v.x").is_err());
    }

    #[test]
    fn class_and_static_methods() {
        let (_, guest) = Geometry::guest();

        assert_eq!(
            guest
                .eval::<String>("geometry.Vector2.kind()")
                .unwrap(),
            "vector"
        );
        assert!(guest
            .eval::<bool>("geometry.Vector2.zero() == (0.0, 0.0)")
            .unwrap());
    }

    #[test]
    fn dunders_dispatch() {
        let (_, guest) = Geometry::guest();

        assert_eq!(
            guest
                .eval::<i64>("len(geometry.Vector2(3, 4))")
                .unwrap(),
            2
        );
        assert!(guest
            .eval::<bool>("geometry.Vector2(3, 4) == geometry.Vector2(3, 4)")
            .unwrap());
        assert_eq!(
            guest
                .eval::<f64>("geometry.Vector2(3, 4)[0]")
                .unwrap(),
            3.0
        );
    }

    #[test]
    fn one_type_object_per_runtime() {
        let runtime = Runtime::<RustPython>::builder()
            .bind(Geometry::module("geometry"))
            .build()
            .unwrap();
        let first = runtime.guest().build().unwrap();
        let second = runtime.guest().build().unwrap();

        first
            .exec(
                r#"import geometry
a = geometry.Vector2(3, 4)"#,
            )
            .unwrap();
        second.exec("import geometry").unwrap();

        second
            .globals()
            .unwrap()
            .set_item(
                "a",
                first
                    .globals()
                    .unwrap()
                    .item::<Value<_>, _>("a")
                    .unwrap(),
            )
            .unwrap();

        assert!(second
            .eval::<bool>("isinstance(a, geometry.Vector2)")
            .unwrap());
    }

    #[test]
    fn two_modules_declaring_one_class_share_it() {
        let runtime = Runtime::<RustPython>::builder()
            .bind(Geometry::module("geometry"))
            .bind(Geometry::module("shapes"))
            .build()
            .unwrap();
        let guest = runtime.guest().build().unwrap();

        guest
            .exec(
                r#"import geometry
import shapes"#,
            )
            .unwrap();

        assert!(guest
            .eval::<bool>("geometry.Vector2 is shapes.Vector2")
            .unwrap());
    }
}
