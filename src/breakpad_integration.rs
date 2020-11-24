use sentry_core::protocol;

fn read_metadata_to_envelope(path: &std::path::Path, envelope: &mut protocol::Envelope) {
    if !path.exists() {
        return;
    }

    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => {
            // Immediately remove the file so we don't try to do this again
            //let _ = std::fs::remove_file(path);
            contents
        }
        Err(e) => {
            eprintln!(
                "unable to read crash metadata from '{}': {}",
                path.display(),
                e
            );
            return;
        }
    };

    let mut lines = contents.lines();

    if let Some(eve) = lines.next() {
        if !eve.is_empty() {
            match serde_json::from_str::<protocol::Event>(eve) {
                Ok(event) => {
                    envelope.add_item(protocol::EnvelopeItem::Event(event));
                }
                Err(e) => {
                    eprintln!("unable to deserialize Event: {}", e);
                }
            };
        }
    }

    if let Some(su) = lines.next() {
        if !su.is_empty() {
            match serde_json::from_str::<protocol::SessionUpdate>(su) {
                Ok(sess) => {
                    envelope.add_item(protocol::EnvelopeItem::SessionUpdate(sess));
                }
                Err(e) => {
                    eprintln!("unable to deserialize SessionUpdate: {}", e);
                }
            };
        }
    }
}

/// Integrates Breakpad crash handling and reporting
pub struct BreakpadIntegration {
    crash_handler: Option<breakpad_handler::BreakpadHandler>,
    crash_dir: std::path::PathBuf,
    hub: std::sync::Arc<sentry_core::Hub>,
}

impl BreakpadIntegration {
    /// Creates a new Breakpad Integration, note that only *one* can exist
    /// in the application at a time!
    pub fn new<P: AsRef<std::path::Path>>(
        crash_dir: P,
        hub: std::sync::Arc<sentry_core::Hub>,
    ) -> Result<Self, crate::Error> {
        use std::io::Write;

        // Ensure the directory exists, breakpad should do this when writing crashdumps
        // anyway, but then again, it's C++ code, so I have low trust
        std::fs::create_dir_all(&crash_dir)?;

        let crash_hub = hub.clone();
        let crash_handler = breakpad_handler::BreakpadHandler::attach(
            &crash_dir,
            Box::new(move |mut minidump_path: std::path::PathBuf| {
                // Create an event for crash so that we can add all of the context
                // we can to it, the important information like stack traces/threads
                // modules/etc is contained in the minidump recorded by breakpad
                let event = protocol::Event {
                    level: protocol::Level::Fatal,
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
                {
                    if let Some(client) = crash_hub.client() {
                        eprintln!("FILLING EVENT!");
                        crash_hub.configure_scope(|scope| {
                            let assembled = client.assemble_event(event, Some(scope));
                            eve = assembled.0;
                            sess_update = assembled.1;
                        });
                    }
                }

                let mut meta_data = Vec::with_capacity(2048);

                // Serialize the envelope then the session update to their own JSON line
                if let Some(eve) = eve {
                    if let Err(e) = serde_json::to_writer(&mut meta_data, &eve) {
                        eprintln!("failed to serialize event to crash metadata: {}", e);
                    }
                }

                let _ = writeln!(&mut meta_data);

                if let Some(su) = sess_update {
                    if let Err(e) = serde_json::to_writer(&mut meta_data, &su) {
                        eprintln!(
                            "failed to serialize session update to crash metadata: {}",
                            e
                        );
                    }
                }

                let _ = writeln!(&mut meta_data);
                minidump_path.set_extension("metadata");

                if let Err(e) = std::fs::write(&minidump_path, &meta_data) {
                    eprintln!(
                        "failed to write sentry crash metadata to '{}': {}",
                        minidump_path.display(),
                        e
                    );
                }

                eprintln!(
                    "wrote {} of metadata for crash to {}",
                    meta_data.len(),
                    minidump_path.display()
                );
            }),
        )?;

        let crash_dir = crash_dir.as_ref().to_owned();

        Ok(Self {
            crash_dir,
            crash_handler: Some(crash_handler),
            hub,
        })
    }

    /// Run this once you have initialized Sentry to upload any minidumps + metadata
    /// that may exist from an earlier run
    pub fn upload_minidumps(&self) {
        // Scan the directory the integration was initialized with to find any
        // envelopes that have been serialized to disk and send + delete them
        let rd = match std::fs::read_dir(&self.crash_dir) {
            Ok(rd) => rd,
            Err(e) => {
                eprintln!(
                    "Unable to read crash directory '{}': {}",
                    self.crash_dir.display(),
                    e
                );
                return;
            }
        };

        let client = self.hub.client();

        let client = match client {
            Some(c) => c,
            None => return,
        };

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

            let mut minidump_path = entry.path();
            let mut envelope = protocol::Envelope::new();

            let minidump_contents = match std::fs::read(&minidump_path) {
                Err(e) => {
                    eprintln!(
                        "unable to read minidump from '{}': {}",
                        minidump_path.display(),
                        e
                    );

                    let _ = std::fs::remove_file(&minidump_path);

                    minidump_path.set_extension("metadata");
                    if minidump_path.exists() {
                        let _ = std::fs::remove_file(&minidump_path);
                    }

                    continue;
                }
                Ok(minidump) => {
                    // Remove the minidump so we don't process it again
                    let _ = std::fs::remove_file(&minidump_path);
                    eprintln!("LOADED MINIDUMP FROM {}", minidump_path.display());
                    minidump
                }
            };

            envelope.add_item(protocol::EnvelopeItem::Attachment(protocol::Attachment {
                buffer: std::sync::Arc::new(minidump_contents),
                filename: minidump_path.file_name().unwrap().to_owned(),
                ty: Some(protocol::AttachmentType::Minidump),
            }));

            minidump_path.set_extension("metadata");

            // We might be able to attach metadata to the event, but it's optional
            read_metadata_to_envelope(&minidump_path, &mut envelope);

            // An event_id is required, so if we were unable to get one from the .metadata
            // we just use the guid in the filename of the minidump
            if envelope.uuid().is_none() {
                let event = protocol::Event {
                    event_id: minidump_path
                        .file_stem()
                        .and_then(|fname| {
                            fname
                                .to_str()
                                .and_then(|fs| fs.parse::<sentry_core::types::Uuid>().ok())
                        })
                        .unwrap_or_else(|| sentry_core::types::Uuid::new_v4()),
                    level: protocol::Level::Fatal,
                    timestamp: sentry_core::types::Utc::now(),
                    ..Default::default()
                };

                envelope.add_item(event);
            }

            client.send_envelope(envelope);
        }
    }
}

impl Drop for BreakpadIntegration {
    fn drop(&mut self) {
        let _ = self.crash_handler.take();
    }
}
