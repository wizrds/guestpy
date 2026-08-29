//! Host-defined member collections shared by modules and classes.

use std::{future::Future, marker::PhantomData, rc::Rc};

use crate::{
    backend::{
        Backend, BackendCallables, BackendCoroutines, BackendExceptions, BackendModules,
        BackendValues, Val, callables::PendingValue,
    },
    errors::Error,
    handle::Value,
    host::{
        declaration::{DeclarationContext, DeclareMember, Member, ModuleGetter},
        function::{AsyncFunctionDeclaration, FunctionDeclaration},
    },
    marshal::{FromGuest, ToGuest, args::Args},
    scope::Enter,
};

struct ConstantValue<B: Backend, V> {
    value: V,
    backend: PhantomData<B>,
}

struct GetterValue<B: Backend, F, R> {
    get: F,
    backend: PhantomData<fn() -> (B, R)>,
}

pub(crate) trait ValueThunkBody<B: Backend> {
    fn produce<'py>(&self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error>;
}

impl<B, V> ValueThunkBody<B> for ConstantValue<B, V>
where
    B: Backend,
    V: ToGuest<B> + Clone + 'static,
{
    fn produce<'py>(&self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        self.value.clone().to_guest(enter)
    }
}

impl<B, F, R> ValueThunkBody<B> for GetterValue<B, F, R>
where
    B: Backend,
    F: for<'py> Fn(&Enter<'py, B>) -> Result<R, Error> + 'static,
    R: ToGuest<B> + 'static,
{
    fn produce<'py>(&self, enter: &Enter<'py, B>) -> Result<B::Value<'py>, Error> {
        (self.get)(enter)?.to_guest(enter)
    }
}

pub(crate) type ValueThunk<B> = Rc<dyn ValueThunkBody<B>>;

pub(crate) type SetThunk<B> = Rc<dyn for<'py> Fn(&Enter<'py, B>, Val<'py, B>) -> Result<(), Error>>;

pub(crate) type DelThunk<B> = Rc<dyn for<'py> Fn(&Enter<'py, B>) -> Result<(), Error>>;

struct PropertyDeclaration<B: Backend> {
    get: Option<ValueThunk<B>>,
    set: Option<SetThunk<B>>,
    del: Option<DelThunk<B>>,
    module_getter: Option<ModuleGetter<B>>,
}

impl<B: Backend> PropertyDeclaration<B> {
    fn new(get: Option<ValueThunk<B>>, set: Option<SetThunk<B>>, del: Option<DelThunk<B>>) -> Self {
        let module_getter = get
            .clone()
            .map(|get| Self::erase_module_getter(move |context| get.produce(context.enter())));

        Self { get, set, del, module_getter }
    }

    fn erase_module_getter<F>(getter: F) -> ModuleGetter<B>
    where
        F: for<'py, 'e> Fn(&DeclarationContext<'py, 'e, B>) -> Result<Val<'py, B>, Error> + 'static,
    {
        Rc::new(getter)
    }
}

impl<B> DeclareMember<B> for PropertyDeclaration<B>
where
    B: Backend + BackendValues + BackendCallables,
{
    fn realise<'py>(
        &self,
        context: &DeclarationContext<'py, '_, B>,
        name: &str,
    ) -> Result<Val<'py, B>, Error> {
        let getter = self
            .get
            .clone()
            .map(|get| {
                B::function(
                    context.enter().token(),
                    name,
                    None,
                    context
                        .enter()
                        .guest()
                        .raw_body(Rc::new(move |enter, _| get.produce(enter))),
                )
            })
            .transpose()?;
        let setter = self
            .set
            .clone()
            .map(|set| {
                B::function(
                    context.enter().token(),
                    name,
                    None,
                    context
                        .enter()
                        .guest()
                        .raw_body(Rc::new(move |enter, args| {
                            set(
                                enter,
                                args.required::<Value<B>>(enter, 1, "value")?
                                    .to_guest(enter)?,
                            )?;

                            Ok(B::none(enter.token()))
                        })),
                )
            })
            .transpose()?;
        let deleter = self
            .del
            .clone()
            .map(|del| {
                B::function(
                    context.enter().token(),
                    name,
                    None,
                    context
                        .enter()
                        .guest()
                        .raw_body(Rc::new(move |enter, _| {
                            del(enter)?;

                            Ok(B::none(enter.token()))
                        })),
                )
            })
            .transpose()?;

        context.property(getter, setter, deleter)
    }

    fn module_getter(&self) -> Option<&ModuleGetter<B>> {
        self.module_getter.as_ref()
    }
}

struct ObjectDeclaration<B: Backend> {
    namespace: Namespace<B>,
}

impl<B: Backend> ObjectDeclaration<B> {
    fn new(namespace: Namespace<B>) -> Self {
        Self { namespace }
    }
}

impl<B> DeclareMember<B> for ObjectDeclaration<B>
where
    B: Backend + BackendValues + BackendCallables,
{
    fn realise<'py>(
        &self,
        context: &DeclarationContext<'py, '_, B>,
        _name: &str,
    ) -> Result<Val<'py, B>, Error> {
        let members = B::new_dict(context.enter().token())?;

        for (name, member) in self.namespace.members() {
            B::set_item(
                context.enter().token(),
                &members,
                B::str(context.enter().token(), name),
                member.realise(context, name)?,
            )?;
        }

        let class = B::call(
            context.enter().token(),
            &context.builtin("type")?,
            &[
                B::str(context.enter().token(), "namespace"),
                B::tuple(context.enter().token(), Vec::new())?,
                members,
            ],
            &[],
        )?;

        B::call(context.enter().token(), &class, &[], &[])
    }
}

pub(crate) struct ValueDeclaration<B: Backend> {
    value: ValueThunk<B>,
}

impl<B: Backend> ValueDeclaration<B> {
    pub(crate) fn new(value: ValueThunk<B>) -> Self {
        Self { value }
    }
}

impl<B: Backend> DeclareMember<B> for ValueDeclaration<B> {
    fn realise<'py>(
        &self,
        context: &DeclarationContext<'py, '_, B>,
        _name: &str,
    ) -> Result<Val<'py, B>, Error> {
        self.value.produce(context.enter())
    }
}

pub struct Namespace<B: Backend> {
    members: Vec<(String, Member<B>)>,
}

impl<B> Namespace<B>
where
    B: Backend + BackendValues + BackendCallables,
{
    pub(crate) fn new() -> Self {
        Self { members: Vec::new() }
    }

    pub(crate) fn constant_thunk<V>(value: V) -> ValueThunk<B>
    where
        V: ToGuest<B> + Clone + 'static,
    {
        Rc::new(ConstantValue::<B, V> { value, backend: PhantomData })
    }

    pub(crate) fn members(&self) -> &[(String, Member<B>)] {
        &self.members
    }

    pub(crate) fn push(&mut self, name: &str, member: Member<B>) -> &mut Self {
        self.members
            .push((name.to_owned(), member));

        self
    }

    pub fn constant<V>(&mut self, name: &str, value: V) -> &mut Self
    where
        V: ToGuest<B> + Clone + 'static,
    {
        self.push(name, Rc::new(ValueDeclaration::new(Self::constant_thunk(value))))
    }

    pub fn function<F, R>(&mut self, name: &str, function: F) -> &mut Self
    where
        F: for<'py> Fn(&Enter<'py, B>, Args<'py, B>) -> Result<R, Error> + 'static,
        R: ToGuest<B> + 'static,
    {
        self.push(
            name,
            Rc::new(FunctionDeclaration::new(Rc::new(move |enter, args| {
                function(enter, args)?.to_guest(enter)
            }))),
        )
    }

    pub fn getter<F, R>(&mut self, name: &str, get: F) -> &mut Self
    where
        F: for<'py> Fn(&Enter<'py, B>) -> Result<R, Error> + 'static,
        R: ToGuest<B> + 'static,
    {
        self.push(
            name,
            Rc::new(PropertyDeclaration::new(
                Some(Rc::new(GetterValue::<B, F, R> { get, backend: PhantomData })),
                None,
                None,
            )),
        )
    }

    pub fn object<F: FnOnce(&mut Namespace<B>)>(&mut self, name: &str, build: F) -> &mut Self {
        let mut namespace = Namespace::new();

        build(&mut namespace);

        self.push(name, Rc::new(ObjectDeclaration::new(namespace)))
    }

    pub fn property<G, S, R, V>(&mut self, name: &str, get: G, set: S) -> &mut Self
    where
        G: for<'py> Fn(&Enter<'py, B>) -> Result<R, Error> + 'static,
        S: for<'py> Fn(&Enter<'py, B>, V) -> Result<(), Error> + 'static,
        R: ToGuest<B> + 'static,
        V: FromGuest<B, Owned = V> + 'static,
    {
        self.push(
            name,
            Rc::new(PropertyDeclaration::new(
                Some(Rc::new(GetterValue::<B, G, R> { get, backend: PhantomData })),
                Some(Rc::new(move |enter, value| set(enter, V::from_guest(enter, value)?))),
                None,
            )),
        )
    }
}

impl<B> Namespace<B>
where
    B: Backend
        + BackendValues
        + BackendCallables
        + BackendModules
        + BackendCoroutines
        + BackendExceptions,
{
    pub fn async_function<F, Fut, R>(&mut self, name: &str, function: F) -> &mut Self
    where
        F: for<'py> Fn(&Enter<'py, B>, Args<'py, B>) -> Result<Fut, Error> + 'static,
        Fut: Future<Output = Result<R, Error>> + 'static,
        R: ToGuest<B> + 'static,
    {
        self.push(
            name,
            Rc::new(AsyncFunctionDeclaration::new(Rc::new(move |enter, args| {
                Ok(PendingValue::<B, R>::into_host_future(function(enter, args)?))
            }))),
        )
    }
}
