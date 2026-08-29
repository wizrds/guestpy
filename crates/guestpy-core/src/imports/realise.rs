use std::rc::Rc;

use crate::{
    backend::{
        Backend, BackendCallables, BackendModules, BackendValues, NativeExtensionContext,
        PreparedNativeExtensions, PreparedNativeExtensionsOf,
    },
    bundle::Bundle,
    errors::Error,
    host::{declaration::DeclarationContext, module::ModuleSpec},
    imports::name::DottedName,
    native::NativeModule,
    scope::Enter,
};

pub(super) struct Realiser<'py, 'e, B: Backend> {
    enter: &'e Enter<'py, B>,
}

impl<'py, 'e, B> Realiser<'py, 'e, B>
where
    B: Backend + BackendValues + BackendCallables + BackendModules,
{
    pub(super) fn new(enter: &'e Enter<'py, B>) -> Self {
        Self { enter }
    }

    fn code_for(
        &self,
        bundle: &Bundle,
        dotted: &str,
        origin: &str,
    ) -> Result<B::Value<'py>, Error> {
        if let Some(code) = self
            .enter
            .guest()
            .realisation()
            .compiled(bundle.id(), dotted)
        {
            return Ok(B::attach(self.enter.token(), &code));
        }

        let module = bundle
            .module(dotted)
            .ok_or_else(|| Error::import(dotted, "the bundle has no module of that name"))?;
        let code = B::compile(self.enter.token(), module.source(), origin)?;

        self.enter
            .guest()
            .realisation()
            .cache_compiled(bundle.id(), dotted, B::detach(self.enter.token(), code.clone()));

        Ok(code)
    }

    fn cached(&self, dotted: &str) -> Option<B::Value<'py>> {
        self.enter
            .guest()
            .bindings()
            .cached(dotted)
            .map(|owned| B::attach(self.enter.token(), &owned))
    }

    fn cache(&self, dotted: &str, module: &B::Value<'py>) {
        self.enter
            .guest()
            .bindings()
            .cache(dotted, B::detach(self.enter.token(), module.clone()));
    }

    fn realised_parents(&self, dotted: &str) -> Result<Vec<(String, B::Value<'py>)>, Error> {
        let mut parents = Vec::new();

        for prefix in DottedName(dotted).prefixes() {
            if prefix == dotted {
                break;
            }

            parents.push((
                prefix.to_owned(),
                self.cached(prefix).ok_or_else(|| {
                    Error::import(dotted, format!("parent module {prefix} is not realised"))
                })?,
            ));
        }

        Ok(parents)
    }

    fn module_getattr(&self, spec: &Rc<ModuleSpec<B>>) -> Result<B::Value<'py>, Error> {
        B::function(
            self.enter.token(),
            "__getattr__",
            None,
            self.enter.guest().raw_body({
                let spec = spec.clone();

                Rc::new(move |enter, args| {
                    let name = args.required::<String>(enter, 0, "name")?;
                    let context = DeclarationContext::new(enter);
                    let getter = spec
                        .members()
                        .iter()
                        .find_map(|(member_name, member)| {
                            (*member_name == name)
                                .then(|| member.module_getter())
                                .flatten()
                        })
                        .ok_or_else(|| Error::attribute(&name))?;

                    getter(&context)
                })
            }),
        )
    }

    fn seed_module_tail(&self, dict: &B::Value<'py>, package: &str) -> Result<(), Error> {
        B::set_item(
            self.enter.token(),
            dict,
            B::str(self.enter.token(), "__package__"),
            B::str(self.enter.token(), package),
        )?;
        B::set_item(
            self.enter.token(),
            dict,
            B::str(self.enter.token(), "__loader__"),
            B::none(self.enter.token()),
        )?;
        B::set_item(
            self.enter.token(),
            dict,
            B::str(self.enter.token(), "__spec__"),
            B::none(self.enter.token()),
        )?;

        Ok(())
    }

    fn realise_host(&self, spec: &Rc<ModuleSpec<B>>) -> Result<B::Value<'py>, Error> {
        let module = B::new_module(
            self.enter.token(),
            spec.name(),
            B::new_dict(self.enter.token())?,
            spec.docstring(),
        )?;
        let dict = B::get_attr(self.enter.token(), &module, "__dict__")?;

        B::set_item(
            self.enter.token(),
            &dict,
            B::str(self.enter.token(), "__name__"),
            B::str(self.enter.token(), spec.name()),
        )?;
        B::set_item(
            self.enter.token(),
            &dict,
            B::str(self.enter.token(), "__doc__"),
            spec.docstring()
                .map_or_else(|| B::none(self.enter.token()), |doc| B::str(self.enter.token(), doc)),
        )?;
        B::set_item(
            self.enter.token(),
            &dict,
            B::str(self.enter.token(), "__builtins__"),
            B::context_builtins(self.enter.token(), self.enter.guest().context()),
        )?;
        self.seed_module_tail(&dict, "")?;

        self.cache(spec.name(), &module);

        let context = DeclarationContext::new(self.enter);

        for (name, member) in spec.members() {
            if member.module_getter().is_some() {
                continue;
            }

            B::set_attr(self.enter.token(), &module, name, member.realise(&context, name)?)?;
        }

        if spec
            .members()
            .iter()
            .any(|(_, member)| member.module_getter().is_some())
        {
            B::set_attr(self.enter.token(), &module, "__getattr__", self.module_getattr(spec)?)?;
        }

        Ok(module)
    }

    fn realise_native(
        &self,
        native: &Rc<NativeModule<B>>,
        dotted: &str,
    ) -> Result<B::Value<'py>, Error> {
        let module = native.declare(self.enter.token(), dotted)?;

        self.cache(dotted, &module);

        Ok(module)
    }

    fn realise_bundle(&self, bundle: &Bundle, dotted: &str) -> Result<B::Value<'py>, Error> {
        let source = bundle
            .module(dotted)
            .ok_or_else(|| Error::import(dotted, "the bundle has no module of that name"))?;
        let extensions = self
            .enter
            .guest()
            .bindings()
            .prepared(bundle.id())
            .ok_or_else(|| {
                Error::unexpected("a bound bundle has no prepared native-extension state")
            })?;
        let origin = extensions
            .source_origin(source.origin())
            .unwrap_or_else(|| source.origin().to_owned());
        let module =
            B::new_module(self.enter.token(), dotted, B::new_dict(self.enter.token())?, None)?;
        let dict = B::get_attr(self.enter.token(), &module, "__dict__")?;

        B::set_item(
            self.enter.token(),
            &dict,
            B::str(self.enter.token(), "__name__"),
            B::str(self.enter.token(), dotted),
        )?;
        B::set_item(
            self.enter.token(),
            &dict,
            B::str(self.enter.token(), "__builtins__"),
            B::context_builtins(self.enter.token(), self.enter.guest().context()),
        )?;
        B::set_item(
            self.enter.token(),
            &dict,
            B::str(self.enter.token(), "__file__"),
            B::str(self.enter.token(), &origin),
        )?;
        self.seed_module_tail(
            &dict,
            if source.is_package() {
                dotted
            } else {
                DottedName(dotted)
                    .parent()
                    .map_or("", |(parent, _)| parent)
            },
        )?;

        if source.is_package() {
            B::set_item(
                self.enter.token(),
                &dict,
                B::str(self.enter.token(), "__path__"),
                B::list(
                    self.enter.token(),
                    extensions
                        .package_path(dotted)
                        .map(|path| vec![B::str(self.enter.token(), &path)])
                        .unwrap_or_default(),
                )?,
            )?;
        }

        self.cache(dotted, &module);
        B::exec_code(self.enter.token(), &self.code_for(bundle, dotted, &origin)?, &dict)?;

        Ok(module)
    }

    fn realise_extension(
        &self,
        extensions: &PreparedNativeExtensionsOf<B>,
        dotted: &str,
    ) -> Result<B::Value<'py>, Error> {
        let module = extensions.realise(
            NativeExtensionContext::new(self.enter.token(), self.realised_parents(dotted)?),
            dotted,
        )?;

        self.cache(dotted, &module);

        Ok(module)
    }

    fn realise(&self, dotted: &str) -> Result<B::Value<'py>, Error> {
        if let Some(module) = self.cached(dotted) {
            return Ok(module);
        }

        let result = if let Some(spec) = self
            .enter
            .guest()
            .bindings()
            .spec(dotted)
        {
            self.realise_host(&spec)
        } else if let Some(native) = self
            .enter
            .guest()
            .bindings()
            .native(dotted)
        {
            self.realise_native(&native, dotted)
        } else if let Some(bundle) = self
            .enter
            .guest()
            .bindings()
            .source(dotted)
        {
            self.realise_bundle(&bundle, dotted)
        } else if let Some(extensions) = self
            .enter
            .guest()
            .bindings()
            .extension(dotted)
        {
            self.realise_extension(&extensions, dotted)
        } else {
            return Err(Error::import(dotted, "no module of that name is available to this guest"));
        };

        if result.is_err()
            && let Some(module) = self
                .enter
                .guest()
                .bindings()
                .remove_cached(dotted)
        {
            B::release(module);
        }

        result
    }

    pub(super) fn module(&self, dotted: &str) -> Result<B::Value<'py>, Error> {
        let mut module = None;

        for prefix in DottedName(dotted).prefixes() {
            let realised = self.realise(prefix)?;

            if let Some((parent, last)) = DottedName(prefix).parent() {
                B::set_attr(
                    self.enter.token(),
                    &self
                        .cached(parent)
                        .ok_or_else(|| Error::import(prefix, "its parent is not realised"))?,
                    last,
                    realised.clone(),
                )?;
            }

            module = Some(realised);
        }

        module.ok_or_else(|| Error::import(dotted, "the module name is empty"))
    }
}
