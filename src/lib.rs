//! Capture and report minidumps to [Sentry](https://sentry.io/about/)
//!
//! 1. Create a [`BreakpadTransportFactory`] for [`ClientOptions::transport`](https://docs.rs/sentry-core/0.23.0/sentry_core/struct.ClientOptions.html#structfield.transport)
//! , providing it with the [`TransportFactory`](https://docs.rs/sentry-core/0.23.0/sentry_core/trait.TransportFactory.html)
//! you were previously using.
//! 2. Initialize a Sentry [`Hub`](https://docs.rs/sentry-core/0.23.0/sentry_core/struct.Hub.html).
//! 3. Create the [`BreakpadIntegration`] which will attach a crash handler and
//! send any previous crashes that are in the crash directoy specified.

macro_rules! debug_print {
    ($($arg:tt)*) => {
        #[cfg(feature = "debug-logs")]
        {
            eprintln!("[bp] {}", format_args!($($arg)*));
        }
        #[cfg(not(feature = "debug-logs"))]
        {
            let _ = format_args!($($arg)*);
        }
    }
}

mod breakpad_integration;
mod error;
mod shared;
mod transport;

pub use breakpad_integration::{BreakpadIntegration, InstallOptions};
pub use error::Error;
pub use transport::{BreakpadTransportFactory, CrashSendStyle};
