use super::{Backend, Tok};
use crate::errors::Error;

pub trait BackendInterrupt: Backend {
    type Handle: Send + Sync + Clone + 'static;

    fn handle(engine: &Self::Engine) -> Self::Handle;
    fn request(handle: &Self::Handle);
    fn check<'py>(token: Tok<'py, Self>) -> Result<(), Error>;
    fn reset(engine: &Self::Engine);
}
