use darling::{FromMeta, util::Flag};
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{
    FnArg, GenericArgument, Ident, ImplItemFn, Pat, PatType, PathArguments, ReceiverKind,
    ReturnType, Safety, Signature, Type, spanned::Spanned,
};

use crate::{attributes::HelperAttributes, host::HostMacroError};

struct TypeShape;

impl TypeShape {
    fn inner(value_type: &Type, name: &str) -> Option<Type> {
        let Type::Path(path) = value_type else {
            return None;
        };

        let segment = path.path.segments.last()?;

        if segment.ident != name {
            return None;
        }

        let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
            return None;
        };

        if arguments.args.len() != 1 {
            return None;
        }

        match arguments.args.first()? {
            GenericArgument::Type(inner) => Some(inner.clone()),
            _ => None,
        }
    }

    fn reference_target(value_type: &Type, mutable: bool) -> Result<Type, HostMacroError> {
        match value_type {
            Type::Reference(reference) if reference.mutability.is_some() == mutable => {
                Ok(reference.elem.as_ref().clone())
            }
            _ if mutable => Err(syn::Error::new(
                value_type.span(),
                "a #[guestpy(borrow_mut)] parameter must have type &mut T",
            )
            .into()),
            _ => Err(syn::Error::new(
                value_type.span(),
                "a #[guestpy(borrow)] parameter must have type &T",
            )
            .into()),
        }
    }
}

#[derive(Default, FromMeta)]
#[darling(default)]
struct ParameterOptions {
    kw: Flag,
    rest: Flag,
    borrow: Flag,
    borrow_mut: Flag,
    enter: Flag,
}

impl ParameterOptions {
    fn role_count(&self) -> usize {
        [
            self.kw.is_present(),
            self.rest.is_present(),
            self.borrow.is_present(),
            self.borrow_mut.is_present(),
            self.enter.is_present(),
        ]
        .into_iter()
        .filter(|present| *present)
        .count()
    }
}

enum ParameterRole {
    Value { descriptor: Type, optional: bool },
    Keyword { descriptor: Type, optional: bool },
    Rest { descriptor: Type },
    Borrow { value_type: Type, mutable: bool },
    Enter,
}

struct ResultType;

impl ResultType {
    fn validate(output: &ReturnType) -> Result<(), HostMacroError> {
        let ReturnType::Type(_, value_type) = output else {
            return Err(syn::Error::new(
                output.span(),
                "an exported host callable must return Result<R, E>",
            )
            .into());
        };

        if matches!(
            value_type.as_ref(),
            Type::Path(path)
                if path.path.segments.last().is_some_and(|segment| segment.ident == "Result")
        ) {
            Ok(())
        } else {
            Err(syn::Error::new(
                value_type.span(),
                "an exported host callable must return Result<R, E>",
            )
            .into())
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Receiver {
    None,
    Shared,
    Exclusive,
}

impl Receiver {
    pub(crate) fn of(signature: &Signature) -> Result<Self, HostMacroError> {
        match signature.receiver() {
            None => Ok(Self::None),
            Some(receiver) => match &receiver.kind {
                ReceiverKind::Reference(_, _, Some(_)) => Ok(Self::Exclusive),
                ReceiverKind::Reference(..) => Ok(Self::Shared),
                _ => Err(syn::Error::new_spanned(
                    receiver,
                    "an exported host method must borrow its receiver (&self or &mut self)",
                )
                .into()),
            },
        }
    }
}

pub(crate) struct Parameter {
    name: String,
    guest_index: usize,
    role: ParameterRole,
}

impl Parameter {
    fn new(argument: &mut PatType, guest_index: usize) -> Result<Self, HostMacroError> {
        let options = ParameterOptions::from_list(&HelperAttributes::take(&mut argument.attrs)?)?;

        if options.role_count() > 1 {
            return Err(syn::Error::new(
                argument.span(),
                "a host parameter may declare only one of kw, rest, borrow, borrow_mut, enter",
            )
            .into());
        }

        let Pat::Ident(pattern) = argument.pat.as_ref() else {
            return Err(syn::Error::new_spanned(
                argument.pat.as_ref(),
                "a host parameter must use an identifier pattern",
            )
            .into());
        };

        let value_type = argument.ty.as_ref();
        let role = if options.enter.is_present() {
            ParameterRole::Enter
        } else if options.borrow.is_present() {
            ParameterRole::Borrow {
                value_type: TypeShape::reference_target(value_type, false)?,
                mutable: false,
            }
        } else if options.borrow_mut.is_present() {
            ParameterRole::Borrow {
                value_type: TypeShape::reference_target(value_type, true)?,
                mutable: true,
            }
        } else if options.rest.is_present() {
            ParameterRole::Rest {
                descriptor: TypeShape::inner(value_type, "Vec").ok_or_else(|| {
                    syn::Error::new(
                        value_type.span(),
                        "a #[guestpy(rest)] parameter must have type Vec<T>",
                    )
                })?,
            }
        } else if options.kw.is_present() {
            match TypeShape::inner(value_type, "Option") {
                Some(descriptor) => ParameterRole::Keyword { descriptor, optional: true },
                None => ParameterRole::Keyword {
                    descriptor: value_type.clone(),
                    optional: false,
                },
            }
        } else {
            match TypeShape::inner(value_type, "Option") {
                Some(descriptor) => ParameterRole::Value { descriptor, optional: true },
                None => ParameterRole::Value {
                    descriptor: value_type.clone(),
                    optional: false,
                },
            }
        };

        Ok(Self {
            name: pattern.ident.to_string(),
            guest_index,
            role,
        })
    }

    fn parse_all(signature: &mut Signature) -> Result<Vec<Self>, HostMacroError> {
        let mut parameters = Vec::new();
        let mut positional = 0;

        for argument in &mut signature.inputs {
            if let FnArg::Typed(typed) = argument {
                let parameter = Self::new(typed, positional)?;

                if parameter.consumes_positional() {
                    positional += 1;
                }

                parameters.push(parameter);
            }
        }

        if parameters
            .iter()
            .filter(|parameter| parameter.is_rest())
            .count()
            > 1
        {
            return Err(syn::Error::new(
                signature.inputs.span(),
                "a host callable may have only one rest parameter",
            )
            .into());
        }

        if let Some(position) = parameters
            .iter()
            .position(Self::is_rest)
        {
            if position + 1 != parameters.len() {
                return Err(syn::Error::new(
                    signature.inputs.span(),
                    "a rest parameter must be the last parameter",
                )
                .into());
            }
        }

        Ok(parameters)
    }

    fn consumes_positional(&self) -> bool {
        matches!(
            self.role,
            ParameterRole::Value { .. } | ParameterRole::Rest { .. } | ParameterRole::Borrow { .. }
        )
    }

    pub(crate) fn consumes_arg(&self) -> bool {
        !matches!(self.role, ParameterRole::Enter)
    }

    pub(crate) fn is_enter(&self) -> bool {
        matches!(self.role, ParameterRole::Enter)
    }

    fn is_rest(&self) -> bool {
        matches!(self.role, ParameterRole::Rest { .. })
    }

    fn expression(&self) -> TokenStream {
        let name = &self.name;
        let index = self.guest_index;

        match &self.role {
            ParameterRole::Value { descriptor, optional: false } => {
                quote!(
                    __guestpy_args
                        .required::<#descriptor>(
                            __guestpy_enter,
                            #index,
                            #name,
                        )?
                )
            }
            ParameterRole::Value { descriptor, optional: true } => {
                quote!(
                    __guestpy_args
                        .optional::<#descriptor>(
                            __guestpy_enter,
                            #index,
                            #name,
                        )?
                )
            }
            ParameterRole::Keyword { descriptor, optional: false } => {
                quote!(
                    __guestpy_args
                        .required_keyword::<#descriptor>(
                            __guestpy_enter,
                            #name,
                        )?
                )
            }
            ParameterRole::Keyword { descriptor, optional: true } => {
                quote!(
                    __guestpy_args
                        .optional_keyword::<#descriptor>(
                            __guestpy_enter,
                            #name,
                        )?
                )
            }
            ParameterRole::Rest { descriptor } => {
                quote!(
                    __guestpy_args
                        .rest::<#descriptor>(
                            __guestpy_enter,
                            #index,
                        )?
                )
            }
            ParameterRole::Borrow { value_type, mutable: false } => {
                quote!(
                    &*__guestpy_args
                        .borrow::<#value_type>(
                            __guestpy_enter,
                            #index,
                        )?
                )
            }
            ParameterRole::Borrow { value_type, mutable: true } => {
                quote!(
                    &mut *__guestpy_args
                        .borrow_mut::<#value_type>(
                            __guestpy_enter,
                            #index,
                        )?
                )
            }
            ParameterRole::Enter => quote!(__guestpy_enter),
        }
    }

    pub(crate) fn is_rest_or_borrow_or_kw(&self) -> bool {
        matches!(
            self.role,
            ParameterRole::Rest { .. }
                | ParameterRole::Borrow { .. }
                | ParameterRole::Keyword { .. }
        )
    }

    fn accessor_expression(&self) -> TokenStream {
        match &self.role {
            ParameterRole::Value { .. } => quote!(__guestpy_value),
            ParameterRole::Enter => quote!(__guestpy_enter),
            ParameterRole::Keyword { .. }
            | ParameterRole::Rest { .. }
            | ParameterRole::Borrow { .. } => unreachable!(),
        }
    }
}

pub(crate) struct Callable {
    span: Span,
    ident: Ident,
    name: String,
    receiver: Receiver,
    asynchronous: bool,
    parameters: Vec<Parameter>,
}

impl Callable {
    pub(crate) fn parse(method: &mut ImplItemFn, name: String) -> Result<Self, HostMacroError> {
        Self::validate_signature(method)?;

        let receiver = Receiver::of(&method.sig)?;
        let parameters = Parameter::parse_all(&mut method.sig)?;

        ResultType::validate(&method.sig.output)?;

        Ok(Self {
            span: method.sig.ident.span(),
            ident: method.sig.ident.clone(),
            name,
            receiver,
            asynchronous: method.sig.asyncness.is_some(),
            parameters,
        })
    }

    fn validate_signature(method: &ImplItemFn) -> Result<(), HostMacroError> {
        if let Safety::Unsafe(safety) = &method.sig.safety {
            return Err(
                syn::Error::new_spanned(safety, "unsafe host callables are not supported").into()
            );
        }

        if !method.sig.generics.params.is_empty() {
            return Err(syn::Error::new(
                method.sig.generics.span(),
                "an exported host callable cannot be generic",
            )
            .into());
        }

        if method.sig.variadic.is_some() {
            return Err(syn::Error::new_spanned(
                &method.sig.variadic,
                "variadic host callables are not supported",
            )
            .into());
        }

        Ok(())
    }

    pub(crate) fn span(&self) -> Span {
        self.span
    }

    pub(crate) fn ident(&self) -> &Ident {
        &self.ident
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn receiver(&self) -> Receiver {
        self.receiver
    }

    pub(crate) fn asynchronous(&self) -> bool {
        self.asynchronous
    }

    pub(crate) fn parameters(&self) -> &[Parameter] {
        &self.parameters
    }

    pub(crate) fn uses_args(&self) -> bool {
        self.parameters
            .iter()
            .any(Parameter::consumes_arg)
    }

    pub(crate) fn uses_enter(&self) -> bool {
        self.uses_args()
            || self
                .parameters
                .iter()
                .any(Parameter::is_enter)
    }

    pub(crate) fn enter_ident(&self) -> Ident {
        Ident::new(
            if self.uses_enter() {
                "__guestpy_enter"
            } else {
                "_enter"
            },
            Span::call_site(),
        )
    }

    pub(crate) fn args_ident(&self) -> Ident {
        Ident::new(
            if self.uses_args() {
                "__guestpy_args"
            } else {
                "_args"
            },
            Span::call_site(),
        )
    }

    pub(crate) fn argument_setup(&self) -> TokenStream {
        let args = self.args_ident();
        let bindings = self.argument_bindings();
        let expressions = self.argument_expressions();

        quote! {
            #(let #bindings = #expressions;)*

            #args.finish()?;
        }
    }

    pub(crate) fn argument_expressions(&self) -> Vec<TokenStream> {
        self.parameters
            .iter()
            .map(Parameter::expression)
            .collect()
    }

    pub(crate) fn argument_bindings(&self) -> Vec<Ident> {
        (0..self.parameters.len())
            .map(|index| format_ident!("__guestpy_arg{index}"))
            .collect()
    }

    pub(crate) fn accessor_expressions(&self) -> Vec<TokenStream> {
        self.parameters
            .iter()
            .map(Parameter::accessor_expression)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use syn::parse_quote;

    use super::Callable;

    #[test]
    fn argument_setup_parses_every_argument_before_finish() {
        let mut method = parse_quote! {
            fn call(
                required: i64,
                optional: Option<String>,
                #[guestpy(kw)] named: bool,
            ) -> Result<(), Error> {
                Ok(())
            }
        };
        let output = Callable::parse(
            &mut method,
            String::from("call"),
        )
        .unwrap()
        .argument_setup()
        .to_string();

        assert!(output.contains("required :: < i64 >"));
        assert!(output.contains("optional :: < String >"));
        assert!(output.contains("required_keyword :: < bool >"));
        assert_eq!(output.matches("finish () ?").count(), 1);
    }
}
