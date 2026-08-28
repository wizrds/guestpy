//! Guest import resolution.

mod bindings;
mod name;
mod realise;

pub(crate) use bindings::GuestBindings;

use crate::{
    backend::{Backend, BackendCallables, BackendModules, BackendValues},
    bundle::Bundle,
    errors::Error,
    handle::Value,
    marshal::{ToGuest, args::Args},
    scope::Enter,
};

use name::DottedName;
use realise::Realiser;

pub(crate) struct Imports<'py, 'e, B: Backend> {
    enter: &'e Enter<'py, B>,
}

impl<'py, 'e, B> Imports<'py, 'e, B>
where
    B: Backend + BackendValues + BackendCallables + BackendModules,
{
    pub(crate) fn new(enter: &'e Enter<'py, B>) -> Self {
        Self { enter }
    }

    fn from_list(&self, value: Option<&B::Value<'py>>) -> Result<Vec<String>, Error> {
        let Some(value) = value else {
            return Ok(Vec::new());
        };
        let iterator = B::iter(self.enter.token(), value)?;
        let mut names = Vec::new();

        while let Some(entry) = B::next(self.enter.token(), &iterator)? {
            names.push(B::as_str(self.enter.token(), &entry)?);
        }

        Ok(names)
    }

    fn expand(&self, module: &B::Value<'py>, dotted: &str, names: &[String]) -> Result<(), Error> {
        let mut wanted = Vec::new();

        for name in names {
            if name != "*" {
                wanted.push(name.clone());
            } else if B::has_attr(self.enter.token(), module, "__all__") {
                wanted.extend(
                    self.from_list(Some(&B::get_attr(self.enter.token(), module, "__all__")?))?
                        .into_iter()
                        .filter(|entry| entry != "*"),
                );
            }
        }

        for name in wanted {
            if B::has_attr(self.enter.token(), module, &name) {
                continue;
            }

            if let Err(error) = Realiser::new(self.enter).module(&format!("{dotted}.{name}"))
                && !matches!(error, Error::Import { .. })
            {
                return Err(error);
            }
        }

        Ok(())
    }

    fn package(&self, globals: Option<&B::Value<'py>>) -> Result<String, Error> {
        let Some(globals) = globals else {
            return Ok(String::new());
        };

        match B::get_item_opt(
            self.enter.token(),
            globals,
            &B::str(self.enter.token(), "__package__"),
        )? {
            Some(value) if !B::is_none(self.enter.token(), &value) => {
                B::as_str(self.enter.token(), &value)
            }
            _ => Ok(String::new()),
        }
    }

    fn optional(
        &self,
        args: &Args<'py, B>,
        index: usize,
        name: &str,
    ) -> Result<Option<B::Value<'py>>, Error> {
        Ok(args
            .optional::<Value<B>>(self.enter, index, name)?
            .map(|value| value.to_guest(self.enter))
            .transpose()?
            .filter(|value| !B::is_none(self.enter.token(), value)))
    }

    fn delegate(
        &self,
        name: &str,
        globals: Option<B::Value<'py>>,
        locals: Option<B::Value<'py>>,
        fromlist: Option<B::Value<'py>>,
        level: Option<B::Value<'py>>,
    ) -> Result<B::Value<'py>, Error> {
        B::call(
            self.enter.token(),
            &B::attach(self.enter.token(), self.enter.guest().real_import()),
            &[
                B::str(self.enter.token(), name),
                globals.unwrap_or_else(|| B::none(self.enter.token())),
                locals.unwrap_or_else(|| B::none(self.enter.token())),
                fromlist.unwrap_or_else(|| B::none(self.enter.token())),
                level.unwrap_or_else(|| B::uint(self.enter.token(), 0)),
            ],
            &[],
        )
    }

    pub(crate) fn is_host_module(&self, dotted: &str) -> bool {
        self.enter
            .guest()
            .bindings()
            .is_host_module(dotted)
    }

    pub(crate) fn mount(&self, bundle: &Bundle, root: &str) -> Result<(), Error> {
        self.enter
            .guest()
            .bindings()
            .mount(bundle, root)
    }

    pub(crate) fn module(&self, dotted: &str) -> Result<B::Value<'py>, Error> {
        if self
            .enter
            .guest()
            .bindings()
            .is_denied(dotted)
        {
            return Err(Error::import(dotted, "this module is denied to this guest"));
        }

        Realiser::new(self.enter).module(dotted)
    }

    pub(crate) fn dispatch(&self, args: &Args<'py, B>) -> Result<B::Value<'py>, Error> {
        let name = args.required::<String>(self.enter, 0, "name")?;
        let globals = self.optional(args, 1, "globals")?;
        let locals = self.optional(args, 2, "locals")?;
        let fromlist = self.optional(args, 3, "fromlist")?;
        let level = self.optional(args, 4, "level")?;
        let depth = match &level {
            Some(value) => usize::try_from(B::as_u64(self.enter.token(), value)?)
                .map_err(|_| Error::import(&name, "the import level is out of range"))?,
            None => 0,
        };
        let resolved = if depth == 0 {
            name.clone()
        } else {
            let package = self.package(globals.as_ref())?;

            if package.is_empty() {
                return self.delegate(&name, globals, locals, fromlist, level);
            }

            DottedName::absolutise(&package, &name, depth)?
        };

        if self
            .enter
            .guest()
            .bindings()
            .is_denied(&resolved)
        {
            return Err(Error::import(resolved, "this module is denied to this guest"));
        }

        if !self
            .enter
            .guest()
            .bindings()
            .contains(DottedName(&resolved).head())
        {
            return self.delegate(&name, globals, locals, fromlist, level);
        }

        let module = Realiser::new(self.enter).module(&resolved)?;
        let names = self.from_list(fromlist.as_ref())?;

        if names.is_empty() {
            return self
                .enter
                .guest()
                .bindings()
                .cached(DottedName(&resolved).head())
                .map(|owned| B::attach(self.enter.token(), &owned))
                .ok_or_else(|| Error::import(resolved, "its head module is not realised"));
        }

        self.expand(&module, &resolved, &names)?;

        Ok(module)
    }
}
