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
    #[cfg(target_os = "macos")]
    pause_ctx: Box<atomic::AtomicBool>,
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

        unsafe {
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

            extern "C" fn crash_callback(
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

            #[cfg(target_os = "macos")]
            {
                let pause_ctx = Box::new(atomic::AtomicBool::new(false));

                extern "C" fn pause_callback(ctx: *mut std::ffi::c_void) -> bool {
                    let is_paused = unsafe { &*(ctx as *const atomic::AtomicBool) };

                    is_paused.load(atomic::Ordering::Relaxed)
                }

                let handler = breakpad_sys::attach_exception_handler(
                    path.as_ptr(),
                    path.len(),
                    crash_callback,
                    on_crash,
                    Some(pause_callback),
                    &*pause_ctx as *const atomic::AtomicBool as *mut _,
                );

                Ok(Self {
                    handler,
                    on_crash,
                    pause_ctx,
                })
            }

            #[cfg(not(target_os = "macos"))]
            {
                let handler = breakpad_sys::attach_exception_handler(
                    path.as_ptr(),
                    path.len(),
                    crash_callback,
                    on_crash,
                    None,
                    std::ptr::null_mut(),
                );

                Ok(Self { handler, on_crash })
            }
        }
    }

    /// Pauses Breakpad's exception handler, temporarily pretending as if Breakpad
    /// is not attached. This is exposed only when targetting on mac's, as this is
    /// (usually) only needed when an application intentionally wants to handle
    /// regular signals, but can't since breakpad's exception handler always
    /// takes precedence.
    #[cfg(target_os = "macos")]
    pub fn pause(&self) {
        self.pause_ctx.store(true, atomic::Ordering::Relaxed);
    }

    #[cfg(target_os = "macos")]
    pub fn unpause(&self) {
        self.pause_ctx.store(false, atomic::Ordering::Relaxed);
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
