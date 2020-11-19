mod error;
pub use error::Error;

use std::sync::atomic;

pub trait CrashEvent: Sync + Send {
    fn on_crash(&self, minidump_path: std::path::PathBuf);
}

impl<F> CrashEvent for F
where
    F: Fn(std::path::PathBuf) + Send + Sync,
{
    fn on_crash(&self, minidump_path: std::path::PathBuf) {
        self(minidump_path)
    }
}

static HANDLER_ATTACHED: atomic::AtomicBool = atomic::AtomicBool::new(false);

pub struct BreakpadHandler {
    handler: *mut breakpad_sys::ExceptionHandler,
    on_crash: *mut std::ffi::c_void,
}

unsafe impl Send for BreakpadHandler {}
unsafe impl Sync for BreakpadHandler {}

impl BreakpadHandler {
    /// Sets up a breakpad handler to catch exceptions/signals, writing out
    /// a minidump to the designated directory if a crash occurs. Only one
    /// handler can be attached at a time
    pub fn attach<P: AsRef<std::path::Path>>(
        crash_dir: P,
        on_crash: Box<dyn CrashEvent>,
    ) -> Result<Self, Error> {
        if HANDLER_ATTACHED.compare_and_swap(false, true, atomic::Ordering::Relaxed) {
            return Err(Error::HandlerAlreadyRegistered);
        }

        let on_crash = Box::into_raw(Box::new(on_crash)) as *mut _;

        let handler = unsafe {
            let os_str = crash_dir.as_ref().as_os_str();

            let path: Vec<breakpad_sys::PathChar> = {
                #[cfg(windows)]
                {
                    use std::os::windows::ffi::OsStrExt;
                    os_str.encode_wide().collect()
                }
                #[cfg(unix)]
                {
                    use std::os::unix::ffi::OsStrExt;
                    Vec::from(os_str.as_bytes())
                }
            };

            extern "C" fn callback(
                path: *const breakpad_sys::PathChar,
                path_len: usize,
                ctx: *mut std::ffi::c_void,
            ) {
                let path_slice = unsafe { std::slice::from_raw_parts(path, path_len) };

                let path = {
                    #[cfg(windows)]
                    {
                        use std::os::windows::ffi::OsStringExt;
                        std::path::PathBuf::from(std::ffi::OsString::from_wide(path_slice))
                    }
                    #[cfg(unix)]
                    {
                        use std::os::unix::ffi::OsStrExt;
                        std::path::PathBuf::from(std::ffi::OsStr::from_bytes(path_slice).to_owned())
                    }
                };

                let context: Box<Box<dyn CrashEvent>> = unsafe { Box::from_raw(ctx as *mut _) };
                context.on_crash(path);
                Box::leak(context);
            }

            breakpad_sys::attach_exception_handler(path.as_ptr(), path.len(), callback, on_crash)
        };

        Ok(Self { handler, on_crash })
    }
}

impl Drop for BreakpadHandler {
    fn drop(&mut self) {
        unsafe {
            breakpad_sys::detach_exception_handler(self.handler);
            let _: Box<Box<dyn CrashEvent>> = Box::from_raw(self.on_crash as *mut _);
            HANDLER_ATTACHED.swap(false, atomic::Ordering::Relaxed);
        }
    }
}
