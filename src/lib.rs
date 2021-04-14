#[cfg_attr(not(feature = "debug-logs"), allow(unused_variables))]
mod breakpad_integration;
mod error;

pub use breakpad_integration::{BreakpadIntegration, InstallOptions};
pub use error::Error;
