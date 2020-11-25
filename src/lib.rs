#[cfg_attr(not(feature = "debug-logs"), allow(unused_variables))]
mod breakpad_integration;
mod error;

pub use breakpad_integration::BreakpadIntegration;
pub use error::Error;
