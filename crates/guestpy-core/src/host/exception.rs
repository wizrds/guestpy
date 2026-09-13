use std::{
    any::TypeId,
    borrow::Cow,
    fmt::{self, Display, Formatter},
    rc::Rc,
};

use crate::{
    backend::{Backend, BackendCallables, BackendModules, BackendValues, Tok, Val},
    catalog::RealisationCache,
    errors::{ErasedRaise, Error},
    host::{
        declaration::{DeclarationContext, DeclareMember},
        module::ModuleSpec,
    },
    marshal::ToGuest,
    scope::Enter,
};

pub(crate) struct ExceptionRealiser<'py, 'r, B: Backend> {
    token: Tok<'py, B>,
    realisation: &'r RealisationCache<B>,
}

impl<'py, 'r, B> ExceptionRealiser<'py, 'r, B>
where
    B: Backend + BackendValues + BackendModules,
{
    pub(crate) fn new(token: Tok<'py, B>, realisation: &'r RealisationCache<B>) -> Self {
        Self { token, realisation }
    }

    fn create(&self, module: &str, name: &str, base: &Val<'py, B>) -> Result<Val<'py, B>, Error> {
        B::call(
            self.token,
            &self.builtin("type")?,
            &[
                B::str(self.token, name),
                B::tuple(self.token, vec![base.clone()])?,
                B::dict(
                    self.token,
                    vec![(B::str(self.token, "__module__"), B::str(self.token, module))],
                )?,
            ],
            &[],
        )
    }

    pub(crate) fn builtin(&self, name: &str) -> Result<Val<'py, B>, Error> {
        B::get_item(self.token, &B::builtins_dict(self.token)?, &B::str(self.token, name))
    }

    pub(crate) fn exception_class(
        &self,
        class: &ExceptionClass,
        value: Val<'py, B>,
    ) -> Result<Val<'py, B>, Error> {
        if B::is_class(self.token, &value)
            && B::is_subclass(self.token, &value, &self.builtin("BaseException")?)?
        {
            return Ok(value);
        }

        Err(Error::conversion(format!("{class} is not an exception class")))
    }

    pub(crate) fn host(&self, key: &ExceptionKey) -> Result<Val<'py, B>, Error> {
        if let Some(owned) = self.realisation.realised_exception(key) {
            return Ok(B::attach(self.token, &owned));
        }

        let spec = self
            .realisation
            .exception_spec(key)
            .ok_or_else(|| {
                Error::unexpected(format!("host exception {} was not registered", key.name()))
            })?;
        let base = match spec.base().key() {
            Some(base) => self.host(&base)?,
            None => {
                let ExceptionClass::Builtin(name) = spec.base() else {
                    return Err(Error::unsupported(format!(
                        "host exception {} cannot inherit from guest class {}",
                        spec.name(),
                        spec.base(),
                    )));
                };

                self.builtin(name)?
            }
        };
        let class =
            self.create(spec.module(), spec.name(), &self.exception_class(spec.base(), base)?)?;

        self.realisation
            .set_realised_exception(key, B::detach(self.token, class.clone()));

        Ok(class)
    }
}

pub(crate) struct ExceptionDeclaration {
    spec: Rc<ExceptionSpec>,
}

impl ExceptionDeclaration {
    pub(crate) fn new(spec: Rc<ExceptionSpec>) -> Self {
        Self { spec }
    }
}

impl<B> DeclareMember<B> for ExceptionDeclaration
where
    B: Backend + BackendValues + BackendModules,
{
    fn realise<'py>(
        &self,
        context: &DeclarationContext<'py, '_, B>,
        _name: &str,
    ) -> Result<Val<'py, B>, Error> {
        ExceptionRealiser::new(context.enter().token(), context.enter().guest().realisation())
            .host(self.spec.key())
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ExceptionClass {
    Builtin(Cow<'static, str>),
    Host {
        module: Cow<'static, str>,
        name: Cow<'static, str>,
    },
    Guest {
        module: Cow<'static, str>,
        qualname: Cow<'static, str>,
    },
    Typed {
        id: TypeId,
        name: &'static str,
    },
}

impl ExceptionClass {
    pub fn builtin(name: impl Into<Cow<'static, str>>) -> Self {
        Self::Builtin(name.into())
    }

    pub fn host(module: impl Into<Cow<'static, str>>, name: impl Into<Cow<'static, str>>) -> Self {
        Self::Host { module: module.into(), name: name.into() }
    }

    pub fn guest(
        module: impl Into<Cow<'static, str>>,
        qualname: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self::Guest {
            module: module.into(),
            qualname: qualname.into(),
        }
    }

    pub fn of<E: HostException>() -> Self {
        Self::Typed { id: TypeId::of::<E>(), name: E::NAME }
    }

    pub fn exception() -> Self {
        Self::builtin("Exception")
    }

    pub(crate) fn key(&self) -> Option<ExceptionKey> {
        match self {
            Self::Host { module, name } => Some(ExceptionKey::Named {
                module: module.to_string(),
                name: name.to_string(),
            }),
            Self::Typed { id, name } => Some(ExceptionKey::Typed { id: *id, name: *name }),
            Self::Builtin(_) | Self::Guest { .. } => None,
        }
    }
}

impl Display for ExceptionClass {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Builtin(name) => formatter.write_str(name),
            Self::Host { module, name } => write!(formatter, "{module}.{name}"),
            Self::Guest { module, qualname } => write!(formatter, "{module}.{qualname}"),
            Self::Typed { name, .. } => formatter.write_str(name),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum ExceptionKey {
    Named { module: String, name: String },
    Typed { id: TypeId, name: &'static str },
}

impl ExceptionKey {
    fn name(&self) -> &str {
        match self {
            Self::Named { name, .. } => name,
            Self::Typed { name, .. } => name,
        }
    }
}

pub struct ExceptionSpec {
    key: ExceptionKey,
    module: String,
    base: ExceptionClass,
}

impl ExceptionSpec {
    pub(crate) fn named(
        module: impl Into<String>,
        name: impl Into<String>,
        base: ExceptionClass,
    ) -> Self {
        let module = module.into();

        Self {
            key: ExceptionKey::Named {
                module: module.clone(),
                name: name.into(),
            },
            module,
            base,
        }
    }

    pub(crate) fn typed<E: HostException>(module: impl Into<String>) -> Self {
        Self {
            key: ExceptionKey::Typed { id: TypeId::of::<E>(), name: E::NAME },
            module: module.into(),
            base: E::base(),
        }
    }

    pub(crate) fn key(&self) -> &ExceptionKey {
        &self.key
    }

    pub(crate) fn module(&self) -> &str {
        &self.module
    }

    pub(crate) fn name(&self) -> &str {
        self.key.name()
    }

    pub(crate) fn base(&self) -> &ExceptionClass {
        &self.base
    }
}

type Thunk<B> = Box<dyn for<'py> FnOnce(&Enter<'py, B>) -> Result<Val<'py, B>, Error>>;

pub(crate) enum RaiseValue<B: Backend> {
    Text(Cow<'static, str>),
    Guest(Thunk<B>),
}

pub struct Raise<B: Backend> {
    pub(crate) class: ExceptionClass,
    pub(crate) args: Vec<RaiseValue<B>>,
    pub(crate) attrs: Vec<(String, RaiseValue<B>)>,
    pub(crate) cause: Option<Error>,
}

impl<B: Backend> Raise<B> {
    pub fn new(class: ExceptionClass) -> Self {
        Self {
            class,
            args: Vec::new(),
            attrs: Vec::new(),
            cause: None,
        }
    }

    pub fn within<'py>(_: &Enter<'py, B>, class: ExceptionClass) -> Self {
        Self::new(class)
    }

    pub fn arg<V>(mut self, value: V) -> Self
    where
        V: ToGuest<B> + 'static,
    {
        self.args
            .push(RaiseValue::Guest(Box::new(move |enter| value.to_guest(enter))));

        self
    }

    pub fn attr<V>(mut self, name: impl Into<String>, value: V) -> Self
    where
        V: ToGuest<B> + 'static,
    {
        self.attrs
            .push((name.into(), RaiseValue::Guest(Box::new(move |enter| value.to_guest(enter)))));

        self
    }

    pub(crate) fn text(mut self, value: impl Into<Cow<'static, str>>) -> Self {
        self.args
            .push(RaiseValue::Text(value.into()));

        self
    }

    pub fn cause(mut self, cause: impl Into<Error>) -> Self {
        self.cause = Some(cause.into());

        self
    }

    pub fn host<E: IntoRaise<B>>(exception: E) -> Self {
        exception.values(Self::new(E::class()))
    }

    pub(crate) fn from_erased(erased: ErasedRaise) -> Option<Self> {
        erased
            .into_payload()
            .downcast::<Self>()
            .ok()
            .map(|raise| *raise)
    }
}

impl<B: Backend> From<Raise<B>> for Error {
    fn from(raise: Raise<B>) -> Self {
        Self::Raise(Box::new(ErasedRaise::new(raise.class.to_string(), Box::new(raise))))
    }
}

pub trait HostException: Sized + 'static {
    const NAME: &'static str;

    fn base() -> ExceptionClass {
        ExceptionClass::exception()
    }

    fn class() -> ExceptionClass {
        ExceptionClass::of::<Self>()
    }
}

pub trait IntoRaise<B: Backend>: HostException {
    fn values(self, raise: Raise<B>) -> Raise<B>;
}

pub(crate) struct FatalExceptions;

impl FatalExceptions {
    pub(crate) const MODULE: &'static str = "guestpy";
    pub(crate) const TIMEOUT: &'static str = "TimeoutError";
    pub(crate) const CANCELLED: &'static str = "CancelledError";
    pub(crate) const INTERRUPTED: &'static str = "InterruptedError";
    pub(crate) const CLOSED: &'static str = "ClosedError";

    pub(crate) fn class(name: &'static str) -> ExceptionClass {
        ExceptionClass::host(Self::MODULE, name)
    }

    pub(crate) fn spec<B>() -> ModuleSpec<B>
    where
        B: Backend + BackendValues + BackendCallables + BackendModules,
    {
        [
            Self::TIMEOUT,
            Self::CANCELLED,
            Self::INTERRUPTED,
            Self::CLOSED,
        ]
        .into_iter()
        .fold(ModuleSpec::new(Self::MODULE), |spec, name| {
            spec.exception(name, ExceptionClass::builtin("BaseException"))
        })
    }

    pub(crate) fn reserve<B: Backend>(modules: &[Rc<ModuleSpec<B>>]) -> Result<(), Error> {
        if modules
            .iter()
            .any(|module| module.name() == Self::MODULE)
        {
            return Err(Error::NameInUse { name: Self::MODULE.to_owned() });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;

    use super::{ExceptionClass, ExceptionKey, ExceptionSpec, HostException};

    struct Example;

    impl HostException for Example {
        const NAME: &'static str = "Example";
    }

    #[test]
    fn exception_classes_display_their_identity() {
        assert_eq!(ExceptionClass::builtin("ValueError").to_string(), "ValueError");
        assert_eq!(ExceptionClass::host("plugin", "HostError").to_string(), "plugin.HostError",);
        assert_eq!(
            ExceptionClass::guest("plugin", "Guest.Error").to_string(),
            "plugin.Guest.Error",
        );
        assert_eq!(ExceptionClass::of::<Example>().to_string(), "Example");
    }

    #[test]
    fn exception_classes_key_their_registered_identity() {
        assert_eq!(ExceptionClass::builtin("ValueError").key(), None);
        assert_eq!(ExceptionClass::guest("plugin", "Guest.Error").key(), None);
        assert_eq!(
            ExceptionClass::host("plugin", "HostError").key(),
            Some(ExceptionKey::Named {
                module: String::from("plugin"),
                name: String::from("HostError"),
            }),
        );
        assert_eq!(
            ExceptionClass::of::<Example>().key(),
            Some(ExceptionKey::Typed {
                id: TypeId::of::<Example>(),
                name: "Example",
            }),
        );
    }

    #[test]
    fn exception_specs_retain_their_declared_key() {
        let named = ExceptionSpec::named("plugin", "NamedError", ExceptionClass::exception());
        let typed = ExceptionSpec::typed::<Example>("plugin");

        assert_eq!(
            named.key(),
            &ExceptionKey::Named {
                module: String::from("plugin"),
                name: String::from("NamedError"),
            },
        );
        assert_eq!(named.module(), "plugin");
        assert_eq!(named.name(), "NamedError");
        assert_eq!(
            typed.key(),
            &ExceptionKey::Typed {
                id: TypeId::of::<Example>(),
                name: "Example",
            },
        );
        assert_eq!(typed.module(), "plugin");
        assert_eq!(typed.name(), "Example");
    }
}
