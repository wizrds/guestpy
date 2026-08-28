use super::{Backend, Tok, Val};
use crate::errors::Error;

pub trait BackendLibrary: Backend {
    type NativeModule: Clone + 'static;

    fn declare_native<'py>(
        token: Tok<'py, Self>,
        native: &Self::NativeModule,
        name: &str,
    ) -> Result<Val<'py, Self>, Error>;
}
