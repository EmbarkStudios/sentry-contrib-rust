use sentry_core::{protocol as proto, types};
use std::path::Path;

pub(crate) fn assemble_envelope(md: CrashMetadata, minidump_path: &Path) -> proto::Envelope {
    let mut envelope = proto::Envelope::new();

    let timestamp = md
        .event
        .as_ref()
        .map(|eve| eve.timestamp)
        .or_else(|| {
            minidump_path
                .metadata()
                .ok()
                .and_then(|md| md.created().ok().map(|st| st.into()))
        })
        .unwrap_or_else(types::Utc::now);

    // An event_id is required, so if we were unable to get one from the .metadata
    // we just use the guid in the filename of the minidump
    envelope.add_item(md.event.unwrap_or_else(|| {
        proto::Event {
            event_id: minidump_path
                .file_stem()
                .and_then(|fname| fname.to_str().and_then(|fs| fs.parse::<types::Uuid>().ok()))
                .unwrap_or_else(types::Uuid::new_v4),
            level: proto::Level::Fatal,
            timestamp,
            ..Default::default()
        }
    }));

    // Unfortunately we can't really synthesize this with the current API as,
    // among other things, the session id is not exposed anywhere :-/
    if let Some(su) = md.session_update {
        envelope.add_item(su);
    }

    match std::fs::read(minidump_path) {
        Err(e) => {
            debug_print!(
                "unable to read minidump from '{}': {}",
                minidump_path.display(),
                e
            );
        }
        Ok(minidump) => {
            envelope.add_item(proto::EnvelopeItem::Attachment(proto::Attachment {
                buffer: minidump,
                filename: minidump_path.file_name().unwrap().to_string_lossy().into(),
                ty: Some(proto::AttachmentType::Minidump),
            }));
        }
    }

    envelope
}

pub(crate) struct CrashMetadata {
    pub(crate) event: Option<proto::Event<'static>>,
    pub(crate) session_update: Option<proto::SessionUpdate<'static>>,
}

impl CrashMetadata {
    pub(crate) fn deserialize(path: &Path) -> Self {
        if !path.exists() {
            return Self {
                event: None,
                session_update: None,
            };
        }

        let contents = match std::fs::read_to_string(path) {
            Ok(contents) => {
                // Immediately remove the file so we don't try to do this again
                let _ = std::fs::remove_file(path);
                contents
            }
            Err(e) => {
                debug_print!(
                    "unable to read crash metadata from '{}': {}",
                    path.display(),
                    e
                );
                return Self {
                    event: None,
                    session_update: None,
                };
            }
        };

        let mut lines = contents.lines();

        let event = lines.next().and_then(|eve| {
            if !eve.is_empty() {
                match serde_json::from_str::<proto::Event>(eve) {
                    Ok(event) => Some(event),
                    Err(e) => {
                        debug_print!("unable to deserialize Event: {}", e);
                        None
                    }
                }
            } else {
                None
            }
        });

        let session_update = lines.next().and_then(|su| {
            if !su.is_empty() {
                match serde_json::from_str::<proto::SessionUpdate>(su) {
                    Ok(sess) => Some(sess),
                    Err(e) => {
                        debug_print!("unable to deserialize SessionUpdate: {}", e);
                        None
                    }
                }
            } else {
                None
            }
        });

        Self {
            event,
            session_update,
        }
    }

    pub(crate) fn serialize(self) -> Vec<u8> {
        use std::io::Write;

        let mut md = Vec::with_capacity(2048);

        // Serialize the envelope then the session update to their own JSON line
        if let Some(eve) = self.event {
            debug_print!("serializing event to metadata");
            if let Err(e) = serde_json::to_writer(&mut md, &eve) {
                debug_print!("failed to serialize event to crash metadata: {}", e);
            }
        }

        let _ = writeln!(&mut md);

        if let Some(su) = self.session_update {
            debug_print!("serializing session update to metadata");
            if let Err(e) = serde_json::to_writer(&mut md, &su) {
                debug_print!(
                    "failed to serialize session update to crash metadata: {}",
                    e
                );
            }
        }

        let _ = writeln!(&mut md);
        md
    }
}
