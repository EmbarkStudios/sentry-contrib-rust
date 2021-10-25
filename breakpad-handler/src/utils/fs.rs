#![cfg(unix)]

use std::{fs, io};

#[derive(Clone, Debug)]
pub struct OpenOptions {
    // generic
    read: bool,
    write: bool,
    append: bool,
    truncate: bool,
    create: bool,
    create_new: bool,
    // system-specific
    custom_flags: i32,
    mode: libc::mode_t,
}

impl OpenOptions {
    pub fn new() -> OpenOptions {
        OpenOptions {
            // generic
            read: false,
            write: false,
            append: false,
            truncate: false,
            create: false,
            create_new: false,
            // system-specific
            custom_flags: 0,
            mode: 0o666,
        }
    }

    #[inline]
    pub fn read(&mut self, read: bool) {
        self.read = read;
    }

    #[inline]
    pub fn write(&mut self, write: bool) {
        self.write = write;
    }

    #[inline]
    pub fn append(&mut self, append: bool) {
        self.append = append;
    }

    #[inline]
    pub fn truncate(&mut self, truncate: bool) {
        self.truncate = truncate;
    }

    #[inline]
    pub fn create(&mut self, create: bool) {
        self.create = create;
    }

    #[inline]
    pub fn create_new(&mut self, create_new: bool) {
        self.create_new = create_new;
    }

    #[inline]
    pub fn custom_flags(&mut self, flags: i32) {
        self.custom_flags = flags;
    }

    #[inline]
    pub fn mode(&mut self, mode: u32) {
        self.mode = mode as libc::mode_t;
    }

    fn get_access_mode(&self) -> io::Result<libc::c_int> {
        match (self.read, self.write, self.append) {
            (true, false, false) => Ok(libc::O_RDONLY),
            (false, true, false) => Ok(libc::O_WRONLY),
            (true, true, false) => Ok(libc::O_RDWR),
            (false, _, true) => Ok(libc::O_WRONLY | libc::O_APPEND),
            (true, _, true) => Ok(libc::O_RDWR | libc::O_APPEND),
            (false, false, false) => Err(io::Error::from_raw_os_error(libc::EINVAL)),
        }
    }

    fn get_creation_mode(&self) -> io::Result<libc::c_int> {
        match (self.write, self.append) {
            (true, false) => {}
            (false, false) => {
                if self.truncate || self.create || self.create_new {
                    return Err(io::Error::from_raw_os_error(libc::EINVAL));
                }
            }
            (_, true) => {
                if self.truncate && !self.create_new {
                    return Err(io::Error::from_raw_os_error(libc::EINVAL));
                }
            }
        }

        Ok(match (self.create, self.truncate, self.create_new) {
            (false, false, false) => 0,
            (true, false, false) => libc::O_CREAT,
            (false, true, false) => libc::O_TRUNC,
            (true, true, false) => libc::O_CREAT | libc::O_TRUNC,
            (_, _, true) => libc::O_CREAT | libc::O_EXCL,
        })
    }
}

/// Unfortunately we can't use [`File::open`](std::fs::File::open) directly as
/// it heap allocates the pathbuffer before doing the syscall
pub fn open(path: &impl AsRef<std::ffi::CStr>, opts: OpenOptions) -> io::Result<fs::File> {
    let flags = libc::O_CLOEXEC
        | opts.get_access_mode()?
        | opts.get_creation_mode()?
        | (opts.custom_flags as libc::c_int & !libc::O_ACCMODE);
    // The third argument of `open` is documented to have type `mode_t`. On
    // some platforms (like macOS, where `open64` is actually `open`),
    // `mode_t` is `u16`. However, since this is a variadic function, C
    // integer promotion rules mean that on the ABI level, this still gets
    // passed as `c_int` (aka `u32` on Unix platforms).
    Ok(unsafe {
        let fd = libc::open(path.as_ref().as_ptr(), flags, opts.mode as libc::c_int);

        if fd == -1 {
            return Err(io::Error::last_os_error());
        }

        use std::os::unix::io::FromRawFd;
        fs::File::from_raw_fd(fd)
    })
}

struct Dir(*mut libc::DIR);

unsafe impl Send for Dir {}
unsafe impl Sync for Dir {}

impl Drop for Dir {
    fn drop(&mut self) {
        let r = unsafe { libc::closedir(self.0) };
        debug_assert_eq!(r, 0);
    }
}

pub struct DirEntry {
    #[cfg(target_os = "android")]
    entry: libc::dirent,
    #[cfg(not(target_os = "android"))]
    entry: libc::dirent64,
    // We need to store an owned copy of the entry name
    // on Solaris and Fuchsia because a) it uses a zero-length
    // array to store the name, b) its lifetime between readdir
    // calls is not guaranteed.
    // #[cfg(any(
    //     target_os = "solaris",
    //     target_os = "illumos",
    //     target_os = "fuchsia",
    //     target_os = "redox"
    // ))]
    // name: CFixedStr<128>,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct FileType {
    mode: libc::mode_t,
}

impl DirEntry {
    #[cfg(any(
        target_os = "solaris",
        target_os = "illumos",
        target_os = "haiku",
        target_os = "vxworks"
    ))]
    pub fn file_type(&self) -> io::Result<FileType> {
        compile_error!("implement me");
        //lstat(&self.path()).map(|m| m.file_type())
    }

    #[cfg(not(any(
        target_os = "solaris",
        target_os = "illumos",
        target_os = "haiku",
        target_os = "vxworks"
    )))]
    pub fn file_type(&self) -> io::Result<FileType> {
        match self.entry.d_type {
            libc::DT_CHR => Ok(FileType {
                mode: libc::S_IFCHR,
            }),
            libc::DT_FIFO => Ok(FileType {
                mode: libc::S_IFIFO,
            }),
            libc::DT_LNK => Ok(FileType {
                mode: libc::S_IFLNK,
            }),
            libc::DT_REG => Ok(FileType {
                mode: libc::S_IFREG,
            }),
            libc::DT_SOCK => Ok(FileType {
                mode: libc::S_IFSOCK,
            }),
            libc::DT_DIR => Ok(FileType {
                mode: libc::S_IFDIR,
            }),
            libc::DT_BLK => Ok(FileType {
                mode: libc::S_IFBLK,
            }),
            _ => Err(io::ErrorKind::NotFound.into()),
        }
    }

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "emscripten",
        target_os = "android",
        target_os = "solaris",
        target_os = "illumos",
        target_os = "haiku",
        target_os = "l4re",
        target_os = "fuchsia",
        target_os = "redox",
        target_os = "vxworks",
        target_os = "espidf"
    ))]
    pub fn ino(&self) -> u64 {
        self.entry.d_ino as u64
    }

    #[cfg(any(
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    pub fn ino(&self) -> u64 {
        self.entry.d_fileno as u64
    }

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "freebsd",
        target_os = "dragonfly"
    ))]
    fn name_bytes(&self) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(
                self.entry.d_name.as_ptr().cast::<u8>(),
                self.entry.d_namlen as usize,
            )
        }
    }
    #[cfg(any(
        target_os = "android",
        target_os = "linux",
        target_os = "emscripten",
        target_os = "l4re",
        target_os = "haiku",
        target_os = "vxworks",
        target_os = "espidf"
    ))]
    fn name_bytes(&self) -> &[u8] {
        unsafe { std::ffi::CStr::from_ptr(self.entry.d_name.as_ptr()).to_bytes() }
    }
    #[cfg(any(
        target_os = "solaris",
        target_os = "illumos",
        target_os = "fuchsia",
        target_os = "redox"
    ))]
    fn name_bytes(&self) -> &[u8] {
        compile_error!("implement me");
        //&*self.name
    }

    pub fn file_name_os_str(&self) -> &std::ffi::OsStr {
        use std::os::unix::ffi::OsStrExt;
        std::ffi::OsStr::from_bytes(self.name_bytes())
    }
}

pub struct DirReader {
    dirp: Dir,
    #[cfg(not(any(
        target_os = "solaris",
        target_os = "illumos",
        target_os = "fuchsia",
        target_os = "redox",
    )))]
    end_of_stream: bool,
}

impl Iterator for DirReader {
    type Item = io::Result<DirEntry>;

    #[cfg(any(
        target_os = "solaris",
        target_os = "fuchsia",
        target_os = "redox",
        target_os = "illumos"
    ))]
    fn next(&mut self) -> Option<io::Result<DirEntry>> {
        // TODO: Don't really feel like implementing this until it's actually
        // needed on one these OSes, just due to the annoyance
        compile_error!("implement me please");
        // unsafe {
        //     loop {
        //         // Although readdir_r(3) would be a correct function to use here because
        //         // of the thread safety, on Illumos and Fuchsia the readdir(3C) function
        //         // is safe to use in threaded applications and it is generally preferred
        //         // over the readdir_r(3C) function.
        //         libc::set_errno(0);
        //         let entry_ptr = libc::readdir(self.dirp.0);
        //         if entry_ptr.is_null() {
        //             // null can mean either the end is reached or an error occurred.
        //             // So we had to clear errno beforehand to check for an error now.
        //             return match libc::errno() {
        //                 0 => None,
        //                 e => Some(Err(io::Error::from_raw_os_error(e))),
        //             };
        //         }

        //         let name = (*entry_ptr).d_name.as_ptr();
        //         let namelen = libc::strlen(name) as usize;

        //         let ret = DirEntry {
        //             entry: *entry_ptr,
        //             name: slice::from_raw_parts(name as *const u8, namelen as usize)
        //                 .to_owned()
        //                 .into_boxed_slice(),
        //         };
        //         if ret.name_bytes() != b"." && ret.name_bytes() != b".." {
        //             return Some(Ok(ret));
        //         }
        //     }
        // }
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
                entry: std::mem::zeroed(),
            };
            let mut entry_ptr = std::ptr::null_mut();
            loop {
                let err = libc::readdir64_r(self.dirp.0, &mut ret.entry, &mut entry_ptr);
                if err != 0 {
                    if entry_ptr.is_null() {
                        // We encountered an error (which will be returned in this iteration), but
                        // we also reached the end of the directory stream. The `end_of_stream`
                        // flag is enabled to make sure that we return `None` in the next iteration
                        // (instead of looping forever)
                        self.end_of_stream = true;
                    }
                    return Some(Err(io::Error::from_raw_os_error(err)));
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
pub fn read_dir(root: &impl AsRef<std::ffi::CStr>) -> io::Result<DirReader> {
    let root = root.as_ref();
    unsafe {
        let ptr = libc::opendir(root.as_ptr());
        if ptr.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(DirReader {
                dirp: Dir(ptr),
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
