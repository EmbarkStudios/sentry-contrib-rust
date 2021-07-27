use std::fmt;

#[derive(Debug)]
pub enum Error {
    Handler(breakpad_handler::Error),
    Io(std::io::Error),
    /// Paths in some cases are required to be utf-8 compatible
    NonUtf8Path(std::path::PathBuf),
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Handler(e) => Some(e),
            Self::Io(e) => Some(e),
            Self::NonUtf8Path(_) => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Handler(e) => write!(f, "handler error: {}", e),
            Self::Io(e) => write!(f, "io error: {}", e),
            Self::NonUtf8Path(p) => write!(f, "{} is not a utf-8 path", p.display()),
        }
    }
}

impl From<breakpad_handler::Error> for Error {
    fn from(e: breakpad_handler::Error) -> Self {
        Self::Handler(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
