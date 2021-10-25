use std::path::Path;

struct DirReader {}

struct Dir(*mut libc::DIR);

unsafe impl Send for Dir {}
unsafe impl Sync for Dir {}

impl Iterator for ReadDir {
    type Item = io::Result<DirEntry>;

    #[cfg(any(
        target_os = "solaris",
        target_os = "fuchsia",
        target_os = "redox",
        target_os = "illumos"
    ))]
    fn next(&mut self) -> Option<io::Result<DirEntry>> {
        use crate::slice;

        unsafe {
            loop {
                // Although readdir_r(3) would be a correct function to use here because
                // of the thread safety, on Illumos and Fuchsia the readdir(3C) function
                // is safe to use in threaded applications and it is generally preferred
                // over the readdir_r(3C) function.
                super::os::set_errno(0);
                let entry_ptr = libc::readdir(self.inner.dirp.0);
                if entry_ptr.is_null() {
                    // null can mean either the end is reached or an error occurred.
                    // So we had to clear errno beforehand to check for an error now.
                    return match super::os::errno() {
                        0 => None,
                        e => Some(Err(Error::from_raw_os_error(e))),
                    };
                }

                let name = (*entry_ptr).d_name.as_ptr();
                let namelen = libc::strlen(name) as usize;

                let ret = DirEntry {
                    entry: *entry_ptr,
                    name: slice::from_raw_parts(name as *const u8, namelen as usize)
                        .to_owned()
                        .into_boxed_slice(),
                    dir: Arc::clone(&self.inner),
                };
                if ret.name_bytes() != b"." && ret.name_bytes() != b".." {
                    return Some(Ok(ret));
                }
            }
        }
    }

    #[cfg(not(any(
        target_os = "solaris",
        target_os = "fuchsia",
        target_os = "redox",
        target_os = "illumos"
    )))]
    fn next(&mut self) -> Option<io::Result<DirEntry>> {
        if self.end_of_stream {
            return None;
        }

        unsafe {
            let mut ret = DirEntry {
                entry: mem::zeroed(),
                dir: Arc::clone(&self.inner),
            };
            let mut entry_ptr = ptr::null_mut();
            loop {
                let err = readdir64_r(self.inner.dirp.0, &mut ret.entry, &mut entry_ptr);
                if err != 0 {
                    if entry_ptr.is_null() {
                        // We encountered an error (which will be returned in this iteration), but
                        // we also reached the end of the directory stream. The `end_of_stream`
                        // flag is enabled to make sure that we return `None` in the next iteration
                        // (instead of looping forever)
                        self.end_of_stream = true;
                    }
                    return Some(Err(Error::from_raw_os_error(err)));
                }
                if entry_ptr.is_null() {
                    return None;
                }
                if ret.name_bytes() != b"." && ret.name_bytes() != b".." {
                    return Some(Ok(ret));
                }
            }
        }
    }
}

/// Unfortunately we can't use `std::fs::read_dir` since it internally allocates
/// on the global heap which we can't trust, so need to roll our own
pub fn read_dir(root: &impl AsRef<Path>) -> Result<DirReader, std::io::Error> {
    unsafe {
        let ptr = libc::opendir(p.as_ptr());
        if ptr.is_null() {
            Err(Error::last_os_error())
        } else {
            let inner = InnerReadDir {
                dirp: Dir(ptr),
                root,
            };
            Ok(ReadDir {
                inner: Arc::new(inner),
                #[cfg(not(any(
                    target_os = "solaris",
                    target_os = "illumos",
                    target_os = "fuchsia",
                    target_os = "redox",
                )))]
                end_of_stream: false,
            })
        }
    }
}
