//! Re-exports the public guestpy API.

#[allow(unused_imports)]
pub use guestpy_core::{
    backend::{
        Backend, BackendCallables, BackendClasses, BackendCoroutines, BackendExceptions,
        BackendInterrupt, BackendLibrary, BackendModules, BackendValues, Step,
        callables::{HostBody, RawBody, RawCall},
    },
    bundle::*,
    driver::*,
    errors::*,
    guest::*,
    handle::*,
    host::{class::*, dunder::*, exception::*, iter::*, library::*, module::*},
    marshal::{args::*, collections::*, primitives::*},
    native::*,
    policy::*,
    runtime::*,
    scope::*,
};

pub use guestpy_macros::{FromGuest, ToGuest, guest_class, guest_module, host_class, host_module};