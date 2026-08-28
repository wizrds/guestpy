//! Backend-generic core of guestpy: the engine contract, the host-facing API, and the driver.

#[allow(unused_extern_crates)]
extern crate self as guestpy_core;

pub mod backend;
pub mod bundle;
pub mod driver;
pub mod errors;
pub mod guest;
pub mod handle;
pub mod host;
pub mod marshal;
pub mod native;
pub mod policy;
pub mod runtime;
pub mod scope;

pub(crate) mod catalog;
pub(crate) mod imports;

#[cfg(feature = "embedded")]
pub mod embed {
    pub use include_dir::{Dir, DirEntry, File};

    #[doc(hidden)]
    pub mod __include_dir {
        pub use include_dir::{Dir, DirEntry, File};
    }

    #[doc(hidden)]
    pub use include_dir::include_dir as __include_dir_macro;
}
