#[repr(C)]
pub struct ExceptionHandler {
    _unused: [u8; 0],
}

#[cfg(not(windows))]
pub type PathChar = u8;
#[cfg(windows)]
pub type PathChar = u16;

pub type CrashCallback = extern "C" fn(
    minidump_path: *const PathChar,
    minidump_path_len: usize,
    ctx: *mut std::ffi::c_void,
);

pub const INSTALL_NO_HANDLER: u32 = 0x0;
pub const INSTALL_EXCEPTION_HANDLER: u32 = 0x1;
pub const INSTALL_SIGNAL_HANDLER: u32 = 0x2;
pub const INSTALL_BOTH_HANDLERS: u32 = INSTALL_EXCEPTION_HANDLER | INSTALL_SIGNAL_HANDLER;

extern "C" {
    /// Creates and attaches an exception handler that will monitor this process
    /// for crashes
    ///
    /// Note: The `install_options` only applies on MacOS/iOS, it is ignored
    /// for all other platforms.
    pub fn attach_exception_handler(
        path: *const PathChar,
        path_len: usize,
        crash_callback: CrashCallback,
        crash_callback_ctx: *mut std::ffi::c_void,
        install_options: u32,
    ) -> *mut ExceptionHandler;

    /// Detaches and frees the exception handler
    pub fn detach_exception_handler(handler: *mut ExceptionHandler);
}
