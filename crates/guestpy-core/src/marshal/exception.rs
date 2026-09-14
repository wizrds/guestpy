use crate::{
    backend::{Backend, BackendExceptions, BackendValues, Tok, Val},
    errors::GuestException,
    marshal::FromException,
};

impl<B> FromException<B> for GuestException
where
    B: Backend + BackendValues + BackendExceptions,
{
    fn from_exception<'py>(token: Tok<'py, B>, exception: Val<'py, B>) -> Self {
        Self::describe::<B>(token, exception.clone(), B::traceback(token, &exception))
    }
}
