use sentry_core::{ClientOptions, Envelope, Transport, TransportFactory};
use std::{sync::Arc, time::Duration};

/// Determines how crashes are sent to Sentry after they have been captured.
#[derive(Copy, Clone)]
pub enum CrashSendStyle {
    /// Attempts to send crash envelopes immediately, in the same session that
    /// crashed, which may be unreliable depending on the overall state of the
    /// session. Use with care.
    SendImmediately,
    /// Serializes the envelope to disk instead of forwarding it to the final
    /// [`Transport`], initializing the BreakpadIntegration with the same path
    /// for crashes will send any existing crashes from previous sessions.
    SendNextSession,
}

/// The [`TransportFactory`](https://docs.rs/sentry-core/0.23.0/sentry_core/trait.TransportFactory.html) implementation that must be used in concert with
/// [`BreakpadIntegration`](crate::BreakpadIntegration) to report crash events to
/// Sentry
pub struct BreakpadTransportFactory {
    inner: Arc<dyn TransportFactory>,
    style: CrashSendStyle,
}

impl BreakpadTransportFactory {
    pub fn new(style: CrashSendStyle, transport: Arc<dyn TransportFactory>) -> Self {
        Self {
            style,
            inner: transport,
        }
    }
}

impl TransportFactory for BreakpadTransportFactory {
    fn create_transport(&self, options: &ClientOptions) -> Arc<dyn Transport> {
        Arc::new(BreakpadTransport {
            inner: self.inner.create_transport(options),
            style: self.style,
        })
    }
}

struct BreakpadTransport {
    inner: Arc<dyn Transport>,
    style: CrashSendStyle,
}

impl BreakpadTransport {
    fn process(&self, envelope: Envelope) -> Option<Envelope> {
        use sentry_core::protocol as proto;

        match envelope.event() {
            // Check if this is actually a crash event
            Some(eve) if !eve.extra.contains_key("__breakpad_minidump_path") => Some(envelope),
            None => Some(envelope),
            Some(eve) => {
                let mut event = eve.clone();

                // Clear the exceptions array, Sentry will automatically fill this
                // in for the event due to it having a minidump attachment
                event.exception.values.clear();

                let mut minidump_path = match event.extra.remove("__breakpad_minidump_path") {
                    Some(sentry_core::protocol::Value::String(s)) => std::path::PathBuf::from(s),
                    other => unreachable!(
                        "__breakpad_minidump_path should be a String, but was {:?}",
                        other
                    ),
                };

                let session_update = envelope.items().find_map(|ei| match ei {
                    proto::EnvelopeItem::SessionUpdate(su) => {
                        let mut su = su.clone();
                        su.status = proto::SessionStatus::Crashed;

                        Some(su)
                    }
                    _ => None,
                });

                let md = crate::shared::CrashMetadata {
                    event: Some(event),
                    session_update,
                };

                match self.style {
                    CrashSendStyle::SendImmediately => {
                        let envelope = crate::shared::assemble_envelope(md, &minidump_path);

                        if let Err(e) = std::fs::remove_file(&minidump_path) {
                            debug_print!(
                                "failed to remove crashdump {}: {}",
                                minidump_path.display(),
                                e
                            );
                        }

                        Some(envelope)
                    }
                    CrashSendStyle::SendNextSession => {
                        let serialized = md.serialize();

                        minidump_path.set_extension("metadata");
                        if let Err(e) = std::fs::write(&minidump_path, serialized) {
                            debug_print!(
                                "failed to write crash metadata {}: {}",
                                minidump_path.display(),
                                e
                            );
                        }

                        None
                    }
                }
            }
        }
    }
}

impl Transport for BreakpadTransport {
    fn send_envelope(&self, envelope: Envelope) {
        if let Some(envelope) = self.process(envelope) {
            self.inner.send_envelope(envelope);
        }
    }

    fn flush(&self, timeout: Duration) -> bool {
        self.inner.flush(timeout)
    }

    fn shutdown(&self, timeout: Duration) -> bool {
        self.inner.shutdown(timeout)
    }
}
