use std::fmt;

#[derive(Debug)]
pub enum Error {
    HandlerAlreadyRegistered,
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HandlerAlreadyRegistered => {
                f.write_str("Unable to register crash handler, only one is allowed at a time")
            }
        }
    }
}
