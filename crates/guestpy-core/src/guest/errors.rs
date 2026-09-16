use std::marker::PhantomData;

use crate::{
    backend::{Backend, BackendCallables, BackendModules, BackendValues, Tok, Val},
    errors::{Error, GuestException},
    host::exception::{ExceptionClass, ExceptionRealiser, FatalExceptions, Raise, RaiseValue},
    imports::Imports,
    runtime::RuntimeInner,
    scope::Enter,
};

pub(crate) trait GuestErrorHandler<B: Backend + BackendValues> {
    fn raise_runtime<'py>(
        &self,
        token: Tok<'py, B>,
        runtime: &RuntimeInner<B>,
        error: Error,
    ) -> Error;

    fn raise_guest<'py>(&self, enter: &Enter<'py, B>, error: Error) -> Error;

    fn exception<'py>(&self, enter: &Enter<'py, B>, error: Error) -> Val<'py, B>;
}

enum ErrorSite<'py, 'e, B: Backend + BackendValues> {
    Guest(&'e Enter<'py, B>),
    Runtime {
        token: Tok<'py, B>,
        runtime: &'e RuntimeInner<B>,
    },
}

pub(crate) struct ExceptionRaiser<B>
where
    B: Backend + BackendValues + BackendCallables + BackendModules,
{
    _marker: PhantomData<B>,
}

impl<B> ExceptionRaiser<B>
where
    B: Backend + BackendValues + BackendCallables + BackendModules,
{
    pub fn new() -> Self {
        Self { _marker: PhantomData }
    }

    fn token<'py>(&self, site: &ErrorSite<'py, '_, B>) -> Tok<'py, B> {
        match site {
            ErrorSite::Guest(enter) => enter.token(),
            ErrorSite::Runtime { token, .. } => *token,
        }
    }

    fn runtime<'py, 'e>(&self, site: &ErrorSite<'py, 'e, B>) -> ExceptionRealiser<'py, 'e, B> {
        let token = self.token(site);

        ExceptionRealiser::new(
            token,
            match site {
                ErrorSite::Guest(enter) => enter.guest().realisation(),
                ErrorSite::Runtime { runtime, .. } => runtime.realisation(),
            },
        )
    }

    fn value<'py>(
        &self,
        site: &ErrorSite<'py, '_, B>,
        value: RaiseValue<B>,
    ) -> Result<Val<'py, B>, Error> {
        match (value, site) {
            (RaiseValue::Text(text), _) => Ok(B::str(self.token(site), &text)),
            (RaiseValue::Guest(thunk), ErrorSite::Guest(enter)) => thunk(enter),
            (RaiseValue::Guest(_), ErrorSite::Runtime { .. }) => {
                Err(Error::unexpected("host raise needs an active guest"))
            }
        }
    }

    fn class<'py>(
        &self,
        site: &ErrorSite<'py, '_, B>,
        class: &ExceptionClass,
    ) -> Result<Val<'py, B>, Error> {
        let realiser = self.runtime(site);

        let resolved = match class {
            ExceptionClass::Builtin(name) => realiser.builtin(name)?,
            ExceptionClass::Host { .. } | ExceptionClass::Typed { .. } => {
                let key = class
                    .key()
                    .expect("host and typed classes always have a key");

                realiser.host(&key)?
            }
            ExceptionClass::Guest { module, qualname } => match site {
                ErrorSite::Guest(enter) => {
                    let imports = Imports::new(enter);

                    imports.qualified(&imports.module(module)?, qualname)?
                }
                ErrorSite::Runtime { .. } => {
                    return Err(Error::unexpected("host raise needs an active guest"));
                }
            },
        };

        realiser.exception_class(class, resolved)
    }

    fn raise_value<'py>(
        &self,
        site: &ErrorSite<'py, '_, B>,
        error: Error,
    ) -> Result<Val<'py, B>, Error> {
        let token = self.token(site);

        if let Error::Guest(exception) = &error
            && let Some(owned) = exception.object::<B>()
        {
            return Ok(B::attach(token, owned));
        }

        let raise = match error {
            Error::Guest(exception) => Raise::new(ExceptionClass::builtin("RuntimeError"))
                .text(exception.message().to_owned()),
            Error::Raise(erased) => match Raise::<B>::from_erased(*erased) {
                Some(raise) => raise,
                None => Raise::new(ExceptionClass::builtin("SystemError"))
                    .text("raise was built for a different backend"),
            },
            Error::Conversion { message, .. } => {
                Raise::new(ExceptionClass::builtin("TypeError")).text(message)
            }
            Error::Import { name, message } => Raise::new(ExceptionClass::builtin("ImportError"))
                .text(message)
                .attr("name", name),
            Error::Attribute { name } => Raise::new(ExceptionClass::builtin("AttributeError"))
                .text(format!("no attribute named '{name}'")),
            Error::Bundle { message, .. } => Raise::new(ExceptionClass::builtin("ImportError"))
                .text(message)
                .attr("name", String::from("guestpy")),
            Error::AmbiguousBundle { roots } => Raise::new(ExceptionClass::builtin("ImportError"))
                .text(format!("bundle has {roots} top-level modules"))
                .attr("name", String::from("guestpy")),
            Error::NameInUse { name } => Raise::new(ExceptionClass::builtin("ImportError"))
                .text(format!("module {name} is already loaded in this guest"))
                .attr("name", name),
            Error::Borrow { class, kind } => Raise::new(ExceptionClass::builtin("RuntimeError"))
                .text(format!("host class {class} is already borrowed ({kind})")),
            Error::Unsupported { message } => {
                Raise::new(ExceptionClass::builtin("NotImplementedError")).text(message)
            }
            Error::Host(inner) => {
                Raise::new(ExceptionClass::builtin("RuntimeError")).text(inner.to_string())
            }
            Error::Timeout => Raise::new(FatalExceptions::class(FatalExceptions::TIMEOUT))
                .text("execution timed out"),
            Error::Cancelled => Raise::new(FatalExceptions::class(FatalExceptions::CANCELLED))
                .text("execution cancelled"),
            Error::Interrupted => Raise::new(FatalExceptions::class(FatalExceptions::INTERRUPTED))
                .text("execution interrupted"),
            Error::Closed => {
                Raise::new(FatalExceptions::class(FatalExceptions::CLOSED)).text("guest is closed")
            }
            Error::StopIteration => Raise::new(ExceptionClass::builtin("StopIteration")),
            Error::StopAsyncIteration => Raise::new(ExceptionClass::builtin("StopAsyncIteration")),
            Error::Io(inner) => {
                Raise::new(ExceptionClass::builtin("OSError")).text(inner.to_string())
            }
            Error::Engine { message, .. } | Error::Unexpected { message, .. } => {
                Raise::new(ExceptionClass::builtin("SystemError")).text(message)
            }
        };

        let class = self.class(site, &raise.class)?;
        let mut values = Vec::with_capacity(raise.args.len());

        for value in raise.args {
            values.push(self.value(site, value)?);
        }

        let object = B::call(token, &class, &values, &[])?;

        for (name, value) in raise.attrs {
            let value = self.value(site, value)?;

            B::set_attr(token, &object, &name, value)?;
        }

        if let Some(cause) = raise.cause {
            let cause = self.convert(site, cause);

            B::set_attr(token, &object, "__cause__", cause)?;
            B::set_attr(token, &object, "__suppress_context__", B::bool(token, true))?;
        }

        Ok(object)
    }

    fn fallback<'py>(&self, site: &ErrorSite<'py, '_, B>) -> Val<'py, B> {
        let token = self.token(site);

        self.class(site, &ExceptionClass::builtin("SystemError"))
            .and_then(|class| B::call(token, &class, &[], &[]))
            .unwrap_or_else(|_| B::none(token))
    }

    fn convert<'py>(&self, site: &ErrorSite<'py, '_, B>, error: Error) -> Val<'py, B> {
        self.raise_value(site, error)
            .or_else(|error| self.raise_value(site, error))
            .unwrap_or_else(|_| self.fallback(site))
    }
}

impl<B> GuestErrorHandler<B> for ExceptionRaiser<B>
where
    B: Backend + BackendValues + BackendCallables + BackendModules,
{
    fn raise_runtime<'py>(
        &self,
        token: Tok<'py, B>,
        runtime: &RuntimeInner<B>,
        error: Error,
    ) -> Error {
        let object = self.convert(&ErrorSite::Runtime { token, runtime }, error);

        Error::Guest(Box::new(GuestException::describe::<B>(token, object, None)))
    }

    fn raise_guest<'py>(&self, enter: &Enter<'py, B>, error: Error) -> Error {
        let object = self.convert(&ErrorSite::Guest(enter), error);

        Error::Guest(Box::new(GuestException::describe::<B>(enter.token(), object, None)))
    }

    fn exception<'py>(&self, enter: &Enter<'py, B>, error: Error) -> Val<'py, B> {
        self.convert(&ErrorSite::Guest(enter), error)
    }
}
