use super::{Backend, BackendValues, Tok, Val};

pub trait BackendExceptions: Backend + BackendValues {
    fn traceback<'py>(token: Tok<'py, Self>, exception: &Val<'py, Self>) -> Option<String>;
}
