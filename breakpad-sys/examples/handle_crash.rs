fn main() {
    let cur_dir = std::env::current_dir().unwrap();
    let os_str = cur_dir.as_os_str();

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

    unsafe {
        extern "C" fn callback(
            path: *const breakpad_sys::PathChar,
            path_len: usize,
            _ctx: *mut std::ffi::c_void,
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

            println!("Crashdump written to {}", path.display());
            match std::fs::remove_file(&path) {
                Ok(_) => {
                    println!("Removed {}", path.display());
                }
                Err(e) => {
                    println!("Failed to remove {}: {}", path.display(), e);
                }
            }
        }

        let exc_handler = breakpad_sys::attach_exception_handler(
            path.as_ptr(),
            path.len(),
            callback,
            std::ptr::null_mut(),
        );

        if std::env::args().any(|a| a == "--crash") {
            let ptr: *mut u8 = std::ptr::null_mut();
            *ptr = 42;
        }

        breakpad_sys::detach_exception_handler(exc_handler);
    }
}
