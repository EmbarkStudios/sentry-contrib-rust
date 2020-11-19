use sentry_core::{protocol::Envelope, sentry_debug};
use std::sync::Arc;

pub struct DiskTransport {
    queue_size: Arc<parking_lot::Mutex<usize>>,
    sender: crossbeam::channel::Sender<Option<Envelope>>,
    shutdown_signal: Arc<parking_lot::Condvar>,
}

impl DiskTransport {
    pub fn new<P: Into<std::path::PathBuf>>(
        path: P,
        outer_transport: Option<Arc<dyn sentry_core::Transport>>,
    ) -> Self {
        let queue_size = Arc::new(parking_lot::Mutex::new(0));
        let (tx, rx) = crossbeam::channel::bounded(10);
        let shutdown_signal = Arc::new(parking_lot::Condvar::new());

        let qs = queue_size.clone();
        let ss = shutdown_signal.clone();
        let dir = path.into();

        let handle = std::thread::spawn(move || {
            while let Some(envelope) = rx.try_recv().unwrap_or(None) {
                // 
                envelope.items().

                let mut size = qs.lock();
                *size -= 1;
                if *size == 0 {
                    ss.notify_all();
                }
            }

            // Shutdown the outer transport as well
            if let Some(outer) = outer_transport {
                outer.shutdown(timeout)
            }
        });

        Self {
            queue_size,
            sender: tx,
            shutdown_signal,
        }
    }
}

impl sentry_core::Transport for DiskTransport {
    fn send_envelope(&self, envelope: Envelope) {
        if let Err(e) = self.sender.send(Some(envelope)) {
            sentry_debug!("disk transport write thread has been shutdown");
        }
    }

    fn shutdown(&self, timeout: std::time::Duration) -> bool {
        if *self.queue_size.lock() == 0 {
            true
        } else {
            // Signal the write thread to flush and shutdown
            if self.sender.send_timeout(None, timeout).is_err() {
                return false;
            }

            let guard = self.queue_size.lock();
            if *guard > 0 {
                self.shutdown_signal.wait_until(guard, timeout).is_ok()
            } else {
                true
            }
        }
    }
}
