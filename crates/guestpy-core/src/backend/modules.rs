use super::{Backend, BackendValues, Tok, Val};
use crate::errors::Error;

pub trait BackendModules: Backend + BackendValues {
    fn new_module<'py>(
        token: Tok<'py, Self>,
        name: &str,
        dict: Val<'py, Self>,
        doc: Option<&str>,
    ) -> Result<Val<'py, Self>, Error>;

    fn compile<'py>(
        token: Tok<'py, Self>,
        source: &str,
        filename: &str,
    ) -> Result<Val<'py, Self>, Error>;

    fn exec_code<'py>(
        token: Tok<'py, Self>,
        code: &Val<'py, Self>,
        globals: &Val<'py, Self>,
    ) -> Result<(), Error>;

    fn exec<'py>(
        token: Tok<'py, Self>,
        source: &str,
        filename: &str,
        globals: &Val<'py, Self>,
    ) -> Result<(), Error> {
        let code = Self::compile(token, source, filename)?;

        Self::exec_code(token, &code, globals)
    }

    fn eval<'py>(
        token: Tok<'py, Self>,
        source: &str,
        filename: &str,
        globals: &Val<'py, Self>,
    ) -> Result<Val<'py, Self>, Error>;

    fn builtins_dict<'py>(token: Tok<'py, Self>) -> Result<Val<'py, Self>, Error>;

    fn install_dispatcher<'py>(
        token: Tok<'py, Self>,
        engine: &Self::Engine,
        dispatcher: Val<'py, Self>,
    ) -> Result<(), Error>;

    fn real_import<'py>(token: Tok<'py, Self>) -> Result<Val<'py, Self>, Error>;
}
