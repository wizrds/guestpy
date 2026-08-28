//! The RustPython backend for guestpy.

#[allow(unused_extern_crates)]
extern crate self as guestpy_rustpython;

pub mod callables;
pub mod classes;
pub mod coroutines;
pub mod engine;
pub mod errors;
pub mod exceptions;
pub mod interrupt;
pub mod library;
pub mod modules;
pub mod values;

pub use engine::{Config, RustPython};
