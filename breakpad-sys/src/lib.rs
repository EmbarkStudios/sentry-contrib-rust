#[repr(C)]
pub struct ExceptionHandler {
    _unused: [u8; 0],
}

#[cfg(not(windows))]
pub type PathChar = u8;
#[cfg(windows)]
pub type PathChar = u16;

extern "C" {
    /// Creates and attaches an exception handler that will monitor this process
    /// for crashes
    pub fn attach_exception_handler(
        path: *const PathChar,
        path_len: usize,
        callback: extern "C" fn(
            minidump_path: *const PathChar,
            minidump_path_len: usize,
            ctx: *mut std::ffi::c_void,
        ),
        callback_ctx: *mut std::ffi::c_void,
    ) -> *mut ExceptionHandler;

    /// Detaches and frees the exception handler
    pub fn detach_exception_handler(handler: *mut ExceptionHandler);
}
