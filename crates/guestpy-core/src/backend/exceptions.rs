use super::{Backend, BackendValues, Tok, Val};
use crate::errors::{Error, GuestException};

pub trait BackendExceptions: Backend + BackendValues {
    type Raw;

    fn take_error<'py>(token: Tok<'py, Self>, raw: Self::Raw) -> GuestException;
    fn raise<'py>(token: Tok<'py, Self>, error: Error) -> Self::Raw;
    fn exception_object<'py>(token: Tok<'py, Self>, error: Error) -> Result<Val<'py, Self>, Error>;
    fn exception_class<'py>(token: Tok<'py, Self>, name: &str) -> Result<Val<'py, Self>, Error>;
    fn new_exception_class<'py>(
        token: Tok<'py, Self>,
        module: &str,
        name: &str,
        base: Option<&Val<'py, Self>>,
    ) -> Result<Val<'py, Self>, Error>;
}
