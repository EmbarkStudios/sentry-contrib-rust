use sentry_backtrace::current_stacktrace;
use sentry_core::{
    protocol::{Event, Exception, Level, Mechanism},
    ClientOptions, Integration,
};

/// Integrates Breakpad crash handling and reporting
pub struct BreakpadIntegration {
    crash_handler: breakpad_handler::BreakpadHandler,
    crash_dir: std::path::PathBuf,
}

impl Integration for BreakpadIntegration {
    fn name(&self) -> &'static str {
        "breakpad"
    }

    fn setup(&self, _cfg: &mut ClientOptions) {
        // Scan the directory the integration was initialized with to find any
        // envelopes that have been serialized to disk and send, then delete, them
    }
}

impl BreakpadIntegration {
    /// Creates a new Breakpad Integration, note that only *one* can exist
    /// in the application at a time!
    pub fn new<P: AsRef<std::path::Path>>(crash_dir: P) -> Result<Self, breakpad_handler::Error> {
        let crash_handler = breakpad_handler::BreakpadHandler::attach(
            &crash_dir,
            Box::new(|minidump_path: std::path::PathBuf| {}),
        )?;

        let crash_dir = crash_dir.as_ref().to_owned();

        Ok(Self {
            crash_dir,
            crash_handler,
        })
    }

    /// Creates an event from the given panic info.
    ///
    /// The stacktrace is calculated from the current frame.
    pub fn event_from_panic_info(&self, info: &PanicInfo<'_>) -> Event<'static> {
        for extractor in &self.extractors {
            if let Some(event) = extractor(info) {
                return event;
            }
        }

        // TODO: We would ideally want to downcast to `std::error:Error` here
        // and use `event_from_error`, but that way we won‘t get meaningful
        // backtraces yet.

        let msg = message_from_panic_info(info);
        Event {
            exception: vec![Exception {
                ty: "panic".into(),
                mechanism: Some(Mechanism {
                    ty: "panic".into(),
                    handled: Some(false),
                    ..Default::default()
                }),
                value: Some(msg.to_string()),
                stacktrace: current_stacktrace(),
                ..Default::default()
            }]
            .into(),
            level: Level::Fatal,
            ..Default::default()
        }
    }
}
