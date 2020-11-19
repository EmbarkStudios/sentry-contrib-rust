use sentry_core::{
    protocol::{Envelope, EnvelopeItem, Event, Level, SessionUpdate},
    ClientOptions,
};

fn read_metadata_to_envelope(path: &std::path::Path, envelope: &mut Envelope) {
    if !path.exists() {
        return;
    }

    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => {
            // Immediately remove the file we don't try to do this again
            let _ = std::fs::remove_file(path);
            contents
        }
        Err(e) => {
            // sentry_debug!(
            //     "unable to read crash metadata from '{}': {}",
            //     path.display(),
            //     e
            // );
            return;
        }
    };

    let mut lines = contents.lines();

    if let Some(eve) = lines.next() {
        if !eve.is_empty() {
            match serde_json::from_str::<Event>(eve) {
                Ok(event) => {
                    envelope.add_item(EnvelopeItem::Event(event));
                }
                Err(e) => {
                    //sentry_debug!("unable to deserialize Event: {}", e);
                }
            };
        }
    }

    if let Some(su) = lines.next() {
        if !su.is_empty() {
            match serde_json::from_str::<SessionUpdate>(su) {
                Ok(sess) => {
                    envelope.add_item(EnvelopeItem::SessionUpdate(sess));
                }
                Err(e) => {
                    //sentry_debug!("unable to deserialize SessionUpdate: {}", e);
                }
            };
        }
    }
}

/// Integrates Breakpad crash handling and reporting
pub struct BreakpadIntegration {
    crash_handler: breakpad_handler::BreakpadHandler,
    crash_dir: std::path::PathBuf,
}

impl sentry_core::Integration for BreakpadIntegration {
    fn name(&self) -> &'static str {
        "breakpad"
    }

    fn setup(&self, _cfg: &mut ClientOptions) {}
}

impl BreakpadIntegration {
    /// Creates a new Breakpad Integration, note that only *one* can exist
    /// in the application at a time!
    pub fn new<P: AsRef<std::path::Path>>(crash_dir: P) -> Result<Self, breakpad_handler::Error> {
        use std::io::Write;

        // Ensure the directory exists, breakpad should do this when writing crashdumps
        // anyway, but then again, it's C++ code so I have low trust
        std::fs::create_dir_all(crash_dir)?;

        let crash_handler = breakpad_handler::BreakpadHandler::attach(
            &crash_dir,
            Box::new(|minidump_path: std::path::PathBuf| {
                // Create an event for crash so that we can add all of the context
                // we can to it, the important information like stack traces/threads
                // modules/etc is contained in the minidump recorded by breakpad
                let event = Event {
                    level: Level::Fatal,
                    // We want to set the timestep here since we aren't actually
                    // going to send the crash directly, but rather the next time
                    // this integration is initialized
                    timestamp: sentry_core::types::Utc::now(),
                    ..Default::default()
                };

                let mut eve = None;
                let mut sess_update = None;

                // Now fill out the event and (maybe) get the session status so
                // that we can serialize them to disk so that we can add them
                // into the same envelope as the actual minidump
                sentry_core::Hub::with_active(|hub| {
                    hub.with_current_scope(|scope| {
                        (eve, sess_update) = hub.client().assemble_event(event, Some(scope));
                    });
                });

                let mut meta_data = Vec::with_capacity(2048);

                // Serialize the envelope then the session update to their own JSON line
                if let Some(eve) = eve {
                    if let Err(e) = serde_json::to_writer(&mut meta_data, &eve) {
                        //sentry_debug!("failed to serialize event to crash metadata: {}", e);
                    }
                }

                let _ = writeln!(&mut meta_data);

                if let Some(su) = sess_update {
                    if let Err(e) = serde_json::to_writer(&mut meta_data, &su) {
                        // sentry_debug!(
                        //     "failed to serialize session update to crash metadata: {}",
                        //     e
                        // );
                    }
                }

                let _ = writeln!(&mut meta_data);
                minidump_path.set_extension("metadata");

                if let Err(e) = std::fs::write(&minidump_path, &meta_data) {
                    // sentry_debug!(
                    //     "failed to write sentry crash metadata to '{}': {}",
                    //     minidump_path.display(),
                    //     e
                    // );
                }
            }),
        )?;

        let crash_dir = crash_dir.as_ref().to_owned();

        Ok(Self {
            crash_dir,
            crash_handler,
        })
    }

    /// Run this once you have initialized Sentry to upload any minidumps + metadata
    /// that may exist from an earlier run
    pub fn upload_minidumps(&self, hub: &sentry_core::Hub) {
        // Scan the directory the integration was initialized with to find any
        // envelopes that have been serialized to disk and send + delete them
        let rd = match std::fs::read_dir(&self.crash_dir) {
            Ok(rd) => rd,
            Err(e) => {
                // sentry_debug!(
                //     "Unable to read crash directory '{}': {}",
                //     self.crash_dir.display(),
                //     e
                // );
                return;
            }
        };

        let client = hub.client();

        // The minidumps are what we care about the most, but of course, the
        // metadata that we (hopefully) were able to capture along with the crash
        for entry in rd.filter_map(|e| e.ok()) {
            if entry
                .file_name()
                .to_str()
                .map_or(true, |s| !s.ends_with(".dmp"))
            {
                continue;
            }

            let minidump_path = entry.path();
            let mut envelope = Envelope::new();

            match std::fs::read(&minidump_path) {
                Err(e) => {
                    // sentry_debug!(
                    //     "unable to read minidump from '{}': {}",
                    //     minidump_path.display(),
                    //     e
                    // );

                    let _ = std::fs::remove_file(&minidump_path);

                    minidump_path.set_extension("metadata");
                    if minidump_path.exists() {
                        let _ = std::fs::remove_file(&minidump_path);
                    }

                    continue;
                }
                Ok(minidump) => {}
            }

            minidump_path.set_extension("metadata");

            // We might be able to attach metadata to the event, but it's optional
            read_metadata_to_envelope(&minidump_path, &mut envelope);

            client.send_envelope(envelope);
        }
    }
}
