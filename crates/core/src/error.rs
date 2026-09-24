use std::fmt;

use crate::git::Oid;

#[derive(Debug)]
pub enum Error {
    Corrupt(&'static str),
    Missing(Oid),
    Unsupported(&'static str),
    Protocol(String),
    OutOfMemory,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Corrupt(what) => write!(f, "corrupt git data: {what}"),
            Error::Missing(oid) => write!(f, "object {oid} not found"),
            Error::Unsupported(what) => write!(f, "unsupported: {what}"),
            Error::Protocol(msg) => write!(f, "git protocol: {msg}"),
            Error::OutOfMemory => f.write_str("out of memory"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::collections::TryReserveError> for Error {
    fn from(_: std::collections::TryReserveError) -> Self {
        Error::OutOfMemory
    }
}
