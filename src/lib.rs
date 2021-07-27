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
pub use transport::{BreakpadTransport, BreakpadTransportFactory, CrashSendStyle};
