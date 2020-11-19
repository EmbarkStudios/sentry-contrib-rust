mod breakpad_integration;

pub use breakpad_integration::BreakpadIntegration;

pub fn upload_minidumps(hub: &sentry_core::Hub) {
    hub.with_integration(|integration: &BreakpadIntegration| {
        integration.upload_minidumps(hub);
    });
}
