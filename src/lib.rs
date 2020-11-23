mod breakpad_integration;
mod error;

pub use breakpad_integration::BreakpadIntegration;
pub use error::Error;

pub fn upload_minidumps(hub: &sentry_core::Hub) {
    hub.with_integration(|integration: &BreakpadIntegration| {
        integration.upload_minidumps(hub);
    });
}
