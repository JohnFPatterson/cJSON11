//! Error type for malformed or rejected input. Library code never panics on bad JSON.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// Parse failed at the given byte offset into the input.
    Parse { offset: usize },
    /// Nesting deeper than `CJSON_NESTING_LIMIT` (1000).
    NestingTooDeep,
    /// Circular-reference depth exceeded `CJSON_CIRCULAR_LIMIT` (10000).
    CircularTooDeep,
    /// Allocation or capacity failure mirrored from C's NULL returns.
    Alloc,
    /// Invalid argument (NULL-equivalent, wrong type, etc.).
    InvalidArgument,
    /// Print failed (buffer too small, nesting, etc.).
    Print,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Parse { offset } => write!(f, "parse error at offset {offset}"),
            Error::NestingTooDeep => write!(f, "nesting too deep"),
            Error::CircularTooDeep => write!(f, "circular reference limit exceeded"),
            Error::Alloc => write!(f, "allocation failure"),
            Error::InvalidArgument => write!(f, "invalid argument"),
            Error::Print => write!(f, "print failure"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
