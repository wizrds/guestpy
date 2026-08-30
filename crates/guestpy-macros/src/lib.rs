#[allow(unused_extern_crates)]
extern crate self as guestpy_macros;

mod attributes;
mod bundle;
mod derive;
mod guest;
mod host;
mod naming;
mod path;

use bundle::BundleMacro;
use derive::GuestDerive;
use guest::{GuestClassMacro, GuestModuleMacro};
use host::{HostClassMacro, HostModuleMacro};
use proc_macro::TokenStream;
use syn::{DeriveInput, ItemImpl, parse_macro_input};

#[proc_macro_derive(ToGuest, attributes(guestpy))]
pub fn derive_to_guest(input: TokenStream) -> TokenStream {
    match GuestDerive::new(&parse_macro_input!(input as DeriveInput)) {
        Ok(derive) => derive.to_guest(),
        Err(error) => error.write_errors(),
    }
    .into()
}

#[proc_macro_derive(FromGuest, attributes(guestpy))]
pub fn derive_from_guest(input: TokenStream) -> TokenStream {
    match GuestDerive::new(&parse_macro_input!(input as DeriveInput)) {
        Ok(derive) => derive.from_guest(),
        Err(error) => error.write_errors(),
    }
    .into()
}

#[proc_macro_attribute]
pub fn host_class(args: TokenStream, input: TokenStream) -> TokenStream {
    match HostClassMacro::new(args.into(), parse_macro_input!(input as ItemImpl)) {
        Ok(host_class) => host_class.expand(),
        Err(error) => error.write_errors(),
    }
    .into()
}

#[proc_macro_attribute]
pub fn host_module(args: TokenStream, input: TokenStream) -> TokenStream {
    match HostModuleMacro::new(args.into(), parse_macro_input!(input as ItemImpl)) {
        Ok(host_module) => host_module.expand(),
        Err(error) => error.write_errors(),
    }
    .into()
}

#[proc_macro]
pub fn bundle(input: TokenStream) -> TokenStream {
    match BundleMacro::new(input.into()) {
        Ok(bundle) => bundle.expand(),
        Err(error) => error.into_compile_error(),
    }
    .into()
}

#[proc_macro]
pub fn guest_class(input: TokenStream) -> TokenStream {
    match GuestClassMacro::new(input.into()) {
        Ok(guest_class) => guest_class.expand(),
        Err(error) => error.write_errors(),
    }
    .into()
}

#[proc_macro]
pub fn guest_module(input: TokenStream) -> TokenStream {
    match GuestModuleMacro::new(input.into()) {
        Ok(guest_module) => guest_module.expand(),
        Err(error) => error.write_errors(),
    }
    .into()
}
