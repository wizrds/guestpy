use std::{future::Future, rc::Rc};

use crate::{
    backend::{
        Backend, BackendCallables, BackendClasses, BackendCoroutines, BackendExceptions,
        BackendModules, BackendValues,
    },
    errors::Error,
    host::{
        class::{ClassDeclaration, ClassSpec, HostClass, HostClassDefinition},
        declaration::Member,
        exception::{ExceptionBase, ExceptionDeclaration, ExceptionSpec},
        namespace::Namespace,
    },
    marshal::{ToGuest, args::Args},
    scope::Enter,
};

pub(crate) type InitHook<B> = Rc<dyn for<'py> Fn(&Enter<'py, B>) -> Result<(), Error>>;

pub struct ModuleSpec<B: Backend> {
    name: String,
    doc: Option<String>,
    namespace: Namespace<B>,
    classes: Vec<Rc<ClassSpec<B>>>,
    exceptions: Vec<Rc<ExceptionSpec>>,
    init: Option<InitHook<B>>,
}

impl<B: Backend> ModuleSpec<B> {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn docstring(&self) -> Option<&str> {
        self.doc.as_deref()
    }

    pub(crate) fn init_hook(&self) -> Option<&InitHook<B>> {
        self.init.as_ref()
    }

    pub(crate) fn classes(&self) -> impl Iterator<Item = &Rc<ClassSpec<B>>> {
        self.classes.iter()
    }

    pub(crate) fn exceptions(&self) -> impl Iterator<Item = &Rc<ExceptionSpec>> {
        self.exceptions.iter()
    }
}

impl<B> ModuleSpec<B>
where
    B: Backend + BackendValues + BackendCallables,
{
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            doc: None,
            namespace: Namespace::new(),
            classes: Vec::new(),
            exceptions: Vec::new(),
            init: None,
        }
    }

    pub fn doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = Some(doc.into());

        self
    }

    pub fn init<F>(mut self, function: F) -> Self
    where
        F: for<'py> Fn(&Enter<'py, B>) -> Result<(), Error> + 'static,
    {
        self.init = Some(Rc::new(function));

        self
    }

    pub(crate) fn members(&self) -> &[(String, Member<B>)] {
        self.namespace.members()
    }

    pub fn constant<V>(mut self, name: &str, value: V) -> Self
    where
        V: ToGuest<B> + Clone + 'static,
    {
        self.namespace.constant(name, value);

        self
    }

    pub fn function<F, R>(mut self, name: &str, function: F) -> Self
    where
        F: for<'py> Fn(&Enter<'py, B>, Args<'py, B>) -> Result<R, Error> + 'static,
        R: ToGuest<B> + 'static,
    {
        self.namespace.function(name, function);

        self
    }

    pub fn getter<F, R>(mut self, name: &str, get: F) -> Self
    where
        F: for<'py> Fn(&Enter<'py, B>) -> Result<R, Error> + 'static,
        R: ToGuest<B> + 'static,
    {
        self.namespace.getter(name, get);

        self
    }

    pub fn object<F: FnOnce(&mut Namespace<B>)>(mut self, name: &str, build: F) -> Self {
        self.namespace.object(name, build);

        self
    }
}

impl<B> ModuleSpec<B>
where
    B: Backend
        + BackendValues
        + BackendCallables
        + BackendModules
        + BackendCoroutines
        + BackendExceptions,
{
    pub fn async_function<F, Fut, R>(mut self, name: &str, function: F) -> Self
    where
        F: for<'py> Fn(&Enter<'py, B>, Args<'py, B>) -> Result<Fut, Error> + 'static,
        Fut: Future<Output = Result<R, Error>> + 'static,
        R: ToGuest<B> + 'static,
    {
        self.namespace
            .async_function(name, function);

        self
    }
}

impl<B> ModuleSpec<B>
where
    B: Backend + BackendValues + BackendCallables + BackendExceptions,
{
    pub fn exception(mut self, name: &str, base: ExceptionBase) -> Self {
        let spec = Rc::new(ExceptionSpec::new(&self.name, name, base));

        self.exceptions.push(spec.clone());
        self.namespace
            .push(name, Rc::new(ExceptionDeclaration::new(spec)));

        self
    }
}

impl<B> ModuleSpec<B>
where
    B: Backend + BackendValues + BackendCallables + BackendClasses,
{
    pub fn class<C>(mut self) -> Self
    where
        C: HostClass + HostClassDefinition<B>,
    {
        let spec = ClassSpec::of::<C>();

        spec.set_module(&self.name);

        self.classes.push(spec.clone());
        self.namespace
            .push(C::NAME, Rc::new(ClassDeclaration::new(spec)));

        self
    }
}

#[cfg(test)]
mod tests {
    use super::ModuleSpec;
    use crate::{
        backend::{
            Backend, BackendCallables, BackendClasses, BackendCoroutines, BackendExceptions,
            BackendModules, BackendValues, tests::Stub,
        },
        errors::Error,
        host::{
            class::{ClassBuilder, HostClass, HostClassDefinition},
            dunder::Dunder,
            exception::ExceptionBase,
        },
        marshal::args::Args,
        scope::Enter,
    };

    struct BaseVector;

    impl HostClass for BaseVector {
        const NAME: &'static str = "BaseVector";
    }

    impl<B> HostClassDefinition<B> for BaseVector
    where
        B: Backend + BackendValues + BackendCallables + BackendClasses,
    {
        fn build(_: &mut ClassBuilder<B, Self>) {}
    }

    struct Vector2 {
        x: i64,
        y: i64,
    }

    impl HostClass for Vector2 {
        const NAME: &'static str = "Vector2";
    }

    impl<B> HostClassDefinition<B> for Vector2
    where
        B: Backend
            + BackendValues
            + BackendCallables
            + BackendClasses
            + BackendModules
            + BackendCoroutines
            + BackendExceptions,
    {
        fn construct<'py>(_: &Enter<'py, B>, _: Args<'py, B>) -> Result<Self, Error> {
            Ok(Self { x: 3, y: 4 })
        }

        fn build(builder: &mut ClassBuilder<B, Self>) {
            builder
                .method("length", |vector, _, _| Ok::<_, Error>(vector.x + vector.y))
                .method_mut("translate", |vector, _, _| {
                    vector.x += 1;

                    Ok::<_, Error>(())
                })
                .async_method("resolve", |_, _, _| Ok::<_, Error>(async { Ok::<_, Error>(()) }))
                .getter("x", |vector, _| Ok::<_, Error>(vector.x))
                .setter("x", |vector, _, value: i64| {
                    vector.x = value;

                    Ok::<_, Error>(())
                })
                .property(
                    "y",
                    |vector, _| Ok::<_, Error>(vector.y),
                    |vector, _, value: i64| {
                        vector.y = value;

                        Ok::<_, Error>(())
                    },
                )
                .class_method("origin", |_, _, _| Ok::<_, Error>(()))
                .static_method("zero", |_, _| Ok::<_, Error>(()))
                .constant("dimensions", 2)
                .dunder(Dunder::Repr, |_, _, _| Ok::<_, Error>("Vector2"))
                .dunder(Dunder::Len, |_, _, _| Ok::<_, Error>(2))
                .dunder(Dunder::Add, |_, _, _| Ok::<_, Error>(()))
                .statics(|namespace| {
                    namespace.constant("kind", "vector");
                })
                .base::<BaseVector>();
        }
    }

    #[allow(dead_code)]
    fn declaration() {
        let _ = ModuleSpec::<Stub>::new("geometry")
            .doc("Geometry helpers.")
            .constant("api_version", 1)
            .function("hypot", |_, _| Ok::<_, Error>(13.0_f64))
            .async_function("resolve", |_, _| {
                Ok::<_, Error>(async { Ok::<_, Error>(String::new()) })
            })
            .getter("version", |_| Ok::<_, Error>(1))
            .object("metadata", |namespace| {
                namespace
                    .constant("name", "geometry")
                    .property("version", |_| Ok::<_, Error>(1), |_, _: i64| Ok::<_, Error>(()));
            })
            .exception("GeometryError", ExceptionBase::Exception)
            .class::<Vector2>()
            .init(|_| Ok::<_, Error>(()));
    }
}
