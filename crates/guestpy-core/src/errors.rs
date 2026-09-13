use std::{
    any::Any,
    error::Error as StdError,
    fmt::{self, Debug, Display, Formatter},
};

use crate::backend::{Backend, BackendValues, Tok, Val};

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum BorrowKind {
    Shared,
    Exclusive,
}

impl Display for BorrowKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shared => formatter.write_str("shared"),
            Self::Exclusive => formatter.write_str("exclusive"),
        }
    }
}

trait ErasedOwnedInner {
    fn as_any(&self) -> &dyn Any;
}

struct Held<B: Backend>(Option<B::Owned>);

impl<B: Backend> ErasedOwnedInner for Held<B> {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<B: Backend> Drop for Held<B> {
    fn drop(&mut self) {
        if let Some(owned) = self.0.take() {
            B::release(owned);
        }
    }
}

pub struct ErasedOwned(Box<dyn ErasedOwnedInner>);

impl ErasedOwned {
    pub fn new<B: Backend>(owned: B::Owned) -> Self {
        Self(Box::new(Held::<B>(Some(owned))))
    }

    pub fn get<B: Backend>(&self) -> Option<&B::Owned> {
        self.0
            .as_any()
            .downcast_ref::<Held<B>>()?
            .0
            .as_ref()
    }
}

impl Debug for ErasedOwned {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("ErasedOwned(..)")
    }
}

pub struct ErasedRaise {
    class: String,
    payload: Box<dyn Any>,
}

impl ErasedRaise {
    pub(crate) fn new(class: String, payload: Box<dyn Any>) -> Self {
        Self { class, payload }
    }

    pub fn class(&self) -> &str {
        &self.class
    }

    pub(crate) fn into_payload(self) -> Box<dyn Any> {
        self.payload
    }
}

impl Debug for ErasedRaise {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ErasedRaise")
            .field("class", &self.class)
            .finish_non_exhaustive()
    }
}

pub struct GuestException {
    type_name: String,
    qualified_name: String,
    message: String,
    name: Option<String>,
    mro: Vec<String>,
    traceback: Option<String>,
    object: Option<ErasedOwned>,
}

impl GuestException {
    #[doc(hidden)]
    pub fn new(
        type_name: String,
        qualified_name: String,
        message: String,
        name: Option<String>,
        mro: Vec<String>,
        traceback: Option<String>,
        object: Option<ErasedOwned>,
    ) -> Self {
        Self {
            type_name,
            qualified_name,
            message,
            name,
            mro,
            traceback,
            object,
        }
    }

    pub(crate) fn describe<'py, B>(
        token: Tok<'py, B>,
        exception: Val<'py, B>,
        traceback: Option<String>,
    ) -> Self
    where
        B: Backend + BackendValues,
    {
        let class = B::get_attr(token, &exception, "__class__").ok();
        let type_name = class
            .as_ref()
            .and_then(|class| B::get_attr(token, class, "__name__").ok())
            .and_then(|name| B::as_str(token, &name).ok())
            .unwrap_or_else(|| String::from("Exception"));
        let qualified_name = class
            .as_ref()
            .and_then(|class| {
                Some((
                    B::get_attr(token, class, "__module__").ok()?,
                    B::get_attr(token, class, "__qualname__").ok()?,
                ))
            })
            .and_then(|(module, qualname)| {
                Some((B::as_str(token, &module).ok()?, B::as_str(token, &qualname).ok()?))
            })
            .map(|(module, qualname)| format!("{module}.{qualname}"))
            .unwrap_or_else(|| type_name.clone());
        let message = B::display(token, &exception).unwrap_or_else(|_| type_name.clone());
        let name = B::get_attr(token, &exception, "name")
            .ok()
            .filter(|name| !B::is_none(token, name))
            .and_then(|name| B::display(token, &name).ok());
        let mro = class
            .as_ref()
            .and_then(|class| B::get_attr(token, class, "__mro__").ok())
            .and_then(|mro| B::iter(token, &mro).ok())
            .map(|iterator| {
                let mut classes = Vec::new();

                while let Ok(Some(class)) = B::next(token, &iterator) {
                    if let Some((module, qualname)) = B::get_attr(token, &class, "__module__")
                        .ok()
                        .and_then(|module| {
                            Some((
                                B::as_str(token, &module).ok()?,
                                B::get_attr(token, &class, "__qualname__")
                                    .ok()
                                    .and_then(|qualname| B::as_str(token, &qualname).ok())?,
                            ))
                        })
                    {
                        classes.push(format!("{module}.{qualname}"));
                    }
                }

                classes
            })
            .unwrap_or_default();

        Self::new(
            type_name,
            qualified_name,
            message,
            name,
            mro,
            traceback,
            Some(ErasedOwned::new::<B>(B::detach(token, exception))),
        )
    }

    pub fn type_name(&self) -> &str {
        &self.type_name
    }

    pub fn qualified_name(&self) -> &str {
        &self.qualified_name
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn traceback(&self) -> Option<&str> {
        self.traceback.as_deref()
    }

    pub fn matches(&self, name: &str) -> bool {
        self.mro.iter().any(|entry| {
            entry == name
                || entry
                    .rsplit_once('.')
                    .is_some_and(|(_, bare_name)| bare_name == name)
        })
    }

    pub fn object<B: Backend>(&self) -> Option<&B::Owned> {
        self.object.as_ref()?.get::<B>()
    }
}

impl Display for GuestException {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.qualified_name, self.message)
    }
}

impl Debug for GuestException {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GuestException")
            .field("type_name", &self.type_name)
            .field("qualified_name", &self.qualified_name)
            .field("message", &self.message)
            .field("name", &self.name)
            .field("mro", &self.mro)
            .field("traceback", &self.traceback)
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("guest exception: {0}")]
    Guest(Box<GuestException>),

    #[error("raised {}", .0.class())]
    Raise(Box<ErasedRaise>),

    #[error("engine error: {message}")]
    Engine {
        message: String,
        #[source]
        source: Option<Box<dyn StdError + Send + Sync>>,
    },

    #[error("conversion error: {message}")]
    Conversion {
        message: String,
        #[source]
        source: Option<Box<dyn StdError + Send + Sync>>,
    },

    #[error("import error: {name}: {message}")]
    Import { name: String, message: String },

    #[error("no attribute named {name}")]
    Attribute { name: String },

    #[error("invalid bundle at {path}: {message}")]
    Bundle { path: String, message: String },

    #[error("bundle has {roots} top-level modules; mount it with `library` instead")]
    AmbiguousBundle { roots: usize },

    #[error("module {name} is already loaded in this guest")]
    NameInUse { name: String },

    #[error("host class {class} is already borrowed ({kind})")]
    Borrow {
        class: &'static str,
        kind: BorrowKind,
    },

    #[error("unsupported: {message}")]
    Unsupported { message: String },

    #[error(transparent)]
    Host(Box<dyn StdError + Send + Sync>),

    #[error("execution timed out")]
    Timeout,

    #[error("execution cancelled")]
    Cancelled,

    #[error("execution interrupted")]
    Interrupted,

    #[error("guest is closed")]
    Closed,

    #[error("iteration stopped")]
    StopIteration,

    #[error("async iteration stopped")]
    StopAsyncIteration,

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("unexpected error: {message}")]
    Unexpected {
        message: String,
        #[source]
        source: Option<Box<dyn StdError + Send + Sync>>,
    },
}

impl Error {
    pub fn guest(exception: impl Into<GuestException>) -> Self {
        Self::Guest(Box::new(exception.into()))
    }

    pub fn engine(message: impl Into<String>) -> Self {
        Self::Engine { message: message.into(), source: None }
    }

    pub fn sourced_engine(
        message: impl Into<String>,
        source: impl Into<Box<dyn StdError + Send + Sync>>,
    ) -> Self {
        Self::Engine {
            message: message.into(),
            source: Some(source.into()),
        }
    }

    pub fn conversion(message: impl Into<String>) -> Self {
        Self::Conversion { message: message.into(), source: None }
    }

    pub fn sourced_conversion(
        message: impl Into<String>,
        source: impl Into<Box<dyn StdError + Send + Sync>>,
    ) -> Self {
        Self::Conversion {
            message: message.into(),
            source: Some(source.into()),
        }
    }

    pub fn import(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Import {
            name: name.into(),
            message: message.into(),
        }
    }

    pub fn attribute(name: impl Into<String>) -> Self {
        Self::Attribute { name: name.into() }
    }

    pub fn bundle(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Bundle {
            path: path.into(),
            message: message.into(),
        }
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::Unsupported { message: message.into() }
    }

    pub fn host(error: impl Into<Box<dyn StdError + Send + Sync>>) -> Self {
        Self::Host(error.into())
    }

    pub fn unexpected(message: impl Into<String>) -> Self {
        Self::Unexpected { message: message.into(), source: None }
    }

    pub fn sourced_unexpected(
        message: impl Into<String>,
        source: impl Into<Box<dyn StdError + Send + Sync>>,
    ) -> Self {
        Self::Unexpected {
            message: message.into(),
            source: Some(source.into()),
        }
    }

    pub fn is_fatal(&self) -> bool {
        matches!(self, Self::Timeout | Self::Cancelled | Self::Interrupted | Self::Closed)
    }

    pub fn type_mismatch(expected: &str, actual: &str) -> Self {
        Self::conversion(format!("expected {expected}, got {actual}"))
    }
}

impl From<String> for Error {
    fn from(error: String) -> Self {
        Self::unexpected(error)
    }
}

impl From<&str> for Error {
    fn from(error: &str) -> Self {
        Self::unexpected(error)
    }
}

impl From<&String> for Error {
    fn from(error: &String) -> Self {
        Self::unexpected(error.clone())
    }
}

#[cfg(feature = "serde")]
impl ::serde::ser::Error for Error {
    fn custom<T>(message: T) -> Self
    where
        T: Display,
    {
        Self::conversion(message.to_string())
    }
}

#[cfg(feature = "serde")]
impl ::serde::de::Error for Error {
    fn custom<T>(message: T) -> Self
    where
        T: Display,
    {
        Self::conversion(message.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{ErasedRaise, Error, GuestException};

    struct Errors;

    impl Errors {
        fn value_error() -> GuestException {
            GuestException::new(
                "ValueError".to_owned(),
                "builtins.ValueError".to_owned(),
                "bad value".to_owned(),
                None,
                vec![
                    "builtins.ValueError".to_owned(),
                    "builtins.Exception".to_owned(),
                    "builtins.BaseException".to_owned(),
                    "builtins.object".to_owned(),
                ],
                None,
                None,
            )
        }
    }

    struct Payload;

    #[test]
    fn matches_is_subclass_aware() {
        let exception = Errors::value_error();

        assert!(exception.matches("ValueError"));
        assert!(exception.matches("builtins.ValueError"));
        assert!(exception.matches("Exception"));
        assert!(!exception.matches("KeyError"));
    }

    #[test]
    fn display_is_qualified() {
        assert_eq!(
            Error::guest(Errors::value_error()).to_string(),
            "guest exception: builtins.ValueError: bad value",
        );
    }

    #[test]
    fn raise_formats_without_its_payload() {
        let error =
            Error::Raise(Box::new(ErasedRaise::new("ExampleError".to_owned(), Box::new(Payload))));

        assert_eq!(error.to_string(), "raised ExampleError");
        assert_eq!(format!("{error:?}"), "Raise(ErasedRaise { class: \"ExampleError\", .. })",);
    }

    #[test]
    fn fatal_variants() {
        assert!(Error::Timeout.is_fatal());
        assert!(Error::Cancelled.is_fatal());
        assert!(Error::Interrupted.is_fatal());
        assert!(Error::Closed.is_fatal());
        assert!(!Error::unexpected("not fatal").is_fatal());
    }

    #[test]
    fn stop_iteration_variants_are_not_fatal() {
        assert!(!Error::StopIteration.is_fatal());
        assert!(!Error::StopAsyncIteration.is_fatal());
    }
}
