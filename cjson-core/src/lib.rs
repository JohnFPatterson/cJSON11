//! Safe Rust port of cJSON core logic.
//!
//! All parsing, printing, and tree operations live here. The `cjson-ffi` crate
//! is a thin ABI shim over this crate. Original `cJSON.c` / `cJSON.h` remain
//! the behavioral oracle.

#![forbid(unsafe_code)]

pub mod error;
pub mod minify;
pub mod parse;
pub mod print;
pub mod tree;
pub mod value;

pub use error::{Error, Result};
pub use minify::minify;
pub use parse::{last_error_offset, parse, parse_with_opts};
pub use print::{print_formatted, print_unformatted};
pub use tree::{compare, duplicate, estimate_compare_cost};
pub use value::Value;

/// Hidden re-exports of internals for white-box Unity case ports.
#[doc(hidden)]
pub mod internals {
    pub use crate::parse::internals::*;
    pub use crate::print::internals::*;
    pub use crate::value::{case_insensitive_eq, compare_double, saturate_int};
}

pub const VERSION_MAJOR: u32 = 1;
pub const VERSION_MINOR: u32 = 7;
pub const VERSION_PATCH: u32 = 19;

pub fn version() -> String {
    format!("{VERSION_MAJOR}.{VERSION_MINOR}.{VERSION_PATCH}")
}
