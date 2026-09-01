//! The shared vocabulary for reaching into a guest object.

use crate::{
    backend::{Backend, BackendValues},
    errors::Error,
    handle::{Class, Function, Handle, Iter, Object, Value},
    marshal::{
        FromGuest, ToGuest,
        args::{ToGuestArgs, ToGuestKwargs},
    },
};

mod sealed {
    use crate::{backend::Backend, handle::Handle};

    pub trait HasHandle<B: Backend> {
        fn handle(&self) -> &Handle<B>;
    }

    pub trait IsType<B: Backend>: HasHandle<B> {}
}

pub(crate) use sealed::{HasHandle, IsType};

pub trait ObjectProtocol<B>
where
    B: Backend + BackendValues,
    Self: HasHandle<B>,
{
    fn get<T: FromGuest<B>>(&self, name: &str) -> Result<T::Owned, Error> {
        self.handle().with_enter(|enter, object| {
            T::from_guest(enter, B::get_attr(enter.token(), object, name)?)
        })
    }

    fn set<T: ToGuest<B>>(&self, name: &str, value: T) -> Result<(), Error> {
        self.handle().with_enter(|enter, object| {
            B::set_attr(enter.token(), object, name, value.to_guest(enter)?)
        })
    }

    fn delete(&self, name: &str) -> Result<(), Error> {
        self.handle()
            .with_enter(|enter, object| B::del_attr(enter.token(), object, name))
    }

    fn has(&self, name: &str) -> Result<bool, Error> {
        self.handle()
            .with_enter(|enter, object| Ok(B::has_attr(enter.token(), object, name)))
    }

    fn dir(&self) -> Result<Vec<String>, Error> {
        self.handle()
            .with_enter(|enter, object| B::dir(enter.token(), object))
    }

    fn item<T, K>(&self, key: K) -> Result<T::Owned, Error>
    where
        T: FromGuest<B>,
        K: ToGuest<B>,
    {
        self.handle().with_enter(|enter, object| {
            T::from_guest(enter, B::get_item(enter.token(), object, &key.to_guest(enter)?)?)
        })
    }

    fn set_item<K, T>(&self, key: K, value: T) -> Result<(), Error>
    where
        K: ToGuest<B>,
        T: ToGuest<B>,
    {
        self.handle().with_enter(|enter, object| {
            B::set_item(enter.token(), object, key.to_guest(enter)?, value.to_guest(enter)?)
        })
    }

    fn del_item<K: ToGuest<B>>(&self, key: K) -> Result<(), Error> {
        self.handle().with_enter(|enter, object| {
            B::del_item(enter.token(), object, &key.to_guest(enter)?)
        })
    }

    fn len(&self) -> Result<usize, Error> {
        self.handle()
            .with_enter(|enter, object| B::len(enter.token(), object))
    }

    fn is_empty(&self) -> Result<bool, Error> {
        Ok(self.len()? == 0)
    }

    fn call_method<A, R>(&self, name: &str, args: A) -> Result<R::Owned, Error>
    where
        A: ToGuestArgs<B>,
        R: FromGuest<B>,
    {
        self.handle().with_enter(|enter, object| {
            R::from_guest(
                enter,
                B::call(
                    enter.token(),
                    &B::get_attr(enter.token(), object, name)?,
                    &args.into_args(enter)?,
                    &[],
                )?,
            )
        })
    }

    fn call<A, R>(&self, args: A) -> Result<R::Owned, Error>
    where
        A: ToGuestArgs<B>,
        R: FromGuest<B>,
    {
        self.call_with::<A, (), R>(args, ())
    }

    fn call_with<A, K, R>(&self, args: A, kwargs: K) -> Result<R::Owned, Error>
    where
        A: ToGuestArgs<B>,
        K: ToGuestKwargs<B>,
        R: FromGuest<B>,
    {
        self.handle().with_enter(|enter, callable| {
            let kwargs = kwargs.into_kwargs(enter)?;

            R::from_guest(
                enter,
                B::call(
                    enter.token(),
                    callable,
                    &args.into_args(enter)?,
                    &kwargs
                        .iter()
                        .map(|(name, value)| (name.as_str(), value.clone()))
                        .collect::<Vec<_>>(),
                )?,
            )
        })
    }

    fn object(&self, name: &str) -> Result<Object<B>, Error> {
        self.get::<Object<B>>(name)
    }

    fn function(&self, name: &str) -> Result<Function<B>, Error> {
        self.get::<Function<B>>(name)
    }

    fn class(&self, name: &str) -> Result<Class<B>, Error> {
        self.get::<Class<B>>(name)
    }

    fn type_of(&self) -> Result<Class<B>, Error> {
        self.get::<Class<B>>("__class__")
    }

    fn is_instance_of<R>(&self, class: &Class<B, R>) -> Result<bool, Error> {
        self.handle().with_enter(|enter, object| {
            B::is_instance(
                enter.token(),
                object,
                &B::attach(enter.token(), class.handle().owned()),
            )
        })
    }

    fn iter(&self) -> Result<Iter<B>, Error> {
        self.handle().with_enter(|enter, object| {
            Ok(Iter::from_handle(Handle::new(
                B::detach(enter.token(), B::iter(enter.token(), object)?),
                self.handle().guest().clone(),
            )))
        })
    }

    fn cast<T: FromGuest<B>>(&self) -> Result<T::Owned, Error> {
        self.handle()
            .with_enter(|enter, object| T::from_guest(enter, object.clone()))
    }

    fn is_none(&self) -> Result<bool, Error> {
        self.handle()
            .with_enter(|enter, object| Ok(B::is_none(enter.token(), object)))
    }

    fn truthy(&self) -> Result<bool, Error> {
        self.handle()
            .with_enter(|enter, object| B::truthy(enter.token(), object))
    }

    fn type_name(&self) -> Result<String, Error> {
        self.handle()
            .with_enter(|enter, object| Ok(B::type_name(enter.token(), object)))
    }

    fn repr(&self) -> Result<String, Error> {
        self.handle()
            .with_enter(|enter, object| B::repr(enter.token(), object))
    }

    fn str(&self) -> Result<String, Error> {
        self.handle()
            .with_enter(|enter, object| B::display(enter.token(), object))
    }

    fn id(&self) -> Result<usize, Error> {
        self.handle()
            .with_enter(|enter, object| Ok(B::identity(enter.token(), object)))
    }

    fn is<O: HasHandle<B>>(&self, other: &O) -> bool {
        self.handle().ptr_eq(other.handle())
    }

    fn value(&self) -> Value<B> {
        self.handle().value()
    }
}

impl<B, T> ObjectProtocol<B> for T
where
    B: Backend + BackendValues,
    T: HasHandle<B>,
{
}

pub trait Named<B>
where
    B: Backend + BackendValues,
    Self: ObjectProtocol<B>,
{
    fn name(&self) -> Result<String, Error> {
        self.handle().with_enter(|enter, object| {
            B::as_str(enter.token(), &B::get_attr(enter.token(), object, "__name__")?)
        })
    }

    fn qualified_name(&self) -> Result<String, Error> {
        self.handle().with_enter(|enter, object| {
            B::as_str(
                enter.token(),
                &B::get_attr(enter.token(), object, "__qualname__")?,
            )
        })
    }

    fn module_name(&self) -> Result<Option<String>, Error> {
        self.handle().with_enter(|enter, object| {
            let value = B::get_attr(enter.token(), object, "__module__")?;

            if B::is_none(enter.token(), &value) {
                Ok(None)
            } else {
                Ok(Some(B::as_str(enter.token(), &value)?))
            }
        })
    }

    fn doc(&self) -> Result<Option<String>, Error> {
        self.handle().with_enter(|enter, object| {
            let value = B::get_attr(enter.token(), object, "__doc__")?;

            if B::is_none(enter.token(), &value) {
                Ok(None)
            } else {
                Ok(Some(B::as_str(enter.token(), &value)?))
            }
        })
    }
}

pub trait Annotated<B>
where
    B: Backend + BackendValues,
    Self: ObjectProtocol<B>,
{
    fn annotations(&self) -> Result<Vec<(String, Class<B>)>, Error> {
        if !self.has("__annotations__")? {
            return Ok(Vec::new());
        }

        let annotations = self.object("__annotations__")?;
        let mut resolved = Vec::new();

        for name in annotations.iter()?.collect::<String>()? {
            resolved.push((
                name.clone(),
                annotations.item::<Class<B>, _>(name.as_str())?,
            ));
        }

        Ok(resolved)
    }

    fn annotation(&self, name: &str) -> Result<Option<Class<B>>, Error> {
        if !self.has("__annotations__")? {
            return Ok(None);
        }

        self.object("__annotations__")?
            .handle()
            .with_enter(|enter, annotations| {
                match B::get_item_opt(enter.token(), annotations, &name.to_guest(enter)?)? {
                    Some(annotation) => Class::<B>::from_guest(enter, annotation).map(Some),
                    None => Ok(None),
                }
            })
    }
}

pub trait TypeProtocol<B>
where
    B: Backend + BackendValues,
    Self: IsType<B> + Named<B> + Annotated<B>,
{
    fn bases(&self) -> Result<Vec<Class<B>>, Error> {
        self.get::<Vec<Class<B>>>("__bases__")
    }

    fn mro(&self) -> Result<Vec<Class<B>>, Error> {
        self.get::<Vec<Class<B>>>("__mro__")
    }

    fn is_subclass_of<R>(&self, class: &Class<B, R>) -> Result<bool, Error> {
        self.handle().with_enter(|enter, subclass| {
            B::is_subclass(
                enter.token(),
                subclass,
                &B::attach(enter.token(), class.handle().owned()),
            )
        })
    }

    fn generic_bases(&self) -> Result<Vec<GenericAlias<B>>, Error> {
        if !self.has("__orig_bases__")? {
            return Ok(Vec::new());
        }

        let mut aliases = Vec::new();

        for base in self.get::<Vec<Object<B>>>("__orig_bases__")? {
            if let Some(alias) = GenericAlias::of(&base)? {
                aliases.push(alias);
            }
        }

        Ok(aliases)
    }

    fn generic_base_of<R>(&self, class: &Class<B, R>) -> Result<Option<GenericAlias<B>>, Error> {
        for alias in self.generic_bases()? {
            if alias.origin()?.is(class) {
                return Ok(Some(alias));
            }
        }

        Ok(None)
    }
}

impl<B, T> TypeProtocol<B> for T
where
    B: Backend + BackendValues,
    T: IsType<B> + Named<B> + Annotated<B>,
{
}

pub struct GenericAlias<B: Backend> {
    alias: Object<B>,
}

impl<B> GenericAlias<B>
where
    B: Backend + BackendValues,
{
    pub(crate) fn of(base: &Object<B>) -> Result<Option<Self>, Error> {
        if base.has("__origin__")? {
            Ok(Some(Self {
                alias: base.clone(),
            }))
        } else {
            Ok(None)
        }
    }

    pub fn origin(&self) -> Result<Class<B>, Error> {
        self.alias.get::<Class<B>>("__origin__")
    }

    pub fn arguments(&self) -> Result<Vec<Object<B>>, Error> {
        self.alias.get::<Vec<Object<B>>>("__args__")
    }
}

impl<B: Backend> Clone for GenericAlias<B> {
    fn clone(&self) -> Self {
        Self {
            alias: self.alias.clone(),
        }
    }
}

impl<B: Backend> HasHandle<B> for GenericAlias<B> {
    fn handle(&self) -> &Handle<B> {
        self.alias.handle()
    }
}
